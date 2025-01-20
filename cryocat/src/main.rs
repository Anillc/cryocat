#![feature(try_blocks)]
mod error;

use std::sync::Arc;

use anyhow::Result;
use clap::{ArgAction, Parser};
use cryocat_common::Packet;
use error::CryoError;
use futures_util::{stream::{SplitSink, SplitStream}, SinkExt, StreamExt};
use tokio::{io::{AsyncReadExt, AsyncWriteExt, Stdin}, net::TcpStream, select, sync::{broadcast, mpsc, Mutex}};
use tokio_tungstenite::{connect_async, tungstenite::{Bytes, Message}, MaybeTlsStream, WebSocketStream};
use tracing::{error, Level};
use tracing_subscriber::util::SubscriberInitExt;
use webrtc::{api::{interceptor_registry::register_default_interceptors, media_engine::MediaEngine, APIBuilder}, data_channel::RTCDataChannel, ice_transport::{ice_connection_state::RTCIceConnectionState, ice_server::RTCIceServer}, interceptor::registry::Registry, peer_connection::{configuration::RTCConfiguration, RTCPeerConnection}};

type WsWrite = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
type WsRead = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

#[derive(Debug, Parser, Clone)]
struct Args {
    #[arg(short, long, action = ArgAction::Count)]
    verbose: u8,
    #[arg(short = 'e', long, env = "SERVER")]
    server: String,
    #[arg(short, long, env = "STUN")]
    stun: String,
    #[arg(short, long, env = "TURN")]
    turn: String,
    #[arg(short = 'u', long, env = "TURN_USERNAME")]
    turn_username: String,
    #[arg(short = 'c', long, env = "TURN_CREDENTIAL")]
    turn_credential: String,
    id: String,
}

struct Cryocat {
    args: Args,
    rtc: Arc<RTCPeerConnection>,
    ws_write: Arc<Mutex<WsWrite>>,
    ws_read: Arc<Mutex<WsRead>>,
    exit: broadcast::Sender<()>,
}

impl Cryocat {
    async fn new(args: Args) -> Result<Cryocat> {
        let (ws, _) = connect_async(args.server.clone()).await?;
        let (ws_write, ws_read) = ws.split();
        let (exit, _) = broadcast::channel(1);
        Ok(Cryocat {
            args: args.clone(),
            rtc: Arc::new(create_rtc_connection(args).await?),
            ws_write: Arc::new(Mutex::new(ws_write)),
            ws_read: Arc::new(Mutex::new(ws_read)),
            exit,
        })
    }

    async fn start(&self) -> Result<()> {
        let start = Packet::Start(self.args.id.clone()).to_json()?;
        self.ws_write.lock().await.send(Message::text(start)).await?;
        Ok(())
    }

    async fn send_candidate(&self) {
        let write = self.ws_write.clone();
        let exit = self.exit.clone();
        self.rtc.on_ice_candidate(Box::new(move |candidate| {
            let write = write.clone();
            let exit = exit.clone();
            Box::pin(async move {
                let result: Result<()> = try {
                    if let Some(candidate) = candidate {
                        let candidate = candidate.to_json()?;
                        let message = Message::text(Packet::Candidate(candidate).to_json()?);
                        write.lock().await.send(message).await?;
                    }
                };
                if let Err(err) = result {
                    error!("{}", err.to_string());
                    exit.send(()).ok();
                }
            })
        }));
    }

    async fn setup(&self) -> mpsc::Receiver<Arc<RTCDataChannel>> {
        let write = self.ws_write.clone();
        let read = self.ws_read.clone();
        let exit = self.exit.clone();
        let rtc = self.rtc.clone();
        let (tx, rx) = mpsc::channel(1);
        tokio::spawn(Self::setup_(write, read, exit, rtc, tx));
        rx
    }

    async fn setup_(
        write: Arc<Mutex<WsWrite>>,
        read: Arc<Mutex<WsRead>>,
        exit: broadcast::Sender<()>,
        rtc: Arc<RTCPeerConnection>,
        tx: mpsc::Sender<Arc<RTCDataChannel>>,
    ) {
        let result: Result<()> = try {
            let mut exit_rx = exit.subscribe();
            loop {
                let mut read = read.lock().await;
                select! {
                    _ = exit_rx.recv() => break,
                    data = read.next() => {
                        if let None | Some(Err(_)) = data {
                            use RTCIceConnectionState::Connected;
                            if rtc.ice_connection_state() == Connected {
                                continue;
                            } else if let Some(err@Err(_)) = data {
                                err?;
                                // make rust-analyzer happy
                                break;
                            } else {
                                Err(CryoError::WebSocketClosed)?;
                            }
                        };
                        let data = data.unwrap().unwrap();
                        let packet = Packet::from_json(data.to_text()?)?;
                        match packet {
                            Packet::RequestOffer => {
                                tx.send(rtc.create_data_channel("channel", None).await?).await?;
                                let offer = rtc.create_offer(None).await?;
                                rtc.set_local_description(offer.clone()).await?;
                                let message = Message::text(Packet::Offer(offer).to_json()?);
                                write.lock().await.send(message).await?;
                            },
                            Packet::Offer(offer) => {
                                let tx = tx.clone();
                                rtc.on_data_channel(Box::new(move |channel| {
                                    let tx = tx.clone();
                                    Box::pin(async move {
                                        tx.send(channel).await.unwrap();
                                    })
                                }));
                                rtc.set_remote_description(offer).await?;
                                let answer = rtc.create_answer(None).await?;
                                rtc.set_local_description(answer.clone()).await?;
                                let message = Message::text(Packet::Answer(answer).to_json()?);
                                write.lock().await.send(message).await?;

                            },
                            Packet::Answer(answer) => {
                                rtc.set_remote_description(answer).await?;
                            },
                            Packet::Candidate(candidate) => {
                                rtc.add_ice_candidate(candidate).await?;
                            },
                            _ => Err(CryoError::UnexpectedPacket)?,
                        }
                    },
                }
            }
        };
        if let Err(err) = result {
            error!("{}", err.to_string());
            exit.send(()).ok();
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let level = match args.verbose {
        0 => Level::ERROR,
        1 => Level::INFO,
        2 => Level::DEBUG,
        _ => Level::TRACE,
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(level)
        .finish().init();

    let cryocat = Cryocat::new(args).await?;
    cryocat.start().await?;
    cryocat.send_candidate().await;
    let mut channel = cryocat.setup().await;

    let exit = cryocat.exit.clone();

    let channel = channel.recv().await.unwrap();
    let channel2 = channel.clone();
    channel.on_open(Box::new(move || {
        Box::pin(async move {
            if let Err(err) = cat(&cryocat, channel2).await {
                error!("{}", err.to_string());
                cryocat.exit.send(()).ok();
            }
        })
    }));

    exit.subscribe().recv().await.ok();
    
    Ok(())
}

async fn cat(cryocat: &Cryocat, channel: Arc<RTCDataChannel>) -> Result<()> {
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let (message_tx, mut message_rx) = tokio::sync::mpsc::channel(10);
    channel.on_message(Box::new(move |message| {
        let message_tx = message_tx.clone();
        Box::pin(async move {
            message_tx.send(message.data).await.unwrap();
        })
    }));

    let exit = cryocat.exit.clone();
    channel.on_close(Box::new(move || {
        let exit = exit.clone();
        Box::pin(async move {
            exit.send(()).ok();
        })
    }));

    let mut exit_rx = cryocat.exit.subscribe();
    loop {
        select! {
            stdin = read_stdin(&mut stdin) => {
                match stdin {
                    Ok(stdin) => channel.send(&Bytes::from(stdin)).await?,
                    Err(err) => {
                        match err.downcast() {
                            Ok(CryoError::EOF) => break,
                            Ok(err) => Err(err)?,
                            Err(err) => Err(err)?,
                        }
                    },
                };
            },
            channel_rx = message_rx.recv() => {
                let channel_rx = channel_rx.ok_or(CryoError::UnexpectedError)?;
                stdout.write_all(&channel_rx).await?;
            },
            _ = exit_rx.recv() => break,
        }
        
    };

    cryocat.exit.send(()).ok();

    Ok(())
}

async fn create_rtc_connection(args: Args) -> Result<RTCPeerConnection> {
    let mut media_engine = MediaEngine::default();
    media_engine.register_default_codecs()?;
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media_engine)?;
    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build();
    let config = RTCConfiguration {
        ice_servers: vec![
            RTCIceServer {
                urls: vec![format!("stun:{}", args.stun)],
                ..Default::default()
            },
            RTCIceServer {
                urls: vec![format!("turn:{}", args.turn)],
                username: args.turn_username,
                credential : args.turn_credential,
            }
        ],
        ..Default::default()
    };
    Ok(api.new_peer_connection(config).await?)
}

async fn read_stdin(stdin: &mut Stdin) -> Result<Vec<u8>> {
    let mut vec = vec![0; 1024];
    let n = match stdin.read(&mut vec).await {
        err@Err(_) => err?,
        Ok(0) => Err(CryoError::EOF)?,
        Ok(n) => n,
    };
    vec.truncate(n);
    Ok(vec)
}

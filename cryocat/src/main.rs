#![feature(try_blocks)]
mod error;

use std::sync::Arc;

use anyhow::Result;
use clap::{ArgAction, Parser};
use cryocat_common::Packet;
use error::CryoError;
use futures_util::{SinkExt, StreamExt};
use tokio::{io::{AsyncReadExt, AsyncWriteExt, Stdin}, select, sync::Mutex};
use tokio_tungstenite::{connect_async, tungstenite::{Bytes, Message}};
use tracing::{error, Level};
use tracing_subscriber::util::SubscriberInitExt;
use webrtc::{api::{interceptor_registry::register_default_interceptors, media_engine::MediaEngine, APIBuilder}, data_channel::RTCDataChannel, ice_transport::{ice_connection_state::RTCIceConnectionState, ice_server::RTCIceServer}, interceptor::registry::Registry, peer_connection::{configuration::RTCConfiguration, RTCPeerConnection}};

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

    let rtc = create_rtc_connection(args.clone()).await?;
    let (channel_tx, mut channel) = tokio::sync::mpsc::channel::<Arc<RTCDataChannel>>(1);

    let (ws, _) = connect_async(args.server).await?;
    let (mut write, mut read) = ws.split();

    let start = Packet::Start(args.id).to_json()?;
    write.send(Message::text(start)).await?;

    let packet = match read.next().await {
        None => Err(CryoError::WebSocketClosed)?,
        Some(data) => Packet::from_json(data?.to_text()?)?,
    };
    match packet {
        Packet::RequestOffer => {
            // offer
            channel_tx.send(rtc.create_data_channel("channel", None).await?).await?;
            let offer = rtc.create_offer(None).await?;
            rtc.set_local_description(offer.clone()).await?;
            let message = Message::text(Packet::Offer(offer).to_json()?);
            write.send(message).await?;
            // answer
            let packet = match read.next().await {
                None => Err(CryoError::WebSocketClosed)?,
                Some(data) => Packet::from_json(data?.to_text()?)?,
            };
            match packet {
                Packet::Answer(answer) => rtc.set_remote_description(answer).await?,
                _ => Err(CryoError::UnexpectedPacket)?,
            };
        },
        Packet::Offer(offer) => {
            rtc.on_data_channel(Box::new(move |channel| {
                let channel_tx = channel_tx.clone();
                Box::pin(async move {
                    channel_tx.send(channel).await.unwrap();
                })
            }));
            rtc.set_remote_description(offer).await?;
            let answer = rtc.create_answer(None).await?;
            rtc.set_local_description(answer.clone()).await?;
            let message = Message::text(Packet::Answer(answer).to_json()?);
            write.send(message).await?;
        },
        _ => Err(CryoError::UnexpectedPacket)?,
    };

    let write = Arc::new(Mutex::new(write));
    rtc.on_ice_candidate(Box::new(move |candidate| {
        let write = write.clone();
        Box::pin(async move {
            if let Some(candidate) = candidate {
                let result: Result<()> = try {
                    let candidate = candidate.to_json()?;
                    let message = Message::text(Packet::Candidate(candidate).to_json()?);
                    let mut write = write.lock().await;
                    write.send(message).await?;
                };
                if let Err(err) = result {
                    error!("{}", err.to_string());
                }
            }
        })
    }));

    let channel = channel.recv().await.unwrap();

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let (message_tx, mut message_rx) = tokio::sync::mpsc::channel(10);
    channel.on_message(Box::new(move |message| {
        let message_tx = message_tx.clone();
        Box::pin(async move {
            message_tx.send(message.data).await.unwrap();
        })
    }));

    let (close_tx, mut close_rx) = tokio::sync::mpsc::channel(1);
    channel.on_close(Box::new(move || {
        let close_tx = close_tx.clone();
        Box::pin(async move {
            close_tx.send(()).await.unwrap();
        })
    }));

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
            data = read.next() => {
                match data {
                    // TODO: check rtc connection
                    None | Some(Err(_)) => {
                        if rtc.ice_connection_state() == RTCIceConnectionState::Connected {
                            continue;
                        } else {
                            break;
                        }
                    },
                    Some(Ok(data)) => {
                        let packet = Packet::from_json(data.to_text()?)?;
                        match packet {
                            Packet::Candidate(candidate) => {
                                rtc.add_ice_candidate(candidate).await?;
                            },
                            _ => Err(CryoError::UnexpectedPacket)?,
                        }
                    },
                }
            },
            _ = close_rx.recv() => break,
        }
        
    };
    
    Ok(())
}

async fn create_rtc_connection(args: Args) -> Result<Arc<RTCPeerConnection>> {
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
    Ok(Arc::new(api.new_peer_connection(config).await?))
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

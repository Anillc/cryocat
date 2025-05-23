#![feature(try_blocks)]
#![feature(async_closure)]
mod error;

use std::{collections::{hash_map::Entry, HashMap}, future::Future, sync::Arc};

use anyhow::Result;
use axum::{extract::{ws::{Message, WebSocket}, State, WebSocketUpgrade}, response::IntoResponse, routing::get, serve, Router};
use clap::{ArgAction, Parser};
use cryocat_common::Packet;
use error::CryoError;
use tokio::{net::TcpListener, select, sync::{broadcast, mpsc, Mutex}};
use tracing::{error, info, Level};
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Debug, Parser)]
struct Args {
    #[arg(short, long, action = ArgAction::Count)]
    verbose: u8,
    #[arg(short, long, default_value = "0.0.0.0:80")]
    bind: String,
}

#[derive(Debug)]
struct Conn {
    disconnect: broadcast::Sender<()>,
    // will be None if channel has established
    channel: Option<(mpsc::Sender<Message>, mpsc::Receiver<Message>)>
}

#[derive(Debug, Default)]
struct AppState {
    conns: Mutex<HashMap<String, Conn>>
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let level = match args.verbose {
        0 => Level::INFO,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stdout)
        .with_max_level(level)
        .finish().init();

    let state = Arc::new(AppState {
        ..Default::default()
    });

    let app = Router::new()
        .route("/", get(ws_handler))
        .with_state(state.clone());

    let listener = TcpListener::bind(args.bind.clone()).await
        .expect(format!("failed to listen {}", args.bind.clone()).as_str());
    info!("listening on {}", args.bind);
    serve(listener, app).await.expect("failed to start app");
}

async fn ws_handler(
    upgrade: WebSocketUpgrade,
    State(state): State<Arc<AppState>>
) -> impl IntoResponse {
    upgrade.on_upgrade(async move |mut socket| {
        match ws(&mut socket, state).await {
            Ok(_) => {},
            Err(err) => {
                error!("{}", err.to_string());
            },
        };
    })
}

async fn ws(socket: &mut WebSocket, state: Arc<AppState>) -> Result<()> {
    let mut id: Option<String> = None;
    let mut disconnect_tx: Option<broadcast::Sender<()>> = None;
    let mut disconnect_rx: Option<broadcast::Receiver<()>> = None;
    let mut channel_tx: Option<mpsc::Sender<Message>> = None;
    let mut channel_rx: Option<mpsc::Receiver<Message>> = None;

    let result: Result<()> = try {
        loop {
            select! {
                data = socket.recv() => {
                    let data = data.ok_or(CryoError::WebSocketClosed)??;
                    let packet = Packet::from_json(data.to_text()?)?;
                    match (&id, packet) {
                        // new connection
                        (None, Packet::Start(new_id)) => {
                            let mut conns = state.conns.lock().await;
                            let conn = match conns.entry(new_id.clone()) {
                                Entry::Occupied(entry) => {
                                    let conn = entry.into_mut();
                                    match conn.channel.take() {
                                        Some((tx, rx)) => {
                                            // set id here to avoid entry removing
                                            id = Some(new_id);
                                            channel_tx = Some(tx);
                                            channel_rx = Some(rx);
                                        },
                                        None => Err(CryoError::ChannelExists)?,
                                    }
                                    conn
                                },
                                Entry::Vacant(entry) => {
                                    let (tx_1, rx_1) = mpsc::channel(10);
                                    let (tx_2, rx_2) = mpsc::channel(10);
                                    id = Some(new_id);
                                    channel_tx = Some(tx_1);
                                    channel_rx = Some(rx_2);
                                    socket.send(Message::text(Packet::RequestOffer.to_json()?)).await?;
                                    entry.insert(Conn {
                                        disconnect: broadcast::channel(1).0,
                                        channel: Some((tx_2, rx_1)),
                                    })
                                },
                            };
                            disconnect_tx = Some(conn.disconnect.clone());
                            disconnect_rx = Some(conn.disconnect.subscribe());
                        },
                        (None, _) | (Some(_), Packet::Start(_)) => Err(CryoError::UnexpectedPacket)?,
                        (Some(_), packet) => {
                            channel_tx.as_ref().unwrap()
                                .send(Message::text(packet.to_json()?)).await?;
                        },
                    }
                },
                Some(data) = as_async(channel_rx.as_mut().map(|rx| rx.recv())) => {
                    match data {
                        None => Err(CryoError::WebSocketClosed)?,
                        Some(data) => socket.send(data).await?,
                    };
                },
                Some(disconnect) = as_async(disconnect_rx.as_mut().map(|rx| rx.recv())) => {
                    disconnect?;
                    break;
                },
            }
        };
    };
    let mut conns = state.conns.lock().await;
    if let Some(id) = id {
        conns.remove(&id);
    }
    if let Some(tx) = disconnect_tx {
        tx.send(())?;
    }
    result
}

async fn as_async<T>(option: Option<impl Future<Output = T>>) -> Option<T> {
    match option {
        Some(future) => Some(future.await),
        None => None,
    }
}

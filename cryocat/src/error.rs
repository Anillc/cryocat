use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryoError {
    #[error("websocket closed")]
    WebSocketClosed,
    #[error("unexpected packet")]
    UnexpectedPacket,
    #[error("eof")]
    EOF,
    #[error("unexpected error")]
    UnexpectedError,
}

use anyhow::Result;
use cryocat_common::Packet;
use futures_util::{stream, SinkExt, StreamExt};
use tokio::{io::AsyncReadExt, select, sync::mpsc};
use tokio_tungstenite::{connect_async, tungstenite::Message};


#[tokio::main]
async fn main() -> Result<()> {
    let url = "ws://localhost:3000";
    let (ws, _) = connect_async(url).await?;
    let (mut write, mut read) = ws.split();

    let start = Packet::Start("114514".to_string()).to_json()?;
    let d = Packet::Description(true, "".to_string()).to_json()?;
    write.send(Message::text(start)).await?;
    write.send(Message::text(d)).await?;
    write.flush().await?;

    loop {
        select! {
            data = read.next() => {
                match data {
                    None => break,
                    Some(data) => {
                        let data = data?;
                        let text = data.to_text()?;
                        print!("{text}");
                    },
                }
            },
            // Some(data) = rx.recv() => {
            //     dbg!(123);
            //     write.feed(data).await?;
            //     write.flush().await?;
            // },
        }
    };

    Ok(())
}

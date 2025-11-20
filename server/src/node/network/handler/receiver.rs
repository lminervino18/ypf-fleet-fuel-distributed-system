use crate::{errors::AppResult, node::message::Message};
use std::sync::Arc;
use tokio::{io::AsyncReadExt, net::tcp::OwnedReadHalf, sync::mpsc::Sender};

pub struct StreamReceiver {
    messages_tx: Arc<Sender<Message>>,
    stream: OwnedReadHalf,
}

impl StreamReceiver {
    pub fn new(messages_tx: Arc<Sender<Message>>, stream: OwnedReadHalf) -> Self {
        Self {
            messages_tx,
            stream,
        }
    }

    pub async fn recv(&mut self) -> AppResult<()> {
        // two bytes for the len of the incoming msg
        let mut len_bytes = [0; size_of::<u16>()];
        self.stream.read_exact(&mut len_bytes).await;
        let len = u16::from_be_bytes(len_bytes) as usize;
        // read to buffer with allocated expected size
        let mut bytes = vec![0; len];
        self.stream.read_exact(&mut bytes).await;
        self.messages_tx.send(bytes.try_into()?).await;
        Ok(())
    }
}

use crate::{
    errors::{AppError, AppResult},
    node::node_message::NodeMessage,
};
use tokio::{io::AsyncWriteExt, net::tcp::OwnedWriteHalf, sync::mpsc::Receiver};

pub struct StreamSender {
    messages_rx: Receiver<NodeMessage>,
    stream: OwnedWriteHalf,
}

impl StreamSender {
    pub fn new(messages_rx: Receiver<NodeMessage>, stream: OwnedWriteHalf) -> Self {
        Self {
            messages_rx,
            stream,
        }
    }

    pub async fn send(&mut self) -> AppResult<()> {
        if let Some(msg) = self.messages_rx.recv().await {
            // payload into bytes
            let bytes: &[u8] = msg.into();
            // 2 bytes for the msg len (in bytes)
            let len: u16 = bytes.len() as u16;
            let mut len_bytes = len.to_be_bytes().to_vec();
            len_bytes.extend(bytes);
            self.stream
                .write_all(&len_bytes)
                .await
                .map_err(|e| AppError::InvalidData {
                    details: e.to_string(),
                })?;

            return Ok(());
        };

        Err(AppError::ChannelClosed)
    }
}

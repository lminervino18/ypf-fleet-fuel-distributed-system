use crate::{
    errors::{AppError, AppResult},
    node::message::Message,
};
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
        self.stream
            .read_exact(&mut len_bytes)
            .await
            .map_err(|_| AppError::ConnectionClosed {
                addr: self.stream.peer_addr().unwrap_or_else(|e| {
                    // this should never happen since the skt was ok at the moment of
                    // initialization of this sender
                    panic!("failed to get peer address during handling of ConnectionClosed: {e}")
                }),
            })?;
        let len = u16::from_be_bytes(len_bytes) as usize;
        // read to buffer with allocated expected size
        let mut bytes = vec![0; len];
        self.stream
            .read_exact(&mut bytes)
            .await
            .map_err(|_| AppError::ConnectionClosed {
                addr: self.stream.peer_addr().unwrap_or_else(|e| {
                    panic!("failed to get peer address during handling of ConnectionClosed: {e}")
                }),
            })?;
        self.messages_tx
            .send(bytes.try_into()?)
            .await
            .map_err(|_| AppError::ChannelClosed)?;
        Ok(())
    }
}

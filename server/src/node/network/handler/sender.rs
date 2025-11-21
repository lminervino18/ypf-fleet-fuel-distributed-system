use crate::errors::{AppError, AppResult};
use tokio::{io::AsyncWriteExt, net::tcp::OwnedWriteHalf, sync::mpsc::Receiver};

pub struct StreamSender<T> {
    messages_rx: Receiver<T>,
    stream: OwnedWriteHalf,
}

impl<T: Into<Vec<u8>>> StreamSender<T> {
    pub fn new(messages_rx: Receiver<T>, stream: OwnedWriteHalf) -> Self {
        Self {
            messages_rx,
            stream,
        }
    }

    pub async fn send(&mut self) -> AppResult<()> {
        if let Some(msg) = self.messages_rx.recv().await {
            // payload into bytes
            let bytes: Vec<u8> = msg.into();
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

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc;
    use tokio::task;

    #[tokio::test]
    async fn test_successful_send_with_valid_stream() {
        let server_address = "127.0.0.1:12348";
        let message = [1, 2, 3, 4, 5];
        let server = task::spawn(async move {
            let (messages_tx, messages_rx) = mpsc::channel(1);
            let (stream, _) = TcpListener::bind(server_address)
                .await
                .unwrap()
                .accept()
                .await
                .unwrap();
            let (_, stream) = stream.into_split();
            let mut sender = StreamSender {
                messages_rx,
                stream,
            };
            messages_tx.send(message).await.unwrap();
            sender.send().await.unwrap();
        });

        tokio::time::sleep(Duration::from_secs(1)).await; // wait for listener
        let mut client_skt = TcpStream::connect(server_address).await.unwrap();
        let mut buf = vec![0u8; message.len() + 2 /* 2 bytes for msg len */];
        let _ = client_skt.read(&mut buf).await.unwrap();
        server.await.unwrap();
        let mut expected: Vec<u8> = (message.len() as u16).to_be_bytes().to_vec();
        expected.extend(message);
        assert_eq!(buf, expected);
    }
}

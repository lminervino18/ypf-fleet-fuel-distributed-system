use crate::{
    errors::{AppError, AppResult},
    network::serials::protocol::{HEARBEAT_REPLY, HEARTBEAT_REQUEST},
};
use std::net::SocketAddr;
use tokio::{io::AsyncWriteExt, net::tcp::OwnedWriteHalf, sync::mpsc::Receiver};

pub struct StreamSender<T> {
    messages_rx: Receiver<T>,
    stream: OwnedWriteHalf,
    address: SocketAddr,
}

impl<T: Into<Vec<u8>>> StreamSender<T> {
    pub fn new(messages_rx: Receiver<T>, stream: OwnedWriteHalf, address: SocketAddr) -> Self {
        Self {
            messages_rx,
            stream,
            address,
        }
    }

    pub async fn send_heartbeat_request(&mut self) -> AppResult<()> {
        let len = size_of::<u8>() as u16; // u8 medio hardcoded pero es el len del hearbeat
        let mut len_bytes = len.to_be_bytes().to_vec();
        len_bytes.push(HEARTBEAT_REQUEST);
        self.write_all_bytes(&len_bytes).await
    }

    pub async fn send_heartbeat_reply(&mut self) -> AppResult<()> {
        let len = size_of::<u8>() as u16;
        let mut len_bytes = len.to_be_bytes().to_vec();
        len_bytes.push(HEARBEAT_REPLY);
        self.write_all_bytes(&len_bytes).await
    }

    pub async fn send(&mut self) -> AppResult<()> {
        if let Some(msg) = self.messages_rx.recv().await {
            // payload into bytes
            let bytes: Vec<u8> = msg.into();
            // 2 bytes for the msg len (in bytes)
            let len: u16 = bytes.len() as u16;
            let mut len_bytes = len.to_be_bytes().to_vec();
            len_bytes.extend(bytes);
            return self.write_all_bytes(&len_bytes).await;
        };

        Err(AppError::ChannelClosed)
    }

    async fn write_all_bytes(&mut self, bytes: &[u8]) -> AppResult<()> {
        self.stream
            .write_all(bytes)
            .await
            .map_err(|_| AppError::ConnectionLostWith {
                address: self.address,
            })?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc;
    use tokio::task;

    #[tokio::test]
    async fn test_successful_send_with_valid_stream() {
        let server_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12348);
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
            let mut sender = StreamSender::new(messages_rx, stream, server_address);
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

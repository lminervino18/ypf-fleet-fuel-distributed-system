use crate::errors::{AppError, AppResult};
use std::sync::Arc;
use tokio::{io::AsyncReadExt, net::tcp::OwnedReadHalf, sync::mpsc::Sender};

pub struct StreamReceiver<T> {
    messages_tx: Arc<Sender<T>>,
    stream: OwnedReadHalf,
}

impl<T> StreamReceiver<T>
where
    T: TryFrom<Vec<u8>>,
    T::Error: Into<AppError>,
{
    pub fn new(messages_tx: Arc<Sender<T>>, stream: OwnedReadHalf) -> Self {
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
            .map_err(|_| AppError::ConnectionLostWith {
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
            .map_err(|_| AppError::ConnectionLostWith {
                addr: self.stream.peer_addr().unwrap_or_else(|e| {
                    panic!("failed to get peer address during handling of ConnectionClosed: {e}")
                }),
            })?;
        self.messages_tx
            .send(bytes.try_into().map_err(Into::into)?)
            .await
            .map_err(|_| AppError::ChannelClosed)?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Message;
    use tokio::{
        io::AsyncWriteExt,
        net::{TcpListener, TcpStream},
        sync::mpsc,
        task,
    };

    #[tokio::test]
    async fn test_successful_receive_with_valid_stream() {
        let sent_message = Message::Ack { op_id: 0 };
        let sent_message_copy = sent_message.clone();
        let server_address = "127.0.0.1:12350";
        let client = task::spawn(async move {
            let mut client_skt = TcpStream::connect(server_address).await.unwrap();
            let mut message: Vec<u8> = 5u16.to_be_bytes().to_vec(); // 5 is the len of the ack msg
            let sent_message_srl: Vec<u8> = sent_message_copy.into();
            message.extend(sent_message_srl);
            let _ = client_skt.write(&message).await.unwrap();
        });

        let (stream, _) = TcpListener::bind(server_address)
            .await
            .unwrap()
            .accept()
            .await
            .unwrap();
        let (stream_rx, _) = stream.into_split();
        let (messages_tx, mut messages_rx) = mpsc::channel::<Message>(1);
        let messages_tx = Arc::new(messages_tx);
        let mut receiver = StreamReceiver::new(messages_tx, stream_rx);
        client.await.unwrap();
        receiver.recv().await.unwrap();
        let received_msg = messages_rx.recv().await.unwrap();
        assert_eq!(received_msg, sent_message);
    }
}

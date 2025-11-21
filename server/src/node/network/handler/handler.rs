use super::{receiver::StreamReceiver, sender::StreamSender};
use crate::{
    errors::{AppError, AppResult},
    node::message::Message,
};
use std::{net::SocketAddr, sync::Arc, time::Instant};
use tokio::{
    net::TcpStream,
    select,
    sync::mpsc::{self, Receiver, Sender},
    task::{self, JoinHandle},
};

const MSG_BUFF_SIZE: usize = 10;

pub struct Handler {
    handle: JoinHandle<()>,
    messages_tx: Sender<Message>,
    pub address: SocketAddr,
    pub last_used: Instant,
}

impl Handler {
    pub async fn start(address: &SocketAddr, receiver_tx: Arc<Sender<Message>>) -> AppResult<Self> {
        let stream =
            TcpStream::connect(address)
                .await
                .map_err(|_| AppError::ConnectionRefused {
                    addr: address.to_string(),
                })?;

        Self::start_from(stream, receiver_tx).await
    }

    pub async fn start_from(
        stream: TcpStream,
        receiver_tx: Arc<Sender<Message>>,
    ) -> AppResult<Self> {
        let (messages_tx, sender_rx) = mpsc::channel(MSG_BUFF_SIZE);
        let address = stream.local_addr().map_err(|e| AppError::Unexpected {
            details: e.to_string(), // FIXME: creo q esto tendría q ser peer_addr pero lo acabo de
                                    // ver...
        })?;

        Handler::new(stream, messages_tx, sender_rx, receiver_tx, address).await
    }

    pub async fn send(&mut self, msg: Message) -> AppResult<()> {
        self.last_used = Instant::now();
        self.messages_tx
            .send(msg)
            .await
            .map_err(|_| AppError::ConnectionClosed { addr: self.address })
    }

    pub fn stop(&mut self) {
        self.handle.abort();
    }

    async fn new(
        stream: TcpStream,
        messages_tx: Sender<Message>,
        sender_rx: Receiver<Message>,
        receiver_tx: Arc<Sender<Message>>,
        address: SocketAddr,
    ) -> AppResult<Self> {
        let (stream_rx, stream_tx) = stream.into_split();
        let sender = StreamSender::new(sender_rx, stream_tx);
        let receiver = StreamReceiver::new(receiver_tx, stream_rx);
        Ok(Self {
            handle: Self::run(sender, receiver),
            messages_tx,
            address,
            last_used: Instant::now(),
        })
    }

    fn run(
        mut sender: StreamSender<Message>,
        mut receiver: StreamReceiver<Message>,
    ) -> JoinHandle<()> {
        task::spawn(async move {
            loop {
                select! {
                        sent = sender.send() => { match sent {
                            Ok(()) => {},
                            Err(AppError::ChannelClosed) => { break; },
                            _ => todo!(),
                        }},
                        received = receiver.recv() => { match received {
                                Ok(()) => {},
                                Err(AppError::ChannelClosed) => { break; },
                                _ => todo!(),
                            }
                        }
                }
            }
        })
    }
}

impl Drop for Handler {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::{
        net::{IpAddr, Ipv4Addr},
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task,
    };

    #[tokio::test]
    async fn test_succesful_recv_with_a_valid_stream() {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12351);
        let message = Message::Ack { id: 1 };
        let message_copy = message.clone();
        let handle = task::spawn(async move {
            let (mut stream, _) = TcpListener::bind(address)
                .await
                .unwrap()
                .accept()
                .await
                .unwrap();

            let mut len_srl = 5u16.to_be_bytes().to_vec();
            let message_srl: Vec<u8> = message_copy.into();
            len_srl.extend(message_srl);
            let _ = stream.write(&len_srl).await.unwrap();
        });

        tokio::time::sleep(Duration::from_secs(1)).await; // wait for listener
        let (receiver_tx, mut receiver_rx) = mpsc::channel(1);
        let handler = Handler::start(&address, Arc::new(receiver_tx))
            .await
            .unwrap();
        let received = receiver_rx.recv().await.unwrap();
        assert_eq!(received, message);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_succesful_send_with_a_valid_stream() {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12352);
        let message = Message::Ack { id: 1 };
        let message_copy = message.clone();
        let listener = TcpListener::bind(address).await.unwrap();
        let handle = task::spawn(async move {
            let (receiver_tx, _) = mpsc::channel(1);
            let mut handler = Handler::start(&address, Arc::new(receiver_tx))
                .await
                .unwrap();
            handler.send(message_copy).await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 64];
        let _ = stream.read(&mut buf).await.unwrap();
        buf = buf[2..].to_vec();
        let received: Message = buf.try_into().unwrap();
        assert_eq!(received, message);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_successful_send_and_receive_between_two_handlers() {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12353);
        let message = Message::Ack { id: 1 };
        let message_copy = message.clone();
        let listener = TcpListener::bind(address).await.unwrap();
        let handle1 = task::spawn(async move {
            let (receiver_tx, _) = mpsc::channel(1);
            let mut handler = Handler::start(&address, Arc::new(receiver_tx))
                .await
                .unwrap();
            handler.send(message_copy).await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        let (stream2, _) = listener.accept().await.unwrap();
        let (receiver_tx, mut receiver_rx) = mpsc::channel(1);
        let handler2 = Handler::start_from(stream2, Arc::new(receiver_tx))
            .await
            .unwrap();
        let received = receiver_rx.recv().await.unwrap();
        assert_eq!(received, message);
        handle1.await.unwrap();
    }
}

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
            details: e.to_string(),
        })?;

        Ok(Handler::new(stream, messages_tx, sender_rx, receiver_tx, address).await?)
    }

    pub async fn send(&mut self, msg: Message) -> AppResult<()> {
        self.last_used = Instant::now();
        Ok(self
            .messages_tx
            .send(msg)
            .await
            .map_err(|_| AppError::ConnectionClosed {})?)
    }

    pub async fn stop(&mut self) {
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

    fn run(mut sender: StreamSender, mut receiver: StreamReceiver) -> JoinHandle<()> {
        task::spawn(async move {
            loop {
                select! {
                        sent = sender.send() => { match sent {
                            Err(AppError::ChannelClosed) => { break; },
                            _ => todo!(),
                        }},
                        received = receiver.recv() => {
                            match received {
                                _ => todo!(),
                            }
                        }
                }
            }
        })
    }
}

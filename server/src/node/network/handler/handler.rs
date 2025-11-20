use super::{receiver::StreamReceiver, sender::StreamSender};
use crate::{
    errors::{AppError, AppResult},
    node::node_message::NodeMessage,
};
use std::{net::SocketAddr, time::Instant};
use tokio::{
    net::TcpStream,
    select,
    sync::mpsc::{self, Receiver, Sender},
    task::{self, JoinHandle},
};

const MSG_BUFF_SIZE: usize = 10;

pub struct Handler {
    handle: JoinHandle<()>,
    messages_tx: Sender<NodeMessage>,
    messages_rx: Receiver<NodeMessage>,
    last_used: Instant,
}

impl Handler {
    pub async fn start(address: SocketAddr) -> AppResult<Self> {
        let stream =
            TcpStream::connect(address)
                .await
                .map_err(|_| AppError::ConnectionRefused {
                    addr: address.to_string(),
                })?;

        Self::start_from(stream).await
    }

    pub async fn start_from(stream: TcpStream) -> AppResult<Self> {
        let (messages_tx, sender_rx) = mpsc::channel(MSG_BUFF_SIZE);
        let (receiver_tx, messages_rx) = mpsc::channel(MSG_BUFF_SIZE);
        Ok(Handler::new(stream, messages_tx, messages_rx, sender_rx, receiver_tx).await?)
    }

    pub async fn send(&mut self, msg: NodeMessage) -> AppResult<()> {
        Ok(self
            .messages_tx
            .send(msg)
            .await
            .map_err(|_| AppError::ConnectionClosed {})?)
    }

    pub async fn recv(&mut self) -> AppResult<NodeMessage> {
        self.messages_rx
            .recv()
            .await
            .ok_or(AppError::ConnectionClosed {})
    }

    pub async fn stop(&mut self) {
        self.handle.abort();
    }

    async fn new(
        stream: TcpStream,
        messages_tx: Sender<NodeMessage>,
        messages_rx: Receiver<NodeMessage>,
        sender_rx: Receiver<NodeMessage>,
        receiver_tx: Sender<NodeMessage>,
    ) -> AppResult<Self> {
        let (stream_rx, stream_tx) = stream.into_split();
        let sender = StreamSender::new(sender_rx, stream_tx);
        let receiver = StreamReceiver::new(receiver_tx, stream_rx);
        Ok(Self {
            handle: Self::run(sender, receiver),
            messages_tx,
            messages_rx,
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

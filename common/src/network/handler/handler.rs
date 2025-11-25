use super::{receiver::StreamReceiver, sender::StreamSender};
use crate::{
    errors::{AppError, AppResult},
    network::serials::{read_handler_first_message, send_handler_first_message},
    Message,
};
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::TcpStream,
    select,
    sync::mpsc::{self, Receiver, Sender},
    task::{self, JoinHandle},
    time::sleep,
};

pub enum MessageKind {
    HeartbeatRequest,
    HeartbeatReply,
    NodeMessage,
}

use MessageKind::*;

const HEARTBEAT_FREQUENCY: Duration = Duration::from_millis(500);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);
const MSG_BUFF_SIZE: usize = 512;

#[derive(Debug)]
pub struct Handler {
    handle: JoinHandle<AppResult<()>>,
    messages_tx: Sender<Message>,
    pub address: SocketAddr,
    pub last_used: Instant,
}

impl Handler {
    pub async fn start(
        self_addr: SocketAddr,
        peer_addr: SocketAddr,
        receiver_tx: Arc<Sender<AppResult<Message>>>,
    ) -> AppResult<Self> {
        println!("[HANDLER] starting active handler with address: {}", {
            peer_addr
        });
        let mut stream =
            TcpStream::connect(peer_addr)
                .await
                .map_err(|_| AppError::ConnectionRefused {
                    address: peer_addr.to_string(),
                })?;

        send_handler_first_message(self_addr, &mut stream).await?;
        let (messages_tx, sender_rx) = mpsc::channel(MSG_BUFF_SIZE);
        Handler::new(stream, messages_tx, sender_rx, receiver_tx, peer_addr).await
    }

    pub async fn start_from(
        mut stream: TcpStream,
        receiver_tx: Arc<Sender<AppResult<Message>>>,
    ) -> AppResult<Self> {
        let address = read_handler_first_message(&mut stream).await?;
        println!("[HANDLER] starting pasive handler with address: {}", {
            address
        });
        let (messages_tx, sender_rx) = mpsc::channel(MSG_BUFF_SIZE);
        Handler::new(stream, messages_tx, sender_rx, receiver_tx, address).await
    }

    pub async fn send(&mut self, msg: Message) -> AppResult<()> {
        self.last_used = Instant::now();
        self.messages_tx
            .send(msg)
            .await
            .map_err(|_| AppError::ConnectionLostWith {
                address: self.address,
            })
    }

    pub fn stop(&mut self) {
        self.handle.abort();
    }

    async fn new(
        stream: TcpStream,
        messages_tx: Sender<Message>,
        sender_rx: Receiver<Message>,
        receiver_tx: Arc<Sender<AppResult<Message>>>,
        address: SocketAddr,
    ) -> AppResult<Self> {
        let (stream_rx, stream_tx) = stream.into_split();
        let sender = StreamSender::new(sender_rx, stream_tx, address);
        let receiver = StreamReceiver::new(receiver_tx, stream_rx, address);
        Ok(Self {
            handle: Self::run(sender, receiver, address),
            messages_tx,
            address,
            last_used: Instant::now(),
        })
    }

    async fn handle_recv_result(
        sender: &mut StreamSender<Message>,
        received: AppResult<MessageKind>,
        last_seen: &mut Instant,
        _address: SocketAddr, // sólo para printear debug
    ) -> AppResult<()> {
        match received {
            Ok(msg_kind) => match msg_kind {
                HeartbeatRequest => Ok(sender.send_heartbeat_reply().await?),
                HeartbeatReply => {
                    *last_seen = Instant::now();
                    Ok(())
                }
                NodeMessage => Ok(()),
            },
            Err(e) => Err(e),
        }
    }

    // este run se podría alindar un toque ...
    fn run(
        mut sender: StreamSender<Message>,
        mut receiver: StreamReceiver<Message>,
        address: SocketAddr, // sólo para printear debugs
    ) -> JoinHandle<AppResult<()>> {
        task::spawn(async move {
            let mut last_seen = Instant::now();
            loop {
                let mut result = Ok(());
                select! {
                        // mando msjs q el handle escribió en el mpsc
                        sent = sender.send() => result = sent,
                        // leo por recv stream y escribo en el mpsc que tiene Connection
                        received = receiver.recv() =>
                            result = Self::handle_recv_result(&mut sender, received, &mut last_seen, address).await,
                        // si se cumplió esta duration entonces mando hearbeat
                        _ = sleep(HEARTBEAT_FREQUENCY) => {
                            sender.send_heartbeat_request().await?;
                        }
                }

                match result {
                    Ok(()) => {}
                    Err(AppError::ChannelClosed) => break,
                    Err(AppError::ConnectionLostWith { address }) => {
                        println!("[HANDLER] connection lost with: {}", address);
                        receiver.write_connection_lost().await?;
                        break;
                        // return Err(AppError::ConnectionLostWith { address });
                    }
                    x => return x,
                };
                // cada vez que hice alguna de las tres cosas anteriores me fijo si hay timeout del
                // heartbeat
                if Instant::now() - last_seen > HEARTBEAT_TIMEOUT {
                    println!("[HANDLER] heartbeat timeout, writting connection lost");
                    receiver.write_connection_lost().await?;
                    break;
                    // return Err(AppError::ConnectionLostWith { address });
                }
            }

            Ok(())
        })
    }
}

impl Drop for Handler {
    fn drop(&mut self) {
        // TODO: acá tendría que mandar None pero el channel ya es de msg, se podría cambiar
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
        let message = Message::Ack { op_id: 1 };
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
        let _handler = Handler::start(address, address, Arc::new(receiver_tx))
            .await
            .unwrap();
        let received = receiver_rx.recv().await.unwrap();
        assert_eq!(received, Ok(message));
        handle.await.unwrap();
    }

    #[ignore = "no funca porque ahora está el first message"]
    #[tokio::test]
    async fn test_succesful_send_with_a_valid_stream() {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12352);
        let message = Message::Ack { op_id: 1 };
        let message_copy = message.clone();
        let listener = TcpListener::bind(address).await.unwrap();
        let handle = task::spawn(async move {
            let (receiver_tx, _) = mpsc::channel(1);
            let mut handler = Handler::start(address, address, Arc::new(receiver_tx))
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
        let message = Message::Ack { op_id: 1 };
        let message_copy = message.clone();
        let listener = TcpListener::bind(address).await.unwrap();
        let handle1 = task::spawn(async move {
            let (receiver_tx, _) = mpsc::channel(1);
            let mut handler = Handler::start(address, address, Arc::new(receiver_tx))
                .await
                .unwrap();
            handler.send(message_copy).await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });

        let (stream2, _) = listener.accept().await.unwrap();
        let (receiver_tx, mut receiver_rx) = mpsc::channel(1);
        let _handler2 = Handler::start_from(stream2, Arc::new(receiver_tx))
            .await
            .unwrap();
        let received = receiver_rx.recv().await.unwrap();
        assert_eq!(received, Ok(message));
        handle1.await.unwrap();
    }

    // #[tokio::test]
    async fn tesst_send_result_in_connection_lost_with_if_peer_is_down() {}
}

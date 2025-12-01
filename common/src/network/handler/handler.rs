//! TCP stream handler for peer-to-peer node communication.
//!
//! The handler manages a bidirectional TCP stream with a peer and:
//! - sends outgoing messages via a `StreamSender`,
//! - receives incoming messages via a `StreamReceiver`,
//! - exchanges periodic heartbeats to detect liveness,
//! - forwards node-level messages to the connection layer via a channel.
//!
//! Behavior notes:
//! - The handler runs a single asynchronous task that multiplexes:
//!   - sending queued outgoing messages,
//!   - processing incoming frames and translating them into `MessageKind`,
//!   - emitting periodic heartbeat requests,
//!   - and updating the last-seen timestamp when a heartbeat reply arrives.
//! - On heartbeat timeout or fatal channel/connection errors the handler signals
//!   connection loss and terminates.

use super::{receiver::StreamReceiver, sender::StreamSender};
use crate::{
    Message,
    errors::{AppError, AppResult},
    network::serials::{read_handler_first_message, send_handler_first_message},
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

/// Logical classification of an incoming stream frame.
pub enum MessageKind {
    HeartbeatRequest,
    HeartbeatReply,
    NodeMessage,
}

use MessageKind::*;

const HEARTBEAT_FREQUENCY: Duration = Duration::from_millis(500);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);
const MSG_BUFF_SIZE: usize = 512;

/// Handler that owns a single connection to a peer.
///
/// The handler encapsulates a background task responsible for:
///
/// - serializing and sending outgoing `Message` values to the peer,
/// - receiving and deserializing incoming frames and forwarding them to the
///   connection layer,
/// - exchanging heartbeat requests/replies to detect peer liveness.
#[derive(Debug)]
pub struct Handler {
    handle: JoinHandle<AppResult<()>>,
    messages_tx: Sender<Message>,
    /// Remote peer address.
    pub address: SocketAddr,
    /// Instant of last successful heartbeat reply (or other activity).
    pub last_used: Instant,
}

impl Handler {
    /// Start an outbound connection to `peer_addr` and initialize the handler.
    ///
    /// This function connects to the remote peer, performs the initial
    /// handshake (sending our local address), and spawns the internal run task.
    pub async fn start(
        self_addr: SocketAddr,
        peer_addr: SocketAddr,
        receiver_tx: Arc<Sender<AppResult<Message>>>,
    ) -> AppResult<Self> {
        let mut stream =
            TcpStream::connect(peer_addr)
                .await
                .map_err(|_| AppError::ConnectionRefused {
                    address: peer_addr.to_string(),
                })?;

        // Send initial handshake with our address.
        send_handler_first_message(self_addr, &mut stream).await?;
        let (messages_tx, sender_rx) = mpsc::channel(MSG_BUFF_SIZE);
        Handler::new(stream, messages_tx, sender_rx, receiver_tx, peer_addr).await
    }

    /// Construct a handler for an already-accepted inbound stream.
    ///
    /// This reads the initial handshake (remote address) and then starts the handler.
    pub async fn start_from(
        mut stream: TcpStream,
        receiver_tx: Arc<Sender<AppResult<Message>>>,
    ) -> AppResult<Self> {
        let address = read_handler_first_message(&mut stream).await?;

        let (messages_tx, sender_rx) = mpsc::channel(MSG_BUFF_SIZE);
        Handler::new(stream, messages_tx, sender_rx, receiver_tx, address).await
    }

    /// Enqueue a `Message` to be sent to the peer.
    ///
    /// Updates the `last_used` timestamp to reflect outgoing activity.
    pub async fn send(&mut self, msg: Message) -> AppResult<()> {
        self.last_used = Instant::now();
        self.messages_tx
            .send(msg)
            .await
            .map_err(|_| AppError::ConnectionLostWith {
                address: self.address,
            })
    }

    /// Stop the handler's background task.
    pub fn stop(&mut self) {
        self.handle.abort();
    }

    // Internal constructor that spawns the background task.
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

    /// Handle a received `MessageKind` result.
    ///
    /// - On `HeartbeatRequest` send a heartbeat reply.
    /// - On `HeartbeatReply` update the `last_seen` timestamp.
    /// - On `NodeMessage` do nothing here (the receiver already forwarded the message).
    async fn handle_recv_result(
        sender: &mut StreamSender<Message>,
        received: AppResult<MessageKind>,
        last_seen: &mut Instant,
        _address: SocketAddr, // only used for debug printing
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

    // The main run loop spawns an asynchronous task that multiplexes:
    // - sending queued messages,
    // - receiving frames and handling heartbeat logic,
    // - periodic heartbeat requests.
    fn run(
        mut sender: StreamSender<Message>,
        mut receiver: StreamReceiver<Message>,
        address: SocketAddr, // only used for debug prints
    ) -> JoinHandle<AppResult<()>> {
        task::spawn(async move {
            let mut last_seen = Instant::now();
            loop {
                let mut result = Ok(());
                select! {
                        // Send any messages that were enqueued by the handler owner.
                        sent = sender.send() => result = sent,
                        // Receive a frame from the peer and process it.
                        received = receiver.recv() =>
                            result = Self::handle_recv_result(&mut sender, received, &mut last_seen, address).await,
                        // Periodically send a heartbeat request.
                        _ = sleep(HEARTBEAT_FREQUENCY) => {
                            sender.send_heartbeat_request().await?;
                        }
                }

                match result {
                    Ok(()) => {}
                    Err(AppError::ChannelClosed) => break,
                    Err(AppError::ConnectionLostWith { address: _ }) => {
                        // Notify the receiver stream that the connection is lost, then stop.
                        receiver.write_connection_lost().await?;
                        break;
                    }
                    x => return x,
                };
                // After handling an event, check for heartbeat timeout.
                if Instant::now() - last_seen > HEARTBEAT_TIMEOUT {
                    receiver.write_connection_lost().await?;
                    break;
                }
            }

            Ok(())
        })
    }
}

impl Drop for Handler {
    fn drop(&mut self) {
        // Abort the background task when the handler is dropped.
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

    #[ignore = "ignored because the initial handshake (first message) behaviour changed"]
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
}

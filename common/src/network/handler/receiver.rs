//! Stream receiver for peer TCP connections.
//!
//! This module provides `StreamReceiver`, a thin wrapper that reads framed
//! messages from a peer TCP stream and forwards them to the connection layer
//! via an mpsc channel. It recognizes lightweight heartbeat frames and
//! classifies other frames as node messages to be deserialized by the caller.
//!
//! Responsibilities:
//! - read a length-prefixed frame from the TCP stream,
//! - detect heartbeat request/reply frames and report them as `MessageKind`,
 //! - forward normal frames as serialized `T` on `messages_tx`.
//!
//! Errors are reported by sending an `AppError` through `messages_tx` so the
//! owning connection can react (for example, by tearing down the peer).

use super::handler::MessageKind::{self, *};
use crate::errors::{AppError, AppResult};
use crate::network::serials::protocol::{HEARBEAT_REPLY, HEARTBEAT_REQUEST};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::{io::AsyncReadExt, net::tcp::OwnedReadHalf, sync::mpsc::Sender};
use std::mem::size_of;

pub struct StreamReceiver<T> {
    messages_tx: Arc<Sender<AppResult<T>>>,
    stream: OwnedReadHalf,
    address: SocketAddr,
}

impl<T> StreamReceiver<T>
where
    T: TryFrom<Vec<u8>>,
    T::Error: Into<AppError>,
{
    pub fn new(
        messages_tx: Arc<Sender<AppResult<T>>>,
        stream: OwnedReadHalf,
        address: SocketAddr,
    ) -> Self {
        Self {
            messages_tx,
            stream,
            address,
        }
    }

    /// Send a `ConnectionLostWith { address }` error into `messages_tx`.
    ///
    /// This is used by the handler to notify the connection layer that the
    /// peer stream has been lost.
    pub async fn write_connection_lost(&mut self) -> AppResult<()> {
        self.messages_tx
            .send(Err(AppError::ConnectionLostWith {
                address: self.address,
            }))
            .await
            .map_err(|_| AppError::ChannelClosed)?;

        Ok(())
    }

    /// Read the next framed message from the stream and classify it.
    ///
    /// The frame format is:
    /// - 2 bytes big-endian length (u16), followed by `len` bytes of payload.
    ///
    /// Returns:
    /// - `Ok(HeartbeatRequest)` or `Ok(HeartbeatReply)` for heartbeat frames,
    /// - `Ok(NodeMessage)` after sending the deserialized value via `messages_tx`.
    /// - An error (`AppError`) if the stream is closed or deserialization fails.
    pub async fn recv(&mut self) -> AppResult<MessageKind> {
        // Read two bytes for the length of the incoming message.
        let mut len_bytes = [0; size_of::<u16>()];
        self.stream
            .read_exact(&mut len_bytes)
            .await
            .map_err(|_| AppError::ConnectionLostWith {
                address: self.address,
            })?;
        let len = u16::from_be_bytes(len_bytes) as usize;

        // Read the expected number of bytes into a buffer.
        let mut bytes = vec![0; len];
        self.stream
            .read_exact(&mut bytes)
            .await
            .map_err(|_| AppError::ConnectionLostWith {
                address: self.address,
            })?;

        // Detect special heartbeat frames.
        match bytes[..] {
            [HEARTBEAT_REQUEST] => return Ok(HeartbeatRequest),
            [HEARBEAT_REPLY] => return Ok(HeartbeatReply),
            _ => { /* normal flow: forward payload for deserialization */ }
        }

        // Attempt to deserialize and forward the message to the connection layer.
        self.messages_tx
            .send(Ok(bytes.try_into().map_err(Into::into)?))
            .await
            .map_err(|_| AppError::ChannelClosed)?;

        Ok(NodeMessage)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Message;
    use std::net::IpAddr;
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
        let server_address = SocketAddr::new(IpAddr::V4([127, 0, 0, 1].into()), 12350);
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
        let (messages_tx, mut messages_rx) = mpsc::channel::<AppResult<Message>>(1);
        let messages_tx = Arc::new(messages_tx);
        let mut receiver = StreamReceiver::new(messages_tx, stream_rx, server_address);
        client.await.unwrap();
        receiver.recv().await.unwrap();
        let received_msg = messages_rx.recv().await.unwrap();
        assert_eq!(received_msg, Ok(sent_message));
    }
}

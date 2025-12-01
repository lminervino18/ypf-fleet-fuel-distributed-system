//! Handshake helpers for initializing a handler connection.
//!
//! This module provides two small helpers used during the initial TCP
//! handshake between peers when establishing a `Handler`:
//!
//! - `send_handler_first_message(address, stream)`
//!   sends a short, length-prefixed "first message" containing a single
//!   message-type byte followed by the local socket address.
//!
//! - `read_handler_first_message(stream)`
//!   reads the expected fixed-size first message from the peer and returns
//!   the peer's socket address parsed from the payload.
//!
//! These helpers are intentionally minimal and perform only the framing and
//! (de)serialization required to bootstrap an authenticated peer identity for
//! the connection handler. On IO failures they map to `AppError::ConnectionLostWith`.
use super::{
    deserialization::deserialize_socket_address_srl, protocol::MSG_TYPE_HANDLER_FIRST,
    serialization::serialize_socket_address,
};
use crate::{AppError, AppResult};
use std::net::SocketAddr;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

/// Send the initial handler-first message over `stream`.
///
/// The wire format is:
/// - 2 bytes BE length (u16) of the remaining payload,
/// - 1 byte message type (`MSG_TYPE_HANDLER_FIRST`),
/// - 6 bytes serialized local socket address (IPv4 + port).
///
/// Returns `Ok(())` on success, or `Err(AppError::ConnectionLostWith { address })`
/// if the write fails.
pub async fn send_handler_first_message(
    address: SocketAddr,
    stream: &mut TcpStream,
) -> AppResult<()> {
    let type_srl = [MSG_TYPE_HANDLER_FIRST];
    let address_srl: [u8; 6] = serialize_socket_address(address);
    let mut srl = [type_srl.as_slice(), address_srl.as_slice()].concat();
    let len_srl = (srl.len() as u16).to_be_bytes();
    srl = [len_srl.as_slice(), srl.as_slice()].concat();
    stream
        .write_all(&srl)
        .await
        .map_err(|_| AppError::ConnectionLostWith { address })?;

    Ok(())
}

/// Read and parse the handler-first message from `stream`.
///
/// This function expects exactly the fixed-size frame produced by
/// `send_handler_first_message` and returns the parsed `SocketAddr` that the
/// remote peer announced. On IO errors it returns
/// `AppError::ConnectionLostWith { address }`.
pub async fn read_handler_first_message(stream: &mut TcpStream) -> AppResult<SocketAddr> {
    let mut buf = [0u8; 9];
    stream.read_exact(&mut buf).await.map_err(|_| AppError::ConnectionLostWith { address: stream.peer_addr().expect("failed to read peer_addr while mapping connection_lost_with error in handler start from") })?;
    let address_srl = &buf[3..9];
    let address = deserialize_socket_address_srl(address_srl)?;
    Ok(address)
}

#[cfg(test)]
mod test {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tokio::{
        net::{TcpListener, TcpStream},
        task::{self, yield_now},
    };

    #[tokio::test]
    async fn test_successful_send_and_recv_handler_first_message() {
        let address1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12372);
        let listener1 = TcpListener::bind(address1).await.unwrap();
        let handle1 = task::spawn(async move { listener1.accept().await.unwrap() });
        yield_now().await;
        let mut stream2 = TcpStream::connect(address1).await.unwrap();
        let (mut stream1, _) = handle1.await.unwrap();
        send_handler_first_message(address1, &mut stream1)
            .await
            .unwrap();
        let received = read_handler_first_message(&mut stream2).await.unwrap();
        assert_eq!(received, address1);
    }
}

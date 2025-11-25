use super::{
    deserialize_socket_address_srl, protocol::MSG_TYPE_HANDLER_FIRST, serialize_socket_address,
};
use crate::{AppError, AppResult};
use std::net::SocketAddr;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

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

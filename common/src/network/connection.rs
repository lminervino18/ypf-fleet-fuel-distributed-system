//! High-level connection abstraction used by the networking layer.
//!
//! `Connection` manages a set of active `Handler` instances, an acceptor task
//! that listens for inbound connections, and a channel for inbound `Message`
//! events. It provides simple `send`/`recv` methods used by higher-level
//! components to communicate with peers. The implementation includes helpers
//! to simulate disconnections (used in tests).

use super::{Message, acceptor::Acceptor, active_helpers::add_handler_from, handler::Handler};
use crate::errors::{AppError, AppResult};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{
    sync::{
        Mutex,
        mpsc::{self, Receiver, Sender},
    },
    task::JoinHandle,
};

const MSG_BUFF_SIZE: usize = 1600;

pub struct Connection {
    active: Arc<Mutex<HashMap<SocketAddr, Handler>>>,
    acceptor_handle: JoinHandle<()>,
    messages_tx: Arc<Sender<AppResult<Message>>>, // used to pass messages to the receiver side
    messages_rx: Receiver<AppResult<Message>>,
    max_conns: usize,
    // The following fields are only used to simulate connection failures in tests.
    address: SocketAddr,
    connected: bool,
}

// TODO: both `send` and `recv` should return a Result that can contain a
// `ConnectionClosed`-style error so callers (e.g., Node) can handle peer
// disconnections more easily, for example:
//
// match Err(ConnectionClosed { addr }) => if addr == leader_addr { ... }
impl Connection {
    pub async fn start(address: SocketAddr, max_conns: usize) -> AppResult<Self> {
        let active = Arc::new(Mutex::new(HashMap::new()));
        let (messages_tx, messages_rx) = mpsc::channel(MSG_BUFF_SIZE);
        let messages_tx = Arc::new(messages_tx);
        let acceptor_handle =
            Acceptor::start(address, active.clone(), messages_tx.clone(), max_conns).await?;

        Ok(Self {
            active,
            acceptor_handle,
            messages_tx,
            messages_rx,
            max_conns,
            address,
            connected: true,
        })
    }

    pub async fn send(&mut self, msg: Message, address: &SocketAddr) -> AppResult<()> {
        if !self.connected {
            // simulate a dropped connection
            return Err(AppError::ConnectionLost);
        }

        let mut guard = self.active.lock().await;
        if !guard.contains_key(address) {
            let handler = Handler::start(self.address, *address, self.messages_tx.clone()).await?;
            add_handler_from(&mut guard, handler, self.max_conns);
        }

        match guard.get_mut(address).unwrap().send(msg).await {
            Ok(_) => Ok(()),
            Err(e) => {
                guard.remove(address);
                Err(e)
            }
        }
    }

    pub async fn recv(&mut self) -> AppResult<Message> {
        if !self.connected {
            // simulate a dropped connection
            return Err(AppError::ConnectionLost);
        }

        // NOTE:
        // If all handlers are down we return ConnectionLost, which is interpreted
        // as "we lost our local connectivity". In practice, callers should ensure
        // that at least one connection exists before calling `recv`, otherwise
        // this may return ConnectionLost unexpectedly during tests.
        self.messages_rx
            .recv()
            .await
            .ok_or(AppError::ConnectionLost)?
    }

    // The following two methods are only used to simulate failures in tests.
    pub async fn disconnect(&mut self) {
        self.connected = false;
        let mut guard = self.active.lock().await;
        for handle in guard.values_mut() {
            handle.stop();
        }

        self.acceptor_handle.abort();
    }

    pub async fn reconnect(&mut self) -> AppResult<()> {
        self.connected = true;
        self.acceptor_handle = Acceptor::start(
            self.address,
            self.active.clone(),
            self.messages_tx.clone(),
            self.max_conns,
        )
        .await?;

        Ok(())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.acceptor_handle.abort();
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
        task::{self},
        time::sleep,
    };

    #[tokio::test]
    async fn test00_successful_send_and_recv_between_connections() {
        let address1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12354);
        let address2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12355);
        let message = Message::Ack { op_id: 1 };
        let message_copy = message.clone();
        let mut connection2 = Connection::start(address2, 1).await.unwrap();
        let handle1 = task::spawn(async move {
            let mut connection1 = Connection::start(address1, 1).await.unwrap();
            connection1.send(message_copy, &address2).await.unwrap();
        });

        let received = connection2.recv().await.unwrap();
        assert_eq!(received, message);
        handle1.await.unwrap();
    }

    #[tokio::test]
    async fn test01_sending_messages_to_different_addresses_with_only_one_conn_max() {
        let address1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12356);
        let address2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12357);
        let address3 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12358);
        let message = Message::Ack { op_id: 1 };
        let message_copy = message.clone();
        let mut connection2 = Connection::start(address2, 1).await.unwrap();
        let mut connection3 = Connection::start(address3, 1).await.unwrap();
        let handle1 = task::spawn(async move {
            let mut connection1 = Connection::start(address1, 1).await.unwrap();
            connection1
                .send(message_copy.clone(), &address2)
                .await
                .unwrap();
            connection1.send(message_copy, &address3).await.unwrap();
        });

        let received2 = connection2.recv().await.unwrap();
        assert_eq!(received2, message);
        let received3 = connection3.recv().await.unwrap();
        assert_eq!(received3, message);
        handle1.await.unwrap();
    }

    #[tokio::test]
    async fn test02_receiving_messages_from_different_addresses_with_only_one_conn_max() {
        let address1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12359);
        let address2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12360);
        let address3 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12361);
        let message = Message::Ack { op_id: 1 };
        let message_copy = message.clone();
        let mut connection1 = Connection::start(address1, 1).await.unwrap();
        let handle = task::spawn(async move {
            let mut connection2 = Connection::start(address2, 1).await.unwrap();
            connection2
                .send(message_copy.clone(), &address1)
                .await
                .unwrap();
            let mut connection3 = Connection::start(address3, 1).await.unwrap();
            connection3.send(message_copy, &address1).await.unwrap();
        });

        let received2 = connection1.recv().await.unwrap();
        assert_eq!(received2, message);
        let received3 = connection1.recv().await.unwrap();
        assert_eq!(received3, message);
        handle.await.unwrap();
    }

    // In the next two tests we verify behavior when a peer connection is dropped.
    #[tokio::test]
    async fn test03_send_results_in_connection_lost_with_if_peer_is_down() {
        let address1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12368);
        let address2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12369);
        let connection1 = Connection::start(address1, 1).await.unwrap();
        let mut connection2 = Connection::start(address2, 1).await.unwrap();
        let result1 = connection2.send(Message::Ack { op_id: 0 }, &address1).await;
        assert_eq!(result1, Ok(()));
        drop(connection1);
        sleep(Duration::from_secs(3)).await;
        let result2 = connection2.send(Message::Ack { op_id: 0 }, &address1).await;
        assert_eq!(
            result2,
            Err(AppError::ConnectionLostWith { address: address1 })
        );
    }

}

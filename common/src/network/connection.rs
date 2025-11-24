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
    messages_tx: Arc<Sender<Message>>, // sólo para pasarle a quienes me mandan (Receiver)
    messages_rx: Receiver<Message>,
    max_conns: usize,
}

// TODO: tanto send como recv tienen que retornar un Result que pueda contener el error
// ConnectionClosed, para que cuando Node haga send/recv pueda hanldear fácilmente si
// se cayó, por ej, el líder (match Err(ConnectionClosed {addr}) => if addr == leader_addr ...)
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
        })
    }

    pub async fn send(&mut self, msg: Message, address: &SocketAddr) -> AppResult<()> {
        let mut guard = self.active.lock().await;
        if !guard.contains_key(address) {
            let handler = Handler::start(address, self.messages_tx.clone()).await?;
            add_handler_from(&mut guard, handler, self.max_conns);
        }

        guard.get_mut(address).unwrap().send(msg).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> AppResult<Message> {
        self.messages_rx.recv().await.ok_or(AppError::ChannelClosed)
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
        io::AsyncReadExt,
        net::TcpStream,
        task::{self, yield_now},
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

    // en los próximos dos te das cuenta si **a alguien se le cayó la conexión**
    #[tokio::test]
    async fn test03_send_results_in_connection_lost_with_if_peer_is_down() {
        let address1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12368);
        let address2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12369);
        let connection1 = Connection::start(address1, 1).await.unwrap();
        let mut connection2 = Connection::start(address2, 1).await.unwrap();
        let result1 = connection2.send(Message::Ack { op_id: 0 }, &address1).await;
        assert_eq!(result1, Ok(()));
        drop(connection1);
        let result2 = connection2.send(Message::Ack { op_id: 0 }, &address1).await;
        assert_eq!(
            result2,
            Err(AppError::ConnectionLostWith { address: address1 })
        );
    }

    #[tokio::test]
    async fn test04_recv_results_in_connection_lost_with_with_if_peer_is_down() {
        let address1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12368);
        let address2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12369);
        let mut connection1 = Connection::start(address1, 1).await.unwrap();
        let mut connection2 = Connection::start(address2, 1).await.unwrap();
        let msg = Message::Ack { op_id: 0 };
        connection1.send(msg.clone(), &address2).await.unwrap();
        let result1 = connection2.recv().await;
        assert_eq!(result1, Ok(msg));
        drop(connection1);
        let result2 = connection2.recv().await;
        assert_eq!(
            result2,
            Err(AppError::ConnectionLostWith { address: address1 })
        );
    }

    // cómo sabés que se te cayó a vos la conexión? ...
    // #[tokio::test]
    async fn test05_send_and_recv_result_in_connection_lost_if_all_peers_are_down() {
        // TODO
    }
}

use super::{acceptor::Acceptor, active_helpers::add_handler_from, handler::Handler};
use crate::{
    errors::{AppError, AppResult},
    node::message::Message,
};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex,
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
    outgoing_tx: Arc<Sender<(Message, SocketAddr)>>,
}

// TODO: tanto send como recv tienen que retornar un Result que pueda contener el error
// ConnectionClosed, para que cuando Node haga send/recv pueda hanldear fácilmente si
// se cayó, por ej, el líder (match Err(ConnectionClosed {addr}) => if addr == leader_addr ...)
impl Connection {
    pub async fn start(address: SocketAddr, max_conns: usize) -> AppResult<Self> {
        let active = Arc::new(Mutex::new(HashMap::new()));
        let (messages_tx, messages_rx) = mpsc::channel(MSG_BUFF_SIZE);
        let messages_tx = Arc::new(messages_tx);
        // outgoing channel for requests to send messages; processed by a background task
        let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<(Message, SocketAddr)>(MSG_BUFF_SIZE);
        let outgoing_tx = Arc::new(outgoing_tx);
        let acceptor_handle =
            Acceptor::start(address, active.clone(), messages_tx.clone(), max_conns).await?;
        // Spawn background worker that performs actual sends requested through outgoing_rx.
        let active_clone = active.clone();
        let max_conns_clone = max_conns;
        let messages_tx_worker = messages_tx.clone();
        tokio::spawn(async move {
            while let Some((msg, addr)) = outgoing_rx.recv().await {
                // for each outgoing request, ensure handler exists and send
                let mut guard = active_clone.lock().await;
                if !guard.contains_key(&addr) {
                    if let Ok(handler) = Handler::start(&addr, messages_tx_worker.clone()).await {
                        add_handler_from(&mut guard, handler, max_conns_clone);
                    } else {
                        // failed to start handler, skip this send
                        continue;
                    }
                }

                if let Some(h) = guard.get_mut(&addr) {
                    let _ = h.send(msg).await;
                }
            }
        });

        Ok(Self {
            active,
            acceptor_handle,
            messages_tx,
            messages_rx,
            max_conns,
            outgoing_tx,
        })
    }

    pub async fn send(&mut self, msg: Message, address: &SocketAddr) -> AppResult<()> {
        // preserve original immediate-send behaviour for callers that have &mut Connection
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

    /// Returns a cloneable sender that can be used by background tasks to request sends
    /// without needing &mut Connection. Each request is processed by the connection's
    /// internal sender task which performs the real network send.
    pub fn outgoing_sender(&self) -> Sender<(Message, SocketAddr)> {
        (*self.outgoing_tx).clone()
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
    use std::net::{IpAddr, Ipv4Addr};
    use tokio::task;

    #[tokio::test]
    async fn test_successful_send_and_recv_between_connections() {
        let address1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12354);
        let address2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12355);
        let message = Message::Ack { id: 1 };
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
}

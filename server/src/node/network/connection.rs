use super::{acceptor::Acceptor, active_helpers::add_handler_from, handler::Handler};
use crate::{
    errors::{AppError, AppResult},
    node::node_message::NodeMessage,
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
    messages_tx: Arc<Sender<NodeMessage>>, // sólo para pasarle a quienes me mandan (Receiver)
    messages_rx: Receiver<NodeMessage>,
    max_conns: usize,
}

// TODO: tanto send como recv tienen que retornar un Result que pueda contener el error
// ConnectionClosed, para que cuando Node haga send/recv pueda hanldear fácilmente si
// se cayó, por ej, el líder (match Err(ConnectionClosed {addr}) => if addr == leader_addr ...)
impl Connection {
    async fn start(address: SocketAddr, max_conns: usize) -> AppResult<Self> {
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

    pub async fn send(&mut self, msg: NodeMessage, address: SocketAddr) -> AppResult<()> {
        if !self.active.lock().await.contains_key(&address) {
            self.add_handler(address).await?;
        }

        self.active
            .lock()
            .await
            .get_mut(&address)
            .unwrap()
            .send(msg)
            .await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> AppResult<NodeMessage> {
        self.messages_rx.recv().await.ok_or(AppError::ChannelClosed)
    }

    async fn add_handler_from(&mut self, handler: Handler) {
        if self.active.lock().await.len() >= self.max_conns {
            self.remove_last_recently_used();
        }

        self.active.lock().await.insert(handler.address, handler);
    }

    async fn add_handler(&mut self, address: SocketAddr) -> AppResult<()> {
        if self.active.lock().await.len() >= self.max_conns {
            self.remove_last_recently_used();
        }

        add_handler_from(
            self.active.lock().await,
            Handler::start(address, self.messages_tx.clone()).await?,
            self.max_conns,
        );
        Ok(())
    }

    async fn remove_last_recently_used(&mut self) -> Option<Handler> {
        let mut active = self.active.lock().await;
        if let Some((&address, _)) = active.iter().min_by_key(|(_, handler)| handler.last_used) {
            return active.remove(&address);
        }

        None
    }
}

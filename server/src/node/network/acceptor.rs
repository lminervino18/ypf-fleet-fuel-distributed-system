use super::active_helpers::add_handler_from;
use super::handler::Handler;
use crate::errors::{AppError, AppResult};
use crate::node::node_message::NodeMessage;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;
use tokio::task::{self, JoinHandle};

const INCOMING_BUFF_SIZE: usize = 300;

pub struct Acceptor {
    listener: TcpListener,
    active: Arc<Mutex<HashMap<SocketAddr, Handler>>>,
    max_conns: usize,
}

impl Acceptor {
    pub async fn start(
        address: SocketAddr,
        active: Arc<Mutex<HashMap<SocketAddr, Handler>>>,
        messages_tx: Arc<Sender<NodeMessage>>,
        max_conns: usize,
    ) -> AppResult<JoinHandle<()>> {
        let mut acceptor = Acceptor::new(address, active, max_conns).await?;
        let handle = task::spawn(async move {
            acceptor.run(messages_tx).await;
        });

        Ok(handle)
    }

    async fn new(
        address: SocketAddr,
        active: Arc<Mutex<HashMap<SocketAddr, Handler>>>,
        max_conns: usize,
    ) -> AppResult<Self> {
        Ok(Self {
            listener: TcpListener::bind(address).await.map_err(|_| {
                AppError::ConnectionRefused {
                    addr: address.to_string(),
                }
            })?,
            active,
            max_conns,
        })
    }

    async fn run(&mut self, messages_tx: Arc<Sender<NodeMessage>>) {
        while let Ok((stream, _)) = self.listener.accept().await {
            let Ok(handler) = Handler::start_from(stream, messages_tx.clone()).await else {
                continue;
            };

            let guard = self.active.lock().await;
            add_handler_from(guard, handler, self.max_conns);
        }
    }
}

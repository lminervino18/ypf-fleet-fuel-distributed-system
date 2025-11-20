use super::handler::Handler;
use crate::{errors::AppResult, node::node_message::NodeMessage};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;

pub struct Connection {
    active: Arc<Mutex<HashMap<SocketAddr, Handler>>>,
    max_conns: usize,
}

impl Connection {
    async fn send(&mut self, msg: NodeMessage, address: SocketAddr) -> AppResult<()> {
        todo!();
    }

    async fn recv(&mut self) -> AppResult<()> {
        todo!();
    }
}

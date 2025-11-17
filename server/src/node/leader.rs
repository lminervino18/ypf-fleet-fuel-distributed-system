use super::{
    connection_manager::{ConnectionManager, InboundEvent, ManagerCmd},
    node::Node,
    operation::Operation,
};
use crate::{
    actors::ActorRouter,
    errors::{AppError, AppResult},
};
use actix::{Actor, Addr};
use std::{collections::HashMap, future::pending, net::SocketAddr};
use tokio::sync::{mpsc, oneshot};

pub struct Leader {
    id: u64,
    coords: (f64, f64),
    max_conns: usize,
    replicas: Vec<SocketAddr>,
    operations: HashMap<u8, Operation>,
    connection_tx: mpsc::Sender<ManagerCmd>,
    connection_rx: mpsc::Receiver<InboundEvent>,
    router: Addr<ActorRouter>,
}

impl Node for Leader {
    async fn handle_accept(&mut self, op: Operation) {
        todo!()
    }

    async fn handle_learn(&mut self, op: Operation) {
        todo!()
    }

    async fn handle_commit(&mut self, op: Operation) {
        todo!()
    }

    async fn handle_finished(&mut self, op: Operation) {
        todo!()
    }

    async fn recv_node_msg(&mut self) -> Option<InboundEvent> {
        todo!()
    }
}

impl Leader {
    pub async fn start(
        address: SocketAddr,
        coords: (f64, f64),
        replicas: Vec<SocketAddr>,
        max_conns: usize,
    ) -> AppResult<()> {
        let (connection_tx, connection_rx) = ConnectionManager::start(address, max_conns);
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();
        std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new().start();
                if router_tx.send(router.clone()).is_err() {
                    eprintln!("[ERROR] Failed to send router address to Node");
                }

                pending::<()>().await;
            });
        });

        let mut leader = Self {
            id: rand::random::<u64>(),
            coords,
            max_conns,
            replicas,
            operations: HashMap::new(),
            connection_tx,
            connection_rx,
            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive router address: {e}"),
            })?,
        };

        leader.run().await
    }
}

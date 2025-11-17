use super::{
    connection_manager::{ConnectionManager, InboundEvent, ManagerCmd},
    node::Node,
    node_message::NodeMessage,
    operation::Operation,
};
use crate::{
    actors::{ActorEvent, ActorRouter, RouterCmd},
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
    operations: HashMap<u32, (usize, Operation)>,
    connection_tx: mpsc::Sender<ManagerCmd>,
    connection_rx: mpsc::Receiver<InboundEvent>,
    actor_rx: mpsc::Receiver<ActorEvent>,
    router: Addr<ActorRouter>,
}

impl Node for Leader {
    async fn handle_request(&mut self, op: Operation) {
        self.operations.insert(op.id, (0, op.clone()));
        let log_msg: Vec<u8> = NodeMessage::Log { op: op.clone() }.into();
        for replica in &self.replicas {
            self.connection_tx
                .send(ManagerCmd::SendTo(*replica, log_msg.clone()))
                .await
                .unwrap();
        }
    }

    async fn handle_log(&mut self, op: Operation) {
        todo!();
    }

    async fn handle_ack(&mut self, id: u32) {
        self.operations.entry(id).and_modify(|(ack_count, _)| {
            *ack_count += 1;
            if *ack_count <= self.replicas.len() / 2 {
                return;
            }
        });

        let op = self.operations.remove(&id);
        // self.router.send() // tell actix to manage the businness logic
    }

    async fn recv_node_msg(&mut self) -> Option<InboundEvent> {
        self.connection_rx.recv().await
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
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();
        std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(actor_tx).start();
                if router_tx.send(router.clone()).is_err() {
                    eprintln!("[ERROR] Failed to deliver ActorRouter addr to Leader");
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
            actor_rx,
            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive ActorRouter address: {e}"),
            })?,
        };

        leader.run().await
    }
}

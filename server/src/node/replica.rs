use super::{
    connection_manager::{ConnectionManager, InboundEvent, ManagerCmd},
    node::Node,
    node_message::NodeMessage,
    operation::Operation,
};
use crate::actors::types::ActorEvent;
use crate::{
    actors::ActorRouter,
    errors::{AppError, AppResult},
};
use actix::{Actor, Addr};
use std::{collections::HashMap, future::pending, net::SocketAddr};
use tokio::sync::{mpsc, oneshot};

pub struct Replica {
    id: u64,
    coords: (f64, f64),
    max_conns: usize,
    leader_addr: SocketAddr,
    other_replicas: Vec<SocketAddr>,
    operations: HashMap<u32, Operation>,
    connection_tx: mpsc::Sender<ManagerCmd>,
    connection_rx: mpsc::Receiver<InboundEvent>,
    actor_rx: mpsc::Receiver<ActorEvent>,
    router: Addr<ActorRouter>,
}

impl Node for Replica {
    async fn handle_request(&mut self, op: Operation, client_addr: SocketAddr) {
        // redirect to leader node
        self.connection_tx.send(ManagerCmd::SendTo(
            self.leader_addr,
            NodeMessage::Request {
                op,
                addr: client_addr,
            }
            .into(),
        ));
    }

    async fn handle_log(&mut self, op: Operation) {
        let op_id = op.id;
        self.operations.insert(op_id, op);
        self.connection_tx.send(ManagerCmd::SendTo(
            self.leader_addr,
            NodeMessage::Ack { id: op_id }.into(),
        ));
    }

    async fn handle_ack(&mut self, id: u32) {
        todo!(); // TODO: replicas should not receive any ACK msgs
    }

    async fn recv_node_msg(&mut self) -> Option<InboundEvent> {
        self.connection_rx.recv().await
    }
}

impl Replica {
    pub async fn start(
        address: SocketAddr,
        coords: (f64, f64),
        leader_addr: SocketAddr,
        other_replicas: Vec<SocketAddr>,
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
                    eprintln!("[ERROR] Failed to send router address to Replica");
                }

                pending::<()>().await;
            });
        });

        let mut replica = Self {
            id: rand::random::<u64>(),
            coords,
            max_conns,
            leader_addr,
            other_replicas,
            operations: HashMap::new(),
            connection_tx,
            connection_rx,
            actor_rx,
            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive router address: {e}"),
            })?,
        };

        replica.run().await
    }
}

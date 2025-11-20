use super::{
    connection_manager::{ConnectionManager, InboundEvent, ManagerCmd},
    message::Message,
    node::Node,
    operation::Operation,
};
use crate::actors::{types::ActorEvent, RouterCmd};
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
        self.connection_tx
            .send(ManagerCmd::SendTo(
                self.leader_addr,
                Message::Request {
                    op,
                    addr: client_addr,
                }
                .into(),
            ))
            .await
            .unwrap();
    }

    async fn handle_log(&mut self, new_op: Operation) {
        let new_op_id = new_op.id;
        self.operations.insert(new_op_id, new_op);
        self.commit_operation(new_op_id - 1).await;
        self.connection_tx
            .send(ManagerCmd::SendTo(
                self.leader_addr,
                Message::Ack { id: new_op_id }.into(),
            ))
            .await
            .unwrap();
    }

    async fn handle_ack(&mut self, _id: u32) {
        todo!(); // TODO: replicas should not receive any ACK msgs
    }

    async fn recv_node_msg(&mut self) -> Option<InboundEvent> {
        self.connection_rx.recv().await
    }
}

impl Replica {
    async fn commit_operation(&mut self, id: u32) {
        let Some(op) = self.operations.remove(&id) else {
            return;
        };

        if self
            .router
            .send(RouterCmd::ApplyCharge {
                account_id: (op.account_id),
                card_id: (op.card_id),
                amount: (op.amount as f64),
                op_id: (op.id as u64),
                request_id: (0), // ?
                timestamp: 0,
            })
            .await
            .is_err()
        {
            todo!() // TODO
        };
    }

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

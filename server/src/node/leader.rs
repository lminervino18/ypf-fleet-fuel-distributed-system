use super::{message::Message, network::Connection, node::Node, operation::Operation};
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
    operations: HashMap<u32, (usize, SocketAddr, Operation)>,
    connection: Connection,
    actor_rx: mpsc::Receiver<ActorEvent>,
    router: Addr<ActorRouter>,
}

impl Node for Leader {
    async fn handle_request(&mut self, op: Operation, client_addr: SocketAddr) {
        self.operations.insert(op.id, (0, client_addr, op.clone()));
        let msg = Message::Log { op: op.clone() };
        for replica in &self.replicas {
            self.connection.send(msg.clone(), replica);
        }
    }

    async fn handle_log(&mut self, op: Operation) {
        todo!(); // TODO: leader should not receive any Log msgs
    }

    async fn handle_ack(&mut self, id: u32) {
        let Some((ack_count, _, _)) = self.operations.get_mut(&id) else {
            todo!() // TODO: handle this case
        };

        *ack_count += 1;
        if *ack_count <= self.replicas.len() / 2 {
            return;
        }

        // self.commit_operation(id).await; // TODO: this logic should be in the actors mod
        todo!();
    }

    async fn recv_node_msg(&mut self) -> AppResult<Message> {
        self.connection.recv().await
    }
}

impl Leader {
    pub async fn start(
        address: SocketAddr,
        coords: (f64, f64),
        replicas: Vec<SocketAddr>,
        max_conns: usize,
    ) -> AppResult<()> {
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();
        std::thread::spawn(move || {
            // spawns a thread for Actix that runs inside a Tokio runtime 
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(actor_tx).start();
                if router_tx.send(router.clone()).is_err() {
                    eprintln!("[ERROR] Failed to deliver ActorRouter addr to Leader");
                }

                pending::<()>().await;      // Keep the actix system running indefinitely
            });
        });

        let mut leader = Self {
            id: rand::random::<u64>(),
            coords,
            max_conns,
            replicas,
            operations: HashMap::new(),
            connection: Connection::start(address, max_conns).await?,
            actor_rx,
            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive ActorRouter address: {e}"),
            })?,
        };

        leader.run().await
    }
}

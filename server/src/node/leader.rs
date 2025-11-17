use super::{
    connection_manager::{ConnectionManager, InboundEvent, ManagerCmd},
    node::Node,
    operation::Operation,
    node_message::NodeMessage,
};
use crate::actors::types::ActorEvent;
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

    // Network
    connection_tx: mpsc::Sender<ManagerCmd>,
    connection_rx: mpsc::Receiver<InboundEvent>,

    // ActorRouter → Leader
    actor_rx: mpsc::Receiver<ActorEvent>,

    router: Addr<ActorRouter>,
}

impl Node for Leader {
    // ==========================
    // Consensus handlers
    // ==========================
    async fn handle_accept(&mut self, op: Operation) {
        println!("[Leader] handle_accept({:?})", op);
        // TODO
    }

    async fn handle_learn(&mut self, op: Operation) {
        println!("[Leader] handle_learn({:?})", op);
        // TODO
    }

    async fn handle_commit(&mut self, op: Operation) {
        println!("[Leader] handle_commit({:?})", op);
        // TODO
    }

    async fn handle_finished(&mut self, op: Operation) {
        println!("[Leader] handle_finished({:?})", op);
        // TODO
    }

    // ==========================
    // Network event receiver
    // ==========================
    async fn recv_node_msg(&mut self) -> Option<InboundEvent> {
        self.connection_rx.recv().await
    }

    // ==========================
    // Actor event receiver
    // ==========================
    async fn recv_actor_event(&mut self) -> Option<ActorEvent> {
        self.actor_rx.recv().await
    }

    // ==========================
    // Actor event handler
    // ==========================
    async fn handle_actor_event(&mut self, event: ActorEvent) {
        match event {
            ActorEvent::OperationReady { operation } => {
                println!("[Leader] OperationReady from ActorRouter: {:?}", operation);
                // TODO: Charge → accept, CheckLimit → respond locally
            }
            ActorEvent::CardUpdated { card_id, delta } => {
                println!("[Leader] CardUpdated card={} delta={}", card_id, delta);
            }
            ActorEvent::Debug(msg) => {
                println!("[Leader][actor] {msg}");
            }
        }
    }

    // ==========================
    // MAIN LOOP IMPLEMENTATION
    // ==========================
    async fn run_loop(&mut self) -> AppResult<()> {
    let mut net_open = true;
    let mut actors_open = true;

    while net_open || actors_open {
        let net_fut = if net_open {
            Some(self.connection_rx.recv())
        } else {
            None
        };

        let actor_fut = if actors_open {
            Some(self.actor_rx.recv())
        } else {
            None
        };

        tokio::select! {
            // ======================
            // Network event
            // ======================
            maybe_evt = async {
                match net_fut {
                    Some(fut) => fut.await,
                    None => None
                }
            }, if net_open => {
                match maybe_evt {
                    Some(evt) => {
                        match evt {
                            InboundEvent::Received { peer: _, payload } => {
                                match NodeMessage::try_from(payload) {
                                    Ok(msg) => self.handle_node_msg(msg).await,
                                    Err(e) => eprintln!("[WARN][Leader] invalid msg: {:?}", e),
                                }
                            }
                            InboundEvent::ConnClosed { peer } => {
                                println!("[Leader] Connection closed by {}", peer);
                            }
                        }
                    }
                    None => {
                        println!("[Leader] Network channel closed");
                        net_open = false;
                    }
                }
            }

            // ======================
            // Actor event
            // ======================
            maybe_event = async {
                match actor_fut {
                    Some(fut) => fut.await,
                    None => None
                }
            }, if actors_open => {
                match maybe_event {
                    Some(evt) => self.handle_actor_event(evt).await,
                    None => {
                        println!("[Leader] Actor event channel closed");
                        actors_open = false;
                    }
                }
            }
        }
    }

    Ok(())
}

}

impl Leader {
    pub async fn start(
        address: SocketAddr,
        coords: (f64, f64),
        replicas: Vec<SocketAddr>,
        max_conns: usize,
    ) -> AppResult<()> {

        // Start the ConnectionManager
        let (connection_tx, connection_rx) = ConnectionManager::start(address, max_conns);

        // Create ActorRouter → Leader channel
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);

        // oneshot to retrieve router address
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();

        // Spawn Actix system
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

        // Construct Leader
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

        // Start event loop
        leader.run().await
    }
}

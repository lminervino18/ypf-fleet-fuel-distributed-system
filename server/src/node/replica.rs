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
    operations: HashMap<u8, Operation>,

    // Network
    connection_tx: mpsc::Sender<ManagerCmd>,
    connection_rx: mpsc::Receiver<InboundEvent>,

    // ActorRouter → Replica
    actor_rx: mpsc::Receiver<ActorEvent>,

    router: Addr<ActorRouter>,
}

// ===============================================================
// Node trait implementation
// ===============================================================
impl Node for Replica {
    async fn handle_accept(&mut self, op: Operation) {
        println!("[Replica] handle_accept({:?})", op);
        // TODO
    }

    async fn handle_learn(&mut self, op: Operation) {
        println!("[Replica] handle_learn({:?})", op);
        // TODO
    }

    async fn handle_commit(&mut self, op: Operation) {
        println!("[Replica] handle_commit({:?})", op);
        // TODO
    }

    async fn handle_finished(&mut self, op: Operation) {
        println!("[Replica] handle_finished({:?})", op);
        // TODO
    }

    async fn recv_node_msg(&mut self) -> Option<InboundEvent> {
        self.connection_rx.recv().await
    }

    async fn recv_actor_event(&mut self) -> Option<ActorEvent> {
        self.actor_rx.recv().await
    }

    async fn handle_actor_event(&mut self, event: ActorEvent) {
        match event {
            ActorEvent::LimitCheckResult {
                request_id,
                allowed,
                error,
            } => match error {
                None => {
                    println!(
                        "[Replica][actor] LimitCheckResult: request_id={} allowed={}",
                        request_id, allowed
                    );
                }
                Some(err) => {
                    println!(
                        "[Replica][actor] LimitCheckResult: request_id={} allowed={} error={:?}",
                        request_id, allowed, err
                    );
                }
            },

            ActorEvent::ChargeApplied { operation } => {
                println!("[Replica][actor] ChargeApplied: operation={:?}", operation);
            }

            ActorEvent::LimitUpdated {
                scope,
                account_id,
                card_id,
                new_limit,
            } => {
                println!(
                    "[Replica][actor] LimitUpdated: scope={:?} account_id={} card_id={:?} new_limit={:?}",
                    scope, account_id, card_id, new_limit
                );
            }

            ActorEvent::LimitUpdateFailed {
                scope,
                account_id,
                card_id,
                request_id,
                error,
            } => {
                println!(
                    "[Replica][actor] LimitUpdateFailed: scope={:?} account_id={} card_id={:?} request_id={} error={:?}",
                    scope, account_id, card_id, request_id, error
                );
            }

            ActorEvent::Debug(msg) => {
                println!("[Replica][actor][debug] {}", msg);
            }
        }
    }

    // ===============================================================
    // Main event loop: same pattern as Leader
    // ===============================================================
    async fn run_loop(&mut self) -> AppResult<()> {
        let mut net_open = true;
        let mut actors_open = true;

        while net_open || actors_open {
            // Prepare futures WITHOUT borrowing self twice
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
                // ---------------- NETWORK EVENTS ----------------
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
                                        Err(e) =>
                                            eprintln!("[WARN][Replica] invalid NodeMessage: {:?}", e),
                                    }
                                }
                                InboundEvent::ConnClosed { peer } => {
                                    println!("[Replica] Connection closed by {}", peer);
                                }
                            }
                        }
                        None => {
                            println!("[Replica] Network channel closed");
                            net_open = false;
                        }
                    }
                }

                // ---------------- ACTOR EVENTS ----------------
                maybe_actor = async {
                    match actor_fut {
                        Some(fut) => fut.await,
                        None => None
                    }
                }, if actors_open => {
                    match maybe_actor {
                        Some(evt) => self.handle_actor_event(evt).await,
                        None => {
                            println!("[Replica] Actor event channel closed");
                            actors_open = false;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

// ===============================================================
// Start function
// ===============================================================

impl Replica {
    pub async fn start(
        address: SocketAddr,
        coords: (f64, f64),
        leader_addr: SocketAddr,
        other_replicas: Vec<SocketAddr>,
        max_conns: usize,
    ) -> AppResult<()> {
        // Start network subsystem
        let (connection_tx, connection_rx) = ConnectionManager::start(address, max_conns);

        // ActorRouter → Replica channel
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);

        // Retrieve router Addr from actix system
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();

        // Launch Actix thread
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

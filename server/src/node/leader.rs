use super::{
    connection_manager::{ConnectionManager, InboundEvent, ManagerCmd},
    node::Node,
    node_message::NodeMessage,
    operation::Operation,
};
use crate::actors::types::{ActorEvent, RouterCmd};
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
            ActorEvent::LimitCheckResult {
                request_id,
                allowed,
                error,
            } => match error {
                None => {
                    println!(
                        "[Leader][actor] LimitCheckResult: request_id={} allowed={}",
                        request_id, allowed
                    );
                }
                Some(err) => {
                    println!(
                        "[Leader][actor] LimitCheckResult: request_id={} allowed={} error={:?}",
                        request_id, allowed, err
                    );
                }
            },

            ActorEvent::ChargeApplied { operation } => {
                println!("[Leader][actor] ChargeApplied: operation={:?}", operation);
            }

            ActorEvent::LimitUpdated {
                scope,
                account_id,
                card_id,
                new_limit,
            } => {
                println!(
                    "[Leader][actor] LimitUpdated: scope={:?} account_id={} card_id={:?} new_limit={:?}",
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
                    "[Leader][actor] LimitUpdateFailed: scope={:?} account_id={} card_id={:?} request_id={} error={:?}",
                    scope, account_id, card_id, request_id, error
                );
            }

            ActorEvent::Debug(msg) => {
                println!("[Leader][actor][debug] {}", msg);
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
    /// Send some test commands to the ActorRouter on startup.
    ///
    /// This is just a synthetic workload to exercise:
    /// - account/card creation,
    /// - limit updates,
    /// - limit checks that pass/fail,
    /// - charge application.
    async fn send_smoke_test_commands(&self) {
        // Fixed ids for testing
        let account1 = 1;
        let account2 = 2;

        let card_a1_1 = 10;
        let card_a1_2 = 11;
        let card_a2_1 = 20;

        // 1) Set an account-wide limit for account1
        self.router.do_send(RouterCmd::UpdateAccountLimit {
            account_id: account1,
            new_limit: Some(1_000.0),
            request_id: 1,
        });

        // 2) Set a card limit for card_a1_1 (account1)
        self.router.do_send(RouterCmd::UpdateCardLimit {
            account_id: account1,
            card_id: card_a1_1,
            new_limit: Some(300.0),
            request_id: 2,
        });

        // 3) Authorization that SHOULD PASS (100 <= 300 and within account limit)
        self.router.do_send(RouterCmd::AuthorizeCharge {
            account_id: account1,
            card_id: card_a1_1,
            amount: 100.0,
            request_id: 3,
        });

        // 4) Authorization that SHOULD FAIL on CARD LIMIT (400 > 300)
        self.router.do_send(RouterCmd::AuthorizeCharge {
            account_id: account1,
            card_id: card_a1_1,
            amount: 400.0,
            request_id: 4,
        });

        // 5) Apply a charge directly (no limit check here, just state mutation)
        self.router.do_send(RouterCmd::ApplyCharge {
            account_id: account1,
            card_id: card_a1_1,
            amount: 50.0,
            timestamp: 0,   // you can plug real timestamps later
            op_id: 42,
            request_id: 5,
        });

        // 6) Try to set an account limit BELOW already consumed (should fail later)
        self.router.do_send(RouterCmd::UpdateAccountLimit {
            account_id: account1,
            new_limit: Some(10.0),
            request_id: 6,
        });

        // 7) Exercise a second account + card (no limits → always allowed)
        self.router.do_send(RouterCmd::AuthorizeCharge {
            account_id: account2,
            card_id: card_a2_1,
            amount: 500.0,
            request_id: 7,
        });

        // 8) Another card on account1, no card limit explicitly set (only account limit applies)
        self.router.do_send(RouterCmd::AuthorizeCharge {
            account_id: account1,
            card_id: card_a1_2,
            amount: 200.0,
            request_id: 8,
        });

        // 9) List accounts to see what got created
        self.router.do_send(RouterCmd::ListAccounts);
    }

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

        // === Synthetic smoke-test traffic to the ActorRouter ===
        leader.send_smoke_test_commands().await;

        // Start event loop
        leader.run().await
    }
}

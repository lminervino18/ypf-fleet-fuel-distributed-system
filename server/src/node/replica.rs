use super::{
    connection_manager::{ConnectionManager, InboundEvent, ManagerCmd},
    node::Node,
    node_message::NodeMessage,
    // Consensus-level operation used by the distributed protocol.
    operation::Operation as NodeOperation,
};

use crate::{
    actors::actor_router::ActorRouter,
    actors::messages::{ActorEvent, RouterCmd},
    domain::Operation as ActorOperation,
    errors::{AppError, AppResult, VerifyError, ApplyError},
    node::station::{NodeToStationMsg, StationToNodeMsg},
};


use actix::{Actor, Addr};
use std::{collections::HashMap, future::pending, net::SocketAddr};
use tokio::sync::{mpsc, oneshot};


/// Internal state for a charge coming from a station pump.
#[derive(Debug, Clone)]
struct StationPendingCharge {
    account_id: u64,
    card_id: u64,
    amount: f64,
}

/// Replica node.
///
/// Same wiring as the Leader, but intended to be a follower in the
/// distributed system. For station-originated charges it uses the
/// exact same Verify → Apply flow as the Leader.
pub struct Replica {
    id: u64,
    coords: (f64, f64),
    max_conns: usize,
    leader_addr: SocketAddr,
    other_replicas: Vec<SocketAddr>,

    /// Consensus operations (placeholder for now).
    operations: HashMap<u8, NodeOperation>,

    // ===== Network (ConnectionManager) =====
    connection_tx: mpsc::Sender<ManagerCmd>,
    connection_rx: mpsc::Receiver<InboundEvent>,

    // ===== ActorRouter → Replica =====
    actor_rx: mpsc::Receiver<ActorEvent>,

    // ===== Station ↔ Replica =====
    // Station → Replica (commands from pumps)
    station_cmd_rx: mpsc::Receiver<StationToNodeMsg>,
    // Replica → Station (final results)
    station_result_tx: mpsc::Sender<NodeToStationMsg>,

    /// In-flight charge requests coming from the station.
    /// Key = `op_id` / `request_id` chosen by the station simulator.
    station_requests: HashMap<u64, StationPendingCharge>,

    /// Local Actix ActorRouter handle.
    router: Addr<ActorRouter>,
}

// ===============================================================
// Node trait implementation
// ===============================================================
impl Node for Replica {
    async fn handle_accept(&mut self, op: NodeOperation) {
        //println!("[Replica] handle_accept({:?})", op);
        // TODO: integrate with real consensus.
    }

    async fn handle_learn(&mut self, op: NodeOperation) {
        //println!("[Replica] handle_learn({:?})", op);
        // TODO
    }

    async fn handle_commit(&mut self, op: NodeOperation) {
        //println!("[Replica] handle_commit({:?})", op);
        // TODO
    }

    async fn handle_finished(&mut self, op: NodeOperation) {
        //println!("[Replica] handle_finished({:?})", op);
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
            // --------------------------------------------------
            // Response to RouterCmd::Verify { op_id, operation }
            // --------------------------------------------------
            ActorEvent::VerifyResult {
                op_id,
                operation,
                allowed,
                error,
            } => {
                let pending = match self.station_requests.get(&op_id) {
                    Some(p) => p.clone(),
                    None => {
                        // Not from station: just log and return.
                        match &error {
                            None => {
                                // //println!(
                                //     "[Replica][actor] VerifyResult (non-station): op_id={} allowed={} op={:?}",
                                //     op_id, allowed, operation
                                // );
                            }
                            Some(err) => {
                                // //println!(
                                //     "[Replica][actor] VerifyResult (non-station): op_id={} allowed={} op={:?} error={:?}",
                                //     op_id, allowed, operation, err
                                // );
                            }
                        }
                        return;
                    }
                };

                if !matches!(operation, ActorOperation::Charge { .. }) {
                    // //println!(
                    //     "[Replica][actor] VerifyResult for non-Charge operation from station: op_id={} op={:?}",
                    //     op_id, operation
                    // );
                }

                if !allowed {
                    // //println!(
                    //     "[Replica][actor→station] op_id={} -> CHARGE DENIED (error={:?})",
                    //     op_id, error
                    // );

                    self.station_requests.remove(&op_id);

                    let msg = NodeToStationMsg::ChargeResult {
                        request_id: op_id,
                        allowed: false,
                        error,
                    };

                    if let Err(e) = self.station_result_tx.send(msg).await {
                        // //println!(
                        //     "[Replica][actor→station][ERROR] failed to send DENIED result: {}",
                        //     e
                        // );
                    }
                } else {
                    // //println!(
                    //     "[Replica][actor] VerifyResult: op_id={} allowed=true -> Apply(op)",
                    //     op_id
                    // );

                    let _ = pending;

                    self.router.do_send(RouterCmd::Apply {
                        op_id,
                        operation,
                    });
                }
            }

            // --------------------------------------------------
            // Response to RouterCmd::Apply { op_id, operation }
            // --------------------------------------------------
            ActorEvent::ApplyResult {
                op_id,
                operation,
                success,
                error,
            } => {
                // //println!(
                //     "[Replica][actor] ApplyResult: op_id={} success={} op={:?} error={:?}",
                //     op_id, success, operation, error
                // );

                if let Some(pending) = self.station_requests.remove(&op_id) {
                    if success {
                        // //println!(
                        //     "[Replica][actor→station] op_id={} -> CHARGE CONFIRMED (acc={}, card={}, amount={})",
                        //     op_id, pending.account_id, pending.card_id, pending.amount
                        // );

                        let msg = NodeToStationMsg::ChargeResult {
                            request_id: op_id,
                            allowed: true,
                            error: None,
                        };

                        if let Err(e) = self.station_result_tx.send(msg).await {
                            // //println!(
                            //     "[Replica][actor→station][ERROR] failed to send OK result: {}",
                            //     e
                            // );
                        }
                    } else {
                        // e//println!(
                        //     "[Replica][actor→station][ERROR] Apply failed for op_id={} (acc={}, card={}, amount={}) op={:?} error={:?}",
                        //     op_id, pending.account_id, pending.card_id, pending.amount, operation, error
                        // );

                        let mapped_error: Option<VerifyError> = match error {
                            Some(ApplyError::LimitUpdate(e)) => Some(VerifyError::LimitUpdate(e)),
                            None => None,
                        };

                        let msg = NodeToStationMsg::ChargeResult {
                            request_id: op_id,
                            allowed: false,
                            error: mapped_error,
                        };

                        if let Err(e) = self.station_result_tx.send(msg).await {
                            // //println!(
                            //     "[Replica][actor→station][ERROR] failed to send FAILED-APPLY result: {}",
                            //     e
                            // );
                        }
                    }
                } else {
                    // //println!(
                    //     "[Replica][actor] ApplyResult with op_id={} not tracked as station request (op={:?})",
                    //     op_id, operation
                    // );
                }
            }

            // --------------------------------------------------
            // Other events: debug, etc.
            // --------------------------------------------------
            ActorEvent::Debug(msg) => {
                //println!("[Replica][actor][debug] {}", msg);
            }
        }
    }

    // ===============================================================
    // Main event loop: same structure as Leader (net + actors + station)
    // ===============================================================
    async fn run_loop(&mut self) -> AppResult<()> {
        let mut net_open = true;
        let mut actors_open = true;
        let mut station_open = true;

        while net_open || actors_open || station_open {
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

            let station_fut = if station_open {
                Some(self.station_cmd_rx.recv())
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
                                        Err(e) => {}
                                            //println!("[WARN][Replica] invalid msg from network: {:?}", e),
                                    }
                                }
                                InboundEvent::ConnClosed { peer } => {
                                    //println!("[Replica] Connection closed by {}", peer);
                                }
                            }
                        }
                        None => {
                            //println!("[Replica] Network channel closed");
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
                            //println!("[Replica] Actor event channel closed");
                            actors_open = false;
                        }
                    }
                }

                // ---------------- STATION → REPLICA ----------------
                maybe_station = async {
                    match station_fut {
                        Some(fut) => fut.await,
                        None => None
                    }
                }, if station_open => {
                    match maybe_station {
                        Some(msg) => self.handle_station_msg(msg).await,
                        None => {
                            //println!("[Replica] Station command channel closed");
                            station_open = false;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

// ===============================================================
// Extra: handler for Station → Replica messages
// ===============================================================
impl Replica {
    /// Handle a message coming from the station (pump simulator).
    ///
    /// Currently we only support `ChargeRequest`, mapped to
    /// `Operation::Charge` with the same Verify → Apply flow as in Leader.
    async fn handle_station_msg(&mut self, msg: StationToNodeMsg) {
        match msg {
            StationToNodeMsg::ChargeRequest {
                pump_id,
                account_id,
                card_id,
                amount,
                request_id,
            } => {
                // //println!(
                //     "[Replica][station] pump={} -> ChargeRequest(account={}, card={}, amount={}, request_id={})",
                //     pump_id, account_id, card_id, amount, request_id
                // );

                self.station_requests.insert(
                    request_id,
                    StationPendingCharge {
                        account_id,
                        card_id,
                        amount,
                    },
                );

                let op = ActorOperation::Charge {
                    account_id,
                    card_id,
                    amount,
                };

                self.router.do_send(RouterCmd::Verify {
                    op_id: request_id,
                    operation: op,
                });
            }
        }
    }

    /// Start a Replica node:
    ///
    /// - boots the ConnectionManager (TCP),
    /// - boots the ActorRouter in a dedicated Actix system thread,
    /// - spawns the station simulator with `pumps` pumps on stdin,
    /// - then enters the main async event loop.
    pub async fn start(
        address: SocketAddr,
        coords: (f64, f64),
        leader_addr: SocketAddr,
        other_replicas: Vec<SocketAddr>,
        max_conns: usize,
        pumps: usize,
    ) -> AppResult<()> {
        // 1) Network subsystem
        let (connection_tx, connection_rx) = ConnectionManager::start(address, max_conns);

        // 2) ActorRouter → Replica channel
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);

        // 3) Station ↔ Replica channels
        let (station_cmd_tx, station_cmd_rx) = mpsc::channel::<StationToNodeMsg>(128);
        let (station_result_tx, station_result_rx) = mpsc::channel::<NodeToStationMsg>(128);

        // Spawn the station simulator.
        let station_cmd_tx_for_station = station_cmd_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::node::station::run_station_simulator(
                pumps,
                station_cmd_tx_for_station,
                station_result_rx,
            )
            .await
            {
                //println!("[Station][Replica] simulator error: {e:?}");
            }
        });

        // 4) Launch Actix + ActorRouter in another OS thread
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();

        std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(actor_tx).start();

                if router_tx.send(router.clone()).is_err() {
                    //println!("[ERROR] Failed to send router address to Replica");
                }

                pending::<()>().await;
            });
        });

        // 5) Build Replica
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

            station_cmd_rx,
            station_result_tx,
            station_requests: HashMap::new(),

            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive ActorRouter address: {e}"),
            })?,
        };

        // Enter main loop
        replica.run().await
    }
}

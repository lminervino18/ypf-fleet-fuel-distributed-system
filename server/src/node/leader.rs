use super::{
    connection_manager::{ConnectionManager, InboundEvent, ManagerCmd},
    node::Node,
    node_message::NodeMessage,
    operation::Operation as NodeOperation,
};

use crate::actors::{ActorEvent, RouterCmd};
use crate::domain::Operation as ActorOperation;

use crate::{
    actors::actor_router::ActorRouter,
    errors::{AppError, AppResult, VerifyError},
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

/// Leader node.
///
/// The Leader wires together:
/// - the TCP ConnectionManager (network),
/// - the local ActorRouter (Actix),
/// - the station simulator (stdin-driven pumps).
///
/// For a `Charge` coming from a pump the flow is now:
///
/// 1. Station → Leader: `StationToNodeMsg::ChargeRequest`.
/// 2. Leader builds `ActorOperation::Charge { .. }` and sends
///    `RouterCmd::Execute { op_id, operation }` to ActorRouter.
/// 3. ActorRouter/actors run verification + apply internally.
/// 4. Once finished, ActorRouter emits a single
///    `ActorEvent::OperationResult { op_id, operation, success, error }`.
/// 5. Leader translates that into a `NodeToStationMsg::ChargeResult`
///    and sends it back to the station, removing the pending request.
pub struct Leader {
    id: u64,
    coords: (f64, f64),
    max_conns: usize,
    replicas: Vec<SocketAddr>,

    /// Consensus operations (placeholder for future use).
    operations: HashMap<u8, NodeOperation>,

    // ===== Network (ConnectionManager) =====
    connection_tx: mpsc::Sender<ManagerCmd>,
    connection_rx: mpsc::Receiver<InboundEvent>,

    // ===== ActorRouter → Leader =====
    actor_rx: mpsc::Receiver<ActorEvent>,

    // ===== Station ↔ Leader =====
    // Station → Leader (commands from pumps)
    station_cmd_rx: mpsc::Receiver<StationToNodeMsg>,
    // Leader → Station (final results)
    station_result_tx: mpsc::Sender<NodeToStationMsg>,

    /// In-flight charge requests coming from the station.
    /// Key = `op_id` / `request_id` chosen by the station simulator.
    station_requests: HashMap<u64, StationPendingCharge>,

    /// Actix ActorRouter handle.
    router: Addr<ActorRouter>,
}

// ==========================================================
// Node trait implementation
// ==========================================================
impl Node for Leader {
    // ==========================
    // Consensus handlers
    // ==========================
    async fn handle_accept(&mut self, op: NodeOperation) {
        //println!("[Leader] handle_accept({:?})", op);
        // TODO: integrate with real consensus protocol.
    }

    async fn handle_learn(&mut self, op: NodeOperation) {
        //println!("[Leader] handle_learn({:?})", op);
        // TODO
    }

    async fn handle_commit(&mut self, op: NodeOperation) {
        //println!("[Leader] handle_commit({:?})", op);
        // TODO
    }

    async fn handle_finished(&mut self, op: NodeOperation) {
        //println!("[Leader] handle_finished({:?})", op);
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
            // --------------------------------------------------
            // Final result for RouterCmd::Execute { op_id, operation }
            // --------------------------------------------------
            ActorEvent::OperationResult {
                op_id,
                operation,
                success,
                error,
            } => {
                // Is this op_id tracked as a station-originated request?
                let pending = match self.station_requests.remove(&op_id) {
                    Some(p) => p,
                    None => {
                        // Not a pump request (maybe admin / consensus op).
                        // You can add logging here if desired.
                        // println!(
                        //     "[Leader][actor] OperationResult (non-station): op_id={} success={} op={:?} error={:?}",
                        //     op_id, success, operation, error
                        // );
                        return;
                    }
                };

                // For now, station-originated operations are expected to be charges.
                if !matches!(operation, ActorOperation::Charge { .. }) {
                    // println!(
                    //     "[Leader][actor] OperationResult for non-Charge station op: op_id={} op={:?}",
                    //     op_id, operation
                    // );
                }

                let allowed = success;
                let mapped_error: Option<VerifyError> = if allowed {
                    None
                } else {
                    // Propagate the business error (limit, etc.) if present.
                    error
                };

                // println!(
                //     "[Leader][actor→station] op_id={} -> CHARGE {} (acc={}, card={}, amount={}, error={:?})",
                //     op_id,
                //     if allowed { "CONFIRMED" } else { "DENIED" },
                //     pending.account_id,
                //     pending.card_id,
                //     pending.amount,
                //     mapped_error
                // );

                let msg = NodeToStationMsg::ChargeResult {
                    request_id: op_id,
                    allowed,
                    error: mapped_error,
                };

                if let Err(e) = self.station_result_tx.send(msg).await {
                    // println!(
                    //     "[Leader][actor→station][ERROR] failed to send result: {}",
                    //     e
                    // );
                }
            }

            // --------------------------------------------------
            // Other events (debug, etc.)
            // --------------------------------------------------
            ActorEvent::Debug(msg) => {
                //println!("[Leader][actor][debug] {}", msg);
            }
        }
    }

    // ==========================
    // MAIN LOOP IMPLEMENTATION
    // ==========================
    async fn run_loop(&mut self) -> AppResult<()> {
        let mut net_open = true;
        let mut actors_open = true;
        let mut station_open = true;

        while net_open || actors_open || station_open {
            // Build futures without borrowing &mut self multiple times.
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
                // ======================
                // Network events
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
                                        Err(_e) => {
                                            //println!("[WARN][Leader] invalid msg from network: {:?}", e),
                                        }
                                    }
                                }
                                InboundEvent::ConnClosed { peer } => {
                                    //println!("[Leader] Connection closed by {}", peer);
                                }
                            }
                        }
                        None => {
                            //println!("[Leader] Network channel closed");
                            net_open = false;
                        }
                    }
                }

                // ======================
                // Actor events
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
                            //println!("[Leader] Actor event channel closed");
                            actors_open = false;
                        }
                    }
                }

                // ======================
                // Station → Leader events
                // ======================
                maybe_station = async {
                    match station_fut {
                        Some(fut) => fut.await,
                        None => None
                    }
                }, if station_open => {
                    match maybe_station {
                        Some(msg) => self.handle_station_msg(msg).await,
                        None => {
                            //println!("[Leader] Station command channel closed");
                            station_open = false;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

// ==========================================================
// Leader helpers
// ==========================================================
impl Leader {
    /// Handle a message coming from the station (pump simulator).
    ///
    /// Currently we only support `ChargeRequest`, which is mapped to a single
    /// `Operation::Charge` that goes through the internal Execute flow
    /// (verify + apply inside the actor layer).
    async fn handle_station_msg(&mut self, msg: StationToNodeMsg) {
        match msg {
            StationToNodeMsg::ChargeRequest {
                pump_id,
                account_id,
                card_id,
                amount,
                request_id,
            } => {
                // println!(
                //     "[Leader][station] pump={} -> ChargeRequest(account={}, card={}, amount={}, request_id={})",
                //     pump_id, account_id, card_id, amount, request_id
                // );

                // Keep minimal state to be able to log and confirm later.
                self.station_requests.insert(
                    request_id,
                    StationPendingCharge {
                        account_id,
                        card_id,
                        amount,
                    },
                );

                // Build the domain operation for the actor layer.
                let op = ActorOperation::Charge {
                    account_id,
                    card_id,
                    amount,
                };

                // Trigger the full execute flow in the actor system.
                self.router.do_send(RouterCmd::Execute {
                    op_id: request_id,
                    operation: op,
                });
            }
        }
    }

    /// Start a Leader node:
    ///
    /// - boots the ConnectionManager (TCP),
    /// - boots the ActorRouter in a dedicated Actix system thread,
    /// - spawns the station simulator with `pumps` pumps on stdin,
    /// - then enters the main async event loop.
    pub async fn start(
        address: SocketAddr,
        coords: (f64, f64),
        replicas: Vec<SocketAddr>,
        max_conns: usize,
        pumps: usize,
    ) -> AppResult<()> {
        // -------------------------
        // 1) Network (ConnectionManager)
        // -------------------------
        let (connection_tx, connection_rx) = ConnectionManager::start(address, max_conns);

        // -------------------------
        // 2) ActorRouter → Leader channel
        // -------------------------
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);

        // -------------------------
        // 3) Station ↔ Leader channels
        // -------------------------
        let (station_cmd_tx, station_cmd_rx) = mpsc::channel::<StationToNodeMsg>(128);
        let (station_result_tx, station_result_rx) = mpsc::channel::<NodeToStationMsg>(128);

        // Spawn the station simulator (stdin-driven pumps).
        let station_cmd_tx_for_station = station_cmd_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::node::station::run_station_simulator(
                pumps,
                station_cmd_tx_for_station,
                station_result_rx,
            )
            .await
            {
                //println!("[Station] simulator error: {e:?}");
            }
        });

        // -------------------------
        // 4) Launch Actix + ActorRouter on a separate OS thread
        // -------------------------
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();

        std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(actor_tx).start();

                if router_tx.send(router.clone()).is_err() {
                    //println!("[ERROR] Failed to deliver ActorRouter addr to Leader");
                }

                pending::<()>().await;
            });
        });

        // -------------------------
        // 5) Build Leader
        // -------------------------
        let mut leader = Self {
            id: rand::random::<u64>(),
            coords,
            max_conns,
            replicas,
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

        // Enter main event loop.
        leader.run().await
    }
}

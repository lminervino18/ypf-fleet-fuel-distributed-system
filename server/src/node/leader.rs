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
use std::{
    collections::{HashMap, VecDeque},
    future::pending,
    net::SocketAddr,
};
use tokio::sync::{mpsc, oneshot};

/// Internal state for a charge coming from a station pump
/// while the node is ONLINE.
///
/// These are requests for which we *expect* an OperationResult
/// from the ActorRouter.
#[derive(Debug, Clone)]
struct StationPendingCharge {
    account_id: u64,
    card_id: u64,
    amount: f64,
}

/// Internal state for a charge coming from a station pump
/// while the node is OFFLINE.
///
/// These operations are:
/// - immediately acknowledged to the station as OK,
/// - not sent to the actor system while offline,
/// - stored in this queue so that, once connectivity is restored,
///   they can be replayed into the ActorRouter with the
///   `from_offline_station = true` flag set.
#[derive(Debug, Clone)]
struct OfflineQueuedCharge {
    request_id: u64,
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
/// ONLINE behavior (default):
/// --------------------------
/// For a `Charge` coming from a pump the flow is:
///
/// 1. Station → Leader: `StationToNodeMsg::ChargeRequest`.
/// 2. Leader builds `ActorOperation::Charge { .., from_offline_station: false }`
///    and sends `RouterCmd::Execute { op_id, operation }` to ActorRouter.
/// 3. ActorRouter/actors run verification + apply internally.
/// 4. Once finished, ActorRouter emits a single
///    `ActorEvent::OperationResult { op_id, operation, success, error }`.
/// 5. Leader translates that into a `NodeToStationMsg::ChargeResult`
///    and sends it back to the station, removing the pending request.
///
/// OFFLINE behavior:
/// -----------------
/// - Station → Leader: `StationToNodeMsg::DisconnectNode`
///   sets `is_offline = true`.
/// - When `is_offline == true`:
///   * network events from the cluster are ignored (only drained),
///   * pump-originated `ChargeRequest`s are:
///       - enqueued into `offline_queue`,
///       - immediately acknowledged to the station as `allowed = true`,
///       - **not** sent to the ActorRouter yet.
/// - Station → Leader: `StationToNodeMsg::ConnectNode`
///   sets `is_offline = false` and **replays** all queued charges to
///   the ActorRouter with `from_offline_station = true`.
pub struct Leader {
    id: u64,
    coords: (f64, f64),
    max_conns: usize,
    replicas: Vec<SocketAddr>,

    /// Consensus operations (placeholder for future use).
    operations: HashMap<u8, NodeOperation>,

    /// Whether the node is in OFFLINE mode.
    ///
    /// In OFFLINE mode:
    /// - cluster/network traffic is ignored,
    /// - pump operations are auto-approved and queued in `offline_queue`,
    /// - the actor layer is not involved in those pump operations until
    ///   connectivity is restored.
    is_offline: bool,

    /// Queue of pump-originated operations received while OFFLINE.
    ///
    /// When the node switches back to ONLINE (ConnectNode), all entries
    /// in this queue are replayed into the ActorRouter as
    /// `Operation::Charge { .., from_offline_station: true }`.
    offline_queue: VecDeque<OfflineQueuedCharge>,

    // ===== Network (ConnectionManager) =====
    connection_tx: mpsc::Sender<ManagerCmd>,
    connection_rx: mpsc::Receiver<InboundEvent>,

    // ===== ActorRouter → Leader =====
    actor_rx: mpsc::Receiver<ActorEvent>,

    // ===== Station ↔ Leader =====
    // Station → Leader (commands from pumps / console)
    station_cmd_rx: mpsc::Receiver<StationToNodeMsg>,
    // Leader → Station (final results / debug)
    station_result_tx: mpsc::Sender<NodeToStationMsg>,

    /// In-flight charge requests coming from the station
    /// while the node is ONLINE.
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
        // println!("[Leader] handle_accept({:?})", op);
        // TODO: integrate with real consensus protocol.
    }

    async fn handle_learn(&mut self, op: NodeOperation) {
        // println!("[Leader] handle_learn({:?})", op);
        // TODO
    }

    async fn handle_commit(&mut self, op: NodeOperation) {
        // println!("[Leader] handle_commit({:?})", op);
        // TODO
    }

    async fn handle_finished(&mut self, op: NodeOperation) {
        // println!("[Leader] handle_finished({:?})", op);
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
                // For pump-originated ONLINE operations we track them in
                // `station_requests`. OFFLINE replay operations are never
                // inserted there, so they will be treated as "non-station"
                // and ignored at this layer (they were already confirmed
                // to the station while offline).
                let pending = match self.station_requests.remove(&op_id) {
                    Some(p) => p,
                    None => {
                        // Not a pump request (maybe admin / consensus op),
                        // or an OFFLINE replay operation. We ignore it.
                        // println!(
                        //     "[Leader][actor] OperationResult (non-station/offline): op_id={} success={} op={:?} error={:?}",
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
                // println!("[Leader][actor][debug] {}", msg);
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
                // Network events (cluster)
                // ======================
                maybe_evt = async {
                    match net_fut {
                        Some(fut) => fut.await,
                        None => None
                    }
                }, if net_open => {
                    match maybe_evt {
                        Some(evt) => {
                            if self.is_offline {
                                // In OFFLINE mode we ignore all cluster
                                // traffic. We still drain the channel to
                                // avoid backpressure.
                                // println!("[Leader][net] Ignoring network event in OFFLINE mode: {:?}", evt);
                            } else {
                                match evt {
                                    InboundEvent::Received { peer: _, payload } => {
                                        match NodeMessage::try_from(payload) {
                                            Ok(msg) => self.handle_node_msg(msg).await,
                                            Err(_e) => {
                                                // println!("[WARN][Leader] invalid msg from network: {:?}", e);
                                            }
                                        }
                                    }
                                    InboundEvent::ConnClosed { peer } => {
                                        // println!("[Leader] Connection closed by {}", peer);
                                    }
                                }
                            }
                        }
                        None => {
                            // println!("[Leader] Network channel closed");
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
                            // println!("[Leader] Actor event channel closed");
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
                            // println!("[Leader] Station command channel closed");
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
    /// - ONLINE:
    ///   `ChargeRequest` is mapped to `Operation::Charge` and goes
    ///   through the internal Execute flow (verify + apply).
    ///
    /// - OFFLINE:
    ///   `ChargeRequest` is enqueued into `offline_queue` and immediately
    ///   acknowledged to the station as OK, without touching the actor layer.
    ///
    /// - `DisconnectNode`:
    ///   switches the node into OFFLINE mode.
    ///
    /// - `ConnectNode`:
    ///   switches the node back into ONLINE mode **and replays**
    ///   all offline-queued charges into the ActorRouter as
    ///   `from_offline_station = true`.
    async fn handle_station_msg(&mut self, msg: StationToNodeMsg) {
        match msg {
            StationToNodeMsg::ChargeRequest {
                pump_id: _,
                account_id,
                card_id,
                amount,
                request_id,
            } => {
                if self.is_offline {
                    // OFFLINE MODE:
                    // -------------
                    // Queue the logical operation so that, when we reconnect,
                    // a reconciliation step can replay it into the actor layer.
                    self.offline_queue.push_back(OfflineQueuedCharge {
                        request_id,
                        account_id,
                        card_id,
                        amount,
                    });

                    // Immediately respond to the station as if the
                    // operation had succeeded.
                    let msg = NodeToStationMsg::ChargeResult {
                        request_id,
                        allowed: true,
                        error: None,
                    };

                    if let Err(e) = self.station_result_tx.send(msg).await {
                        // println!(
                        //     "[Leader][station][OFFLINE][ERROR] failed to send fake-OK result: {}",
                        //     e
                        // );
                    }

                    return;
                }

                // ONLINE MODE:
                // ------------
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
                    from_offline_station: false,
                };

                // Trigger the full execute flow in the actor system.
                self.router.do_send(RouterCmd::Execute {
                    op_id: request_id,
                    operation: op,
                });
            }

            StationToNodeMsg::DisconnectNode => {
                if self.is_offline {
                    // Already offline; just inform the station.
                    let _ = self
                        .station_result_tx
                        .send(NodeToStationMsg::Debug(
                            "[Leader] Node is already in OFFLINE mode; pump operations are auto-approved."
                                .to_string(),
                        ))
                        .await;
                } else {
                    // Switch to OFFLINE mode.
                    self.is_offline = true;

                    // Optional: here we could instruct the ConnectionManager
                    // to shut down or stop accepting new connections via a
                    // ManagerCmd if such a command existed.
                    //
                    // For now, we simply:
                    // - ignore future network events (see run_loop),
                    // - keep the existing connections alive but inert.

                    let _ = self
                        .station_result_tx
                        .send(NodeToStationMsg::Debug(
                            "[Leader] Node switched to OFFLINE mode. Cluster traffic will be ignored and pump operations will be queued and auto-approved."
                                .to_string(),
                        ))
                        .await;

                    // println!("[Leader] OFFLINE mode enabled");
                }
            }

            StationToNodeMsg::ConnectNode => {
                if !self.is_offline {
                    // Already online; just inform the station.
                    let _ = self
                        .station_result_tx
                        .send(NodeToStationMsg::Debug(
                            "[Leader] Node is already in ONLINE mode; pump operations go through normal verification."
                                .to_string(),
                        ))
                        .await;
                } else {
                    // Switch back to ONLINE mode.
                    self.is_offline = false;

                    // Take the number of queued operations *before* draining.
                    let queued = self.offline_queue.len();

                    // Replay all queued offline charges into the actor system.
                    //
                    // We reuse `request_id` as `op_id` so that, if needed,
                    // logs can correlate them directly with the original
                    // offline pump requests. We deliberately do NOT insert
                    // them into `station_requests`, so OperationResult
                    // events for these ops will be treated as non-station
                    // events and ignored by `handle_actor_event`.
                    while let Some(OfflineQueuedCharge {
                        request_id,
                        account_id,
                        card_id,
                        amount,
                    }) = self.offline_queue.pop_front()
                    {
                        let op = ActorOperation::Charge {
                            account_id,
                            card_id,
                            amount,
                            from_offline_station: true,
                        };

                        self.router.do_send(RouterCmd::Execute {
                            op_id: request_id,
                            operation: op,
                        });
                    }

                    let _ = self
                        .station_result_tx
                        .send(NodeToStationMsg::Debug(
                            format!(
                                "[Leader] Node switched back to ONLINE mode. Replayed {} queued offline operations into the actor system (they were already confirmed to the station).",
                                queued
                            ),
                        ))
                        .await;

                    // println!("[Leader] ONLINE mode enabled, offline_queue replayed: {}", queued);
                }
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
                // println!("[Station] simulator error: {e:?}");
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
                    // println!("[ERROR] Failed to deliver ActorRouter addr to Leader");
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

            is_offline: false,
            offline_queue: VecDeque::new(),

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

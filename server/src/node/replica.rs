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

/// Replica node.
///
/// Same wiring as the Leader, but intended to be a follower in the
/// distributed system. For station-originated charges it uses the
/// exact same Execute (verify + apply interno) flow as the Leader
/// when ONLINE, and the same offline queueing behavior when OFFLINE.
pub struct Replica {
    id: u64,
    coords: (f64, f64),
    max_conns: usize,
    leader_addr: SocketAddr,
    other_replicas: Vec<SocketAddr>,

    /// Consensus operations (placeholder for now).
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

    // ===== ActorRouter → Replica =====
    actor_rx: mpsc::Receiver<ActorEvent>,

    // ===== Station ↔ Replica =====
    // Station → Replica (commands from pumps / console)
    station_cmd_rx: mpsc::Receiver<StationToNodeMsg>,
    // Replica → Station (final results / debug)
    station_result_tx: mpsc::Sender<NodeToStationMsg>,

    /// In-flight charge requests coming from the station
    /// while the node is ONLINE.
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
        // println!("[Replica] handle_accept({:?})", op);
        // TODO: integrate with real consensus.
    }

    async fn handle_learn(&mut self, op: NodeOperation) {
        // println!("[Replica] handle_learn({:?})", op);
        // TODO
    }

    async fn handle_commit(&mut self, op: NodeOperation) {
        // println!("[Replica] handle_commit({:?})", op);
        // TODO
    }

    async fn handle_finished(&mut self, op: NodeOperation) {
        // println!("[Replica] handle_finished({:?})", op);
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
                        // Not from station: just ignore (admin/consensus or
                        // offline-replayed op).
                        // println!(
                        //     "[Replica][actor] OperationResult (non-station/offline): op_id={} success={} op={:?} error={:?}",
                        //     op_id, success, operation, error
                        // );
                        return;
                    }
                };

                if !matches!(operation, ActorOperation::Charge { .. }) {
                    // println!(
                    //     "[Replica][actor] OperationResult for non-Charge operation from station: op_id={} op={:?}",
                    //     op_id, operation
                    // );
                }

                let allowed = success;
                let mapped_error: Option<VerifyError> = if allowed {
                    None
                } else {
                    error
                };

                let msg = NodeToStationMsg::ChargeResult {
                    request_id: op_id,
                    allowed,
                    error: mapped_error,
                };

                if let Err(e) = self.station_result_tx.send(msg).await {
                    // println!(
                    //     "[Replica][actor→station][ERROR] failed to send result: {}",
                    //     e
                    // );
                }
            }

            // --------------------------------------------------
            // Other events: debug, etc.
            // --------------------------------------------------
            ActorEvent::Debug(msg) => {
                // println!("[Replica][actor][debug] {}", msg);
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
                            if self.is_offline {
                                // In OFFLINE mode we ignore all cluster
                                // traffic. We still drain the channel to
                                // avoid backpressure.
                                // println!("[Replica][net] Ignoring network event in OFFLINE mode: {:?}", evt);
                            } else {
                                match evt {
                                    InboundEvent::Received { peer: _, payload } => {
                                        match NodeMessage::try_from(payload) {
                                            Ok(msg) => self.handle_node_msg(msg).await,
                                            Err(_e) => {
                                                // println!("[WARN][Replica] invalid msg from network: {:?}", e);
                                            }
                                        }
                                    }
                                    InboundEvent::ConnClosed { peer } => {
                                        // println!("[Replica] Connection closed by {}", peer);
                                    }
                                }
                            }
                        }
                        None => {
                            // println!("[Replica] Network channel closed");
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
                            // println!("[Replica] Actor event channel closed");
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
                            // println!("[Replica] Station command channel closed");
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
// Station → Replica handler
// ===============================================================
impl Replica {
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
                        //     "[Replica][station][OFFLINE][ERROR] failed to send fake-OK result: {}",
                        //     e
                        // );
                    }

                    return;
                }

                // ONLINE MODE:
                // ------------
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
                    from_offline_station: false,
                };

                // Execute (verify + apply internal).
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
                            "[Replica] Node is already in OFFLINE mode; pump operations are auto-approved."
                                .to_string(),
                        ))
                        .await;
                } else {
                    self.is_offline = true;

                    // Optional: we could instruct the ConnectionManager
                    // to stop accepting/initiating connections via
                    // a ManagerCmd if such a command existed.

                    let _ = self
                        .station_result_tx
                        .send(NodeToStationMsg::Debug(
                            "[Replica] Node switched to OFFLINE mode. Cluster traffic will be ignored and pump operations will be queued and auto-approved."
                                .to_string(),
                        ))
                        .await;

                    // println!("[Replica] OFFLINE mode enabled");
                }
            }

            StationToNodeMsg::ConnectNode => {
                if !self.is_offline {
                    // Already online; just inform the station.
                    let _ = self
                        .station_result_tx
                        .send(NodeToStationMsg::Debug(
                            "[Replica] Node is already in ONLINE mode; pump operations go through normal verification."
                                .to_string(),
                        ))
                        .await;
                } else {
                    self.is_offline = false;

                    let queued = self.offline_queue.len();

                    // Replay all queued offline charges into the actor system.
                    // We reuse `request_id` as `op_id` and do NOT insert them
                    // into `station_requests`, so their OperationResult will be
                    // treated as non-station/offline events and ignored here.
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
                                "[Replica] Node switched back to ONLINE mode. Replayed {} queued offline operations into the actor system (they were already confirmed to the station).",
                                queued
                            ),
                        ))
                        .await;

                    // println!("[Replica] ONLINE mode enabled, offline_queue replayed: {}", queued);
                }
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
                // println!("[Station][Replica] simulator error: {e:?}");
            }
        });

        // 4) Launch Actix + ActorRouter in another OS thread
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();

        std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(actor_tx).start();

                if router_tx.send(router.clone()).is_err() {
                    // println!("[ERROR] Failed to send router address to Replica");
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

        // Enter main loop
        replica.run().await
    }
}

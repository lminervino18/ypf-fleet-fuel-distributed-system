use super::{message::Message, network::Connection, node::Node, operation::Operation};
use crate::actors::{ActorEvent, RouterCmd};
use crate::domain::Operation as ActorOperation;
use crate::{
    actors::actor_router::ActorRouter,
    errors::{AppError, AppResult},
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
    address: SocketAddr,
    max_conns: usize,
    replicas: Vec<SocketAddr>,
    operations: HashMap<u32, (usize, SocketAddr, Operation)>,
    connection: Connection,
    actor_rx: mpsc::Receiver<ActorEvent>,
    is_offline: bool,
    offline_queue: VecDeque<OfflineQueuedCharge>,
    station_cmd_rx: mpsc::Receiver<StationToNodeMsg>,
    station_result_tx: mpsc::Sender<NodeToStationMsg>,
    station_requests: HashMap<u64, StationPendingCharge>,
    router: Addr<ActorRouter>,
}

// ==========================================================
// Node trait implementation
// ==========================================================
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

        todo!();
    }

    async fn recv_node_msg(&mut self) -> AppResult<Message> {
        self.connection.recv().await
    }

    async fn recv_actor_event(&mut self) -> Option<ActorEvent> {
        self.actor_rx.recv().await
    }

    async fn handle_actor_event(&mut self, event: ActorEvent) {}
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

    async fn handle_election(&mut self, candidate_id: u64, candidate_addr: SocketAddr) {
        // If our id is higher than the candidate, reply with ElectionOk
        if self.id > candidate_id {
            let reply = Message::ElectionOk {
                responder_id: self.id,
                responder_addr: self.address,
            };

            self.connection.send(reply, &candidate_addr);
        }
    }

    async fn handle_election_ok(&mut self, _responder_id: u64, _responder_addr: SocketAddr) {
        // Leader ignores election OKs
    }

    async fn handle_coordinator(&mut self, _leader_id: u64, _leader_addr: SocketAddr) {
        // Leader ignores coordinator announcements for now
    }

    async fn start_election(&mut self) {
        // Leader doesn't start elections
    }
}

impl Leader {
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
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();
        let (station_cmd_tx, station_cmd_rx) = mpsc::channel::<StationToNodeMsg>(128);
        let (station_result_tx, station_result_rx) = mpsc::channel::<NodeToStationMsg>(128);
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

        std::thread::spawn(move || {
            // spawns a thread for Actix that runs inside a Tokio runtime 
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(actor_tx).start();
                if router_tx.send(router.clone()).is_err() {
                    // println!("[ERROR] Failed to deliver ActorRouter addr to Leader");
                }

                pending::<()>().await;      // Keep the actix system running indefinitely
            });
        });

        let mut leader = Self {
            id: rand::random::<u64>(),
            coords,
            address,
            max_conns,
            replicas,
            operations: HashMap::new(),
            connection: Connection::start(address, max_conns).await?,
            actor_rx,
            is_offline: false,
            offline_queue: VecDeque::new(),
            station_cmd_rx,
            station_result_tx,
            station_requests: HashMap::new(),
            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive ActorRouter address: {e}"),
            })?,
        };

        leader.run().await
    }
}

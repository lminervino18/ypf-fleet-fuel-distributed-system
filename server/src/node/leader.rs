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
    node::station::{NodeToStationMsg, StationToNodeMsg},
};
use actix::{Actor, Addr};
use std::{collections::HashMap, future::pending, net::SocketAddr};
use tokio::sync::{mpsc, oneshot};

/// Internal state associated with a charge request coming from a station pump.
///
/// We store this so that:
/// - after a successful limit check we can build `ApplyCharge`,
/// - after `ChargeApplied` we know which original request_id to confirm
///   back to the station.
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
/// Flow for a charge coming from a pump:
/// 1. Station sends `StationToNodeMsg::ChargeRequest`.
/// 2. Leader stores `StationPendingCharge` keyed by `request_id`.
/// 3. Leader sends `RouterCmd::AuthorizeCharge` to ActorRouter.
/// 4. Actor layer emits:
///    - `ActorEvent::LimitCheckResult` (allowed / denied),
///      and if allowed:
///    - `ActorEvent::ChargeApplied` when the charge is actually applied.
/// 5. Leader converts these actor events into a single `NodeToStationMsg::ChargeResult`
///    back to the station.
pub struct Leader {
    id: u64,
    coords: (f64, f64),
    max_conns: usize,
    replicas: Vec<SocketAddr>,

    /// Consensus operations (not really used yet, but kept for future logic).
    operations: HashMap<u8, Operation>,

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
    /// Key = `request_id` chosen by the station simulator.
    station_requests: HashMap<u64, StationPendingCharge>,

    /// Actix ActorRouter handle.
    router: Addr<ActorRouter>,
}

impl Node for Leader {
    // ==========================
    // Consensus handlers
    // ==========================
    async fn handle_accept(&mut self, op: Operation) {
        println!("[Leader] handle_accept({:?})", op);
        // TODO: integrate with real consensus protocol.
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
            // ---- Limit check result (authorization) ----
            ActorEvent::LimitCheckResult {
                request_id,
                allowed,
                error,
            } => {
                // Is this a request coming from the station?
                let pending = match self.station_requests.get(&request_id) {
                    Some(p) => p.clone(),
                    None => {
                        // Could be an internal / admin request not related to pumps.
                        match &error {
                            None => {
                                println!(
                                    "[Leader][actor] LimitCheckResult (non-station): request_id={} allowed={}",
                                    request_id, allowed
                                );
                            }
                            Some(err) => {
                                println!(
                                    "[Leader][actor] LimitCheckResult (non-station): request_id={} allowed={} error={:?}",
                                    request_id, allowed, err
                                );
                            }
                        }
                        return;
                    }
                };

                if !allowed {
                    println!(
                        "[Leader][actor→station] request_id={} -> DENIED (error={:?})",
                        request_id, error
                    );

                    // We stop here (no ApplyCharge). Reply to station and forget the request.
                    self.station_requests.remove(&request_id);

                    let msg = NodeToStationMsg::ChargeResult {
                        request_id,
                        allowed: false,
                        error,
                    };

                    if let Err(e) = self.station_result_tx.send(msg).await {
                        eprintln!(
                            "[Leader][actor→station][ERROR] failed to send DENIED result: {}",
                            e
                        );
                    }
                } else {
                    println!(
                        "[Leader][actor] LimitCheckResult: request_id={} allowed=true -> ApplyCharge",
                        request_id
                    );

                    // Build ApplyCharge from the pending state we stored when the pump requested.
                    self.router.do_send(RouterCmd::ApplyCharge {
                        account_id: pending.account_id,
                        card_id: pending.card_id,
                        amount: pending.amount,
                        timestamp: 0,      // TODO: real timestamp if needed
                        op_id: request_id, // correlate op_id == request_id
                        request_id,
                    });
                    // We DO NOT reply to station yet:
                    // wait for ChargeApplied to confirm that the state mutation actually happened.
                }
            }

            // ---- Charge was effectively applied on actors side ----
            ActorEvent::ChargeApplied { operation } => {
                println!("[Leader][actor] ChargeApplied: operation={:?}", operation);

                // Only care if `op_id` matches a station request.
                if let crate::actors::types::Operation::Charge { op_id, .. } = operation {
                    if self.station_requests.remove(&op_id).is_some() {
                        println!(
                            "[Leader][actor→station] request_id={} -> CHARGE CONFIRMED",
                            op_id
                        );
                        let msg = NodeToStationMsg::ChargeResult {
                            request_id: op_id,
                            allowed: true,
                            error: None,
                        };
                        if let Err(e) = self.station_result_tx.send(msg).await {
                            eprintln!(
                                "[Leader][actor→station][ERROR] failed to send OK result: {}",
                                e
                            );
                        }
                    } else {
                        // ChargeApplied not initiated by the station (or already cleaned up).
                        println!(
                            "[Leader][actor] ChargeApplied with op_id={} not tracked as station request",
                            op_id
                        );
                    }
                } else {
                    println!("[Leader][actor] ChargeApplied with non-Charge operation");
                }
            }

            // ---- Other actor events (limits, debug, etc.) ----
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
                                        Err(e) =>
                                            eprintln!("[WARN][Leader] invalid msg from network: {:?}", e),
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
                            println!("[Leader] Actor event channel closed");
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
                            println!("[Leader] Station command channel closed");
                            station_open = false;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

impl Leader {
    /// Handle a message coming from the station (pump simulator).
    ///
    /// Currently we only support `ChargeRequest`, which represents:
    /// "please authorize + apply this charge for (account, card, amount)".
    async fn handle_station_msg(&mut self, msg: StationToNodeMsg) {
        match msg {
            StationToNodeMsg::ChargeRequest {
                pump_id,
                account_id,
                card_id,
                amount,
                request_id,
            } => {
                println!(
                    "[Leader][station] pump={} -> ChargeRequest(account={}, card={}, amount={}, request_id={})",
                    pump_id, account_id, card_id, amount, request_id
                );

                // Store the pending charge so we can:
                // - build ApplyCharge after LimitCheckResult,
                // - know what to confirm back when ChargeApplied fires.
                self.station_requests.insert(
                    request_id,
                    StationPendingCharge {
                        account_id,
                        card_id,
                        amount,
                    },
                );

                // Trigger limit check in the actor system.
                self.router.do_send(RouterCmd::AuthorizeCharge {
                    account_id,
                    card_id,
                    amount,
                    request_id,
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
        // `pumps` is the number of independent pump IDs allowed.
        let station_cmd_tx_for_station = station_cmd_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::node::station::run_station_simulator(
                pumps,
                station_cmd_tx_for_station,
                station_result_rx,
            )
            .await
            {
                eprintln!("[Station] simulator error: {e:?}");
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
                    eprintln!("[ERROR] Failed to deliver ActorRouter addr to Leader");
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

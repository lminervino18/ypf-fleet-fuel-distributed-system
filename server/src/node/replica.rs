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

/// Internal state for a charge coming from a station pump.
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

/// Replica node.
///
/// The Replica wires together:
/// - the TCP ConnectionManager (network),
/// - the local ActorRouter (Actix),
/// - the station simulator (stdin-driven pumps),
/// in the same way as the Leader, but intended to be a follower in the
/// distributed system.
///
/// Flow for a charge coming from a pump:
/// 1. Station sends `StationToNodeMsg::ChargeRequest`.
/// 2. Replica stores `StationPendingCharge` keyed by `request_id`.
/// 3. Replica sends `RouterCmd::AuthorizeCharge` to ActorRouter.
/// 4. Actor layer emits:
///    - `ActorEvent::LimitCheckResult` (allowed / denied),
///      and if allowed:
///    - `ActorEvent::ChargeApplied` when the charge is actually applied.
/// 5. Replica converts these actor events into a single
///    `NodeToStationMsg::ChargeResult` back to the station.
pub struct Replica {
    id: u64,
    coords: (f64, f64),
    max_conns: usize,
    leader_addr: SocketAddr,
    other_replicas: Vec<SocketAddr>,

    /// Consensus operations (placeholder for now).
    operations: HashMap<u8, Operation>,

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
    /// Key = `request_id` chosen by the station simulator.
    station_requests: HashMap<u64, StationPendingCharge>,

    /// Local Actix ActorRouter handle.
    router: Addr<ActorRouter>,
}

// ===============================================================
// Node trait implementation
// ===============================================================
impl Node for Replica {
    async fn handle_accept(&mut self, op: Operation) {
        println!("[Replica] handle_accept({:?})", op);
        // TODO: integrate with real consensus.
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
                        // Not from station: just log it.
                        match &error {
                            None => {
                                println!(
                                    "[Replica][actor] LimitCheckResult (non-station): request_id={} allowed={}",
                                    request_id, allowed
                                );
                            }
                            Some(err) => {
                                println!(
                                    "[Replica][actor] LimitCheckResult (non-station): request_id={} allowed={} error={:?}",
                                    request_id, allowed, err
                                );
                            }
                        }
                        return;
                    }
                };

                if !allowed {
                    println!(
                        "[Replica][actor→station] request_id={} -> DENIED (error={:?})",
                        request_id, error
                    );

                    // Do not continue with ApplyCharge; respond directly to station.
                    self.station_requests.remove(&request_id);

                    let msg = NodeToStationMsg::ChargeResult {
                        request_id,
                        allowed: false,
                        error,
                    };

                    if let Err(e) = self.station_result_tx.send(msg).await {
                        eprintln!(
                            "[Replica][actor→station][ERROR] failed to send DENIED result: {}",
                            e
                        );
                    }
                } else {
                    println!(
                        "[Replica][actor] LimitCheckResult: request_id={} allowed=true -> ApplyCharge",
                        request_id
                    );

                    // Fire ApplyCharge into the actor system using stored state.
                    self.router.do_send(RouterCmd::ApplyCharge {
                        account_id: pending.account_id,
                        card_id: pending.card_id,
                        amount: pending.amount,
                        timestamp: 0,      // TODO: real timestamp
                        op_id: request_id, // correlate op_id == request_id
                        request_id,
                    });
                    // Do not respond to station yet: wait for ChargeApplied.
                }
            }

            // ---- Confirmation that the charge was applied ----
            ActorEvent::ChargeApplied { operation } => {
                println!("[Replica][actor] ChargeApplied: operation={:?}", operation);

                if let crate::actors::types::Operation::Charge { op_id, .. } = operation {
                    if self.station_requests.remove(&op_id).is_some() {
                        println!(
                            "[Replica][actor→station] request_id={} -> CHARGE CONFIRMED",
                            op_id
                        );
                        let msg = NodeToStationMsg::ChargeResult {
                            request_id: op_id,
                            allowed: true,
                            error: None,
                        };
                        if let Err(e) = self.station_result_tx.send(msg).await {
                            eprintln!(
                                "[Replica][actor→station][ERROR] failed to send OK result: {}",
                                e
                            );
                        }
                    } else {
                        println!(
                            "[Replica][actor] ChargeApplied with op_id={} not tracked as station request",
                            op_id
                        );
                    }
                } else {
                    println!("[Replica][actor] ChargeApplied with non-Charge operation");
                }
            }

            // ---- Other events: limit updates, failures, debug ----
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
    // Main event loop: same structure as Leader (net + actors + station)
    // ===============================================================
    async fn run_loop(&mut self) -> AppResult<()> {
        let mut net_open = true;
        let mut actors_open = true;
        let mut station_open = true;

        while net_open || actors_open || station_open {
            // Build futures without borrowing &mut self twice.
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
                                        Err(e) =>
                                            eprintln!("[WARN][Replica] invalid msg from network: {:?}", e),
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
                            println!("[Replica] Station command channel closed");
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
    /// Currently we only support `ChargeRequest`, same as in Leader.
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
                    "[Replica][station] pump={} -> ChargeRequest(account={}, card={}, amount={}, request_id={})",
                    pump_id, account_id, card_id, amount, request_id
                );

                // Store state to:
                // - build ApplyCharge after LimitCheckResult,
                // - know which request_id to confirm to station after ChargeApplied.
                self.station_requests.insert(
                    request_id,
                    StationPendingCharge {
                        account_id,
                        card_id,
                        amount,
                    },
                );

                // Trigger limit check on actor side.
                self.router.do_send(RouterCmd::AuthorizeCharge {
                    account_id,
                    card_id,
                    amount,
                    request_id,
                });
            }
        }
    }

    // ===============================================================
    // Start function
    // ===============================================================
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

        // Spawn the station simulator with the given number of pumps.
        let station_cmd_tx_for_station = station_cmd_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::node::station::run_station_simulator(
                pumps,
                station_cmd_tx_for_station,
                station_result_rx,
            )
            .await
            {
                eprintln!("[Station][Replica] simulator error: {e:?}");
            }
        });

        // 4) Launch Actix + ActorRouter in another OS thread
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();

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

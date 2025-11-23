use super::actors::{actor_router::ActorRouter, ActorEvent, RouterCmd};
use super::node::Node;
use crate::errors::VerifyError;
use crate::{
    errors::{AppError, AppResult},
    node::utils::get_id_given_addr,
};
use actix::{Actor, Addr};
use common::operation::Operation;
use common::{Connection, Message, NodeToStationMsg, Station};
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
    amount: f32,
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
    amount: f32,
}

/// Leader node.
///
/// The Leader wires together:
/// - the TCP ConnectionManager (network),
/// - the local ActorRouter (Actix),
/// - the Station abstraction (which internally runs the stdin-driven pump simulator).
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
    /// Current cluster membership: node_id -> address (including self).
    members: HashMap<u64, SocketAddr>,
    /// Stored operations indexed by op_id.
    operations: HashMap<u32, (usize, SocketAddr, Operation)>,
    // connection: Connection,
    // actor_rx: mpsc::Receiver<ActorEvent>,
    is_offline: bool,
    offline_queue: VecDeque<OfflineQueuedCharge>,
    /// High-level Station wrapper (owns the simulator task and channels).
    // station: Station,
    /// Pending ONLINE station-originated charges.
    station_requests: HashMap<u64, StationPendingCharge>,
    router: Addr<ActorRouter>,
}

// ==========================================================
// Node trait implementation
// ==========================================================
impl Node for Leader {
    async fn handle_request(
        &mut self,
        connection: &mut Connection,
        op_id: u32,
        op: Operation,
        client_addr: SocketAddr,
    ) -> AppResult<()> {
        // Store the operation locally.
        self.operations.insert(op_id, (0, client_addr, op.clone()));
        // Build the log message to replicate.
        let msg = Message::Log {
            op_id,
            op: op.clone(),
        };
        // Broadcast the log to all known members except self.
        for (node_id, addr) in &self.members {
            if *node_id == self.id {
                continue;
            }
            connection.send(msg.clone(), addr).await?;
        }

        Ok(())
    }

    async fn handle_log(&mut self, _connection: &mut Connection, _op_id: u32, _op: Operation) {
        // Leader should not receive Log messages.
        todo!(); // TODO: leader should not receive any Log messages.
    }

    async fn handle_ack(&mut self, _connection: &mut Connection, op_id: u32) {
        let Some((ack_count, _, _)) = self.operations.get_mut(&op_id) else {
            todo!(); // TODO: handle this case (unknown op_id).
        };

        *ack_count += 1;
        // Majority is computed over "replica" count = members - 1 (we are the leader).
        let replica_count = self.members.len().saturating_sub(1);
        if *ack_count <= replica_count / 2 {
            return;
        }

        // TODO: commit operation once majority is reached.
        todo!();
    }

    async fn handle_operation_result(
        &mut self,
        _op_id: u32,
        _operation: Operation,
        _success: bool,
        _error: Option<VerifyError>,
    ) {
        // This will be wired when the ActorRouter sends back OperationResult
        // events and the leader must translate them into NodeToStationMsg
        // for the station.
        todo!();
    }

    async fn handle_charge_request(
        &mut self,
        station: &mut Station,
        _pump_id: usize,
        account_id: u64,
        card_id: u64,
        amount: f32,
        request_id: u64,
    ) {
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
            if let Err(_e) = station.send(msg).await {
                // println!(
                //     "[Leader][station][OFFLINE][ERROR] failed to send fake-OK result: {e}",
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
        let op = Operation::Charge {
            account_id,
            card_id,
            amount,
            from_offline_station: false,
        };
        // Trigger the full execute flow in the actor system.
        self.router.do_send(RouterCmd::Execute {
            op_id: 0,
            operation: op,
        });
    }

    async fn handle_election(
        &mut self,
        connection: &mut Connection,
        candidate_id: u64,
        candidate_addr: SocketAddr,
    ) {
        // If our id is higher than the candidate, reply with ElectionOk.
        if self.id > candidate_id {
            let reply = Message::ElectionOk {
                responder_id: self.id,
            };

            let _ = connection.send(reply, &candidate_addr).await;
        }
    }

    async fn handle_election_ok(&mut self, _connection: &mut Connection, _responder_id: u64) {
        // Leader ignores election OKs.
    }

    async fn handle_coordinator(
        &mut self,
        _connection: &mut Connection,
        _leader_id: u64,
        _leader_addr: SocketAddr,
    ) {
        // Leader ignores coordinator announcements.
    }

    async fn start_election(&mut self, _connection: &mut Connection) {
        // Leader doesn't start elections.
    }

    /// Called when we receive a `Message::Join` through the generic
    /// Node dispatcher.
    async fn handle_join(&mut self, connection: &mut Connection, node_id: u64, addr: SocketAddr) {
        self.handle_join_message(connection, node_id, addr).await;
    }

    /// Called when we receive a `Message::ClusterView` through the
    /// generic Node dispatcher.
    async fn handle_cluster_view(&mut self, members: Vec<(u64, SocketAddr)>) {
        self.handle_cluster_view_message(members);
    }
}

impl Leader {
    /// Start a Leader node:
    ///
    /// - boots the ConnectionManager (TCP),
    /// - boots the ActorRouter in a dedicated Actix system thread,
    /// - starts the Station abstraction (which internally spawns the pump simulator),
    /// - seeds the cluster membership with self (other nodes will join dynamically),
    /// - then enters the main async event loop.
    pub async fn start(
        address: SocketAddr,
        coords: (f64, f64),
        max_conns: usize,
        pumps: usize,
    ) -> AppResult<()> {
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();
        // Spawn a thread for Actix that runs inside a Tokio runtime.
        std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(actor_tx).start();
                if router_tx.send(router.clone()).is_err() {
                    // println!("[ERROR] Failed to deliver ActorRouter addr to Leader");
                }

                pending::<()>().await; // Keep the Actix system running indefinitely.
            });
        });
        // Start the shared Station abstraction (stdin-based simulator).
        let station = Station::start(pumps).await?;
        // Seed membership: leader only. Other members will be
        // registered via Join / ClusterView messages.
        let self_id = get_id_given_addr(address);
        let mut members: HashMap<u64, SocketAddr> = HashMap::new();
        members.insert(self_id, address);
        let connection = Connection::start(address, max_conns).await?;
        let mut leader = Self {
            id: self_id,
            coords,
            address,
            max_conns,
            members,
            operations: HashMap::new(),
            is_offline: false,
            offline_queue: VecDeque::new(),
            station_requests: HashMap::new(),
            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive ActorRouter address: {e}"),
            })?,
        };

        leader.run(connection, actor_rx, station).await
    }

    /// Handle an incoming Join message:
    /// - register the new member in the membership map,
    /// - broadcast a ClusterView snapshot to all members (including the newcomer),
    ///   so everyone can converge to the same view.
    pub async fn handle_join_message(
        &mut self,
        connection: &mut Connection,
        node_id: u64,
        addr: SocketAddr,
    ) {
        self.members.insert(node_id, addr);
        let members_vec: Vec<(u64, SocketAddr)> =
            self.members.iter().map(|(id, addr)| (*id, *addr)).collect();
        let view_msg = Message::ClusterView {
            members: members_vec,
        };
        // Broadcast the updated view to all members.
        for peer_addr in self.members.values() {
            let _ = connection.send(view_msg.clone(), peer_addr).await;
        }
    }

    /// Handle an incoming ClusterView snapshot:
    /// - replace our current membership with the provided one,
    /// - but ensure we always include ourselves with our known address.
    pub fn handle_cluster_view_message(&mut self, members: Vec<(u64, SocketAddr)>) {
        self.members.clear();
        // Rebuild membership from snapshot.
        for (id, addr) in members {
            self.members.insert(id, addr);
        }

        // Ensure we are present with the correct address.
        self.members.insert(self.id, self.address);
    }
}

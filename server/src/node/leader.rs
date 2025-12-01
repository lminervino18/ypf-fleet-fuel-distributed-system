//! Leader node.
//!
//! The Leader wires together:
//! - the TCP ConnectionManager (network),
//! - the local "database" (actor system wrapped in `Database`),
//! - the Station abstraction (stdin-driven pump simulator).
//!
//! ONLINE behavior (default):
//! For a `Charge` coming from a pump the flow is:
//! 1. Station → Leader: `StationToNodeMsg::ChargeRequest`.
//! 2. Leader builds `Operation::Charge { .., from_offline_station: false }`
//!    and sends a high-level Execute command to the actor system.
//! 3. ActorRouter/actors run verification + apply internally.
//! 4. Once finished, ActorRouter emits a single
//!    `ActorEvent::OperationResult { op_id, operation, result, .. }`.
//! 5. Leader translates that `OperationResult` into:
//!    - `NodeToStationMsg::ChargeResult` if it originated from the station,
//!    - `Message::Response { op_result: OperationResult }` if it was a TCP client.
//!
//! OFFLINE behavior:
//! - Station → Leader: `StationToNodeMsg::DisconnectNode`
//!   sets `is_offline = true`.
//! - When `is_offline == true`:
//!   * network events from the cluster are ignored (only drained),
//!   * pump-originated `ChargeRequest`s are:
//!       - enqueued into `offline_queue`,
//!       - immediately acknowledged to the station as `allowed = true`,
//!       - **not** sent to the actor system yet.
//! - Station → Leader: `StationToNodeMsg::ConnectNode`
//!   sets `is_offline = false` and replays all queued charges to
//!   the actor system with `from_offline_station = true`.
use super::database::{Database, DatabaseCmd}; // use the Database abstraction
use super::node::{run_node_runtime, Node, NodeRuntime, RoleChange};
use super::pending_operation::PendingOperation;
use super::replica::Replica;
use super::utils::get_id_given_addr;
use crate::errors::AppResult;
use common::operation::{DatabaseSnapshot, Operation};
use common::operation_result::{ChargeResult, OperationResult};
use common::{Connection, Message, NodeToStationMsg, Station};
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    time::{Duration, Instant},
};

/// Leader node implementation.
pub struct Leader {
    id: u64,
    current_op_id: u32,
    coords: (f64, f64),
    address: SocketAddr,
    cluster: HashMap<u64, SocketAddr>,
    operations: HashMap<u32, PendingOperation>,
    is_offline: bool,
    offline_queue: VecDeque<Operation>,

    // Election state for Bully algorithm (same concept as in Replica)
    election_in_progress: bool,
    election_start: Option<Instant>,
}

// ==========================================================
// Node trait implementation
// ==========================================================
impl Node for Leader {
    fn is_offline(&self) -> bool {
        self.is_offline
    }

    async fn get_status(&self) -> String {
        // Return cluster size and leader address
        format!(
            "Cluster: {:?}, Leader: {:?}",
            self.cluster.len(),
            self.address
        )
    }

    fn log_offline_operation(&mut self, op: Operation) {
        self.offline_queue.push_back(op);
    }

    fn get_address(&self) -> SocketAddr {
        self.address
    }

    async fn handle_disconnect_node(&mut self, connection: &mut Connection) {
        self.is_offline = true;
        connection.disconnect().await;
    }

    async fn handle_connect_node(&mut self, connection: &mut Connection) -> AppResult<()> {
        self.is_offline = false;
        connection.reconnect().await?;
        for node_addr in self.cluster.values() {
            if node_addr == &self.address {
                continue;
            }

            match connection
                .send(Message::Join { addr: self.address }, node_addr)
                .await
            {
                Ok(_) => {
                    break;
                }
                Err(e) => {
                    println!("[LEADER] could not send join msg to {node_addr} because of {e}");
                    // Retry once to avoid modifying connection lost handling here.
                    let _ = connection
                        .send(Message::Join { addr: self.address }, node_addr)
                        .await;
                }
            }
        }

        Ok(())
    }

    async fn anounce_coordinator(&mut self, connection: &mut Connection) -> AppResult<RoleChange> {
        let msg = Message::Coordinator {
            leader_id: self.id,
            leader_addr: self.address,
        };

        for (peer_id, peer_addr) in &self.cluster {
            if *peer_id == self.id {
                continue;
            }
            if let Err(e) = connection.send(msg.clone(), peer_addr).await {
                println!(
                    "[LEADER {}] Failed to send Coordinator to {:?}: {:?}",
                    self.id, peer_addr, e
                );
            }
        }

        // Already leader; no role change.
        Ok(RoleChange::None)
    }

    async fn handle_connection_lost_with(
        &mut self,
        _connection: &mut Connection,
        address: SocketAddr,
    ) -> AppResult<RoleChange> {
        let Some(_removed) = self.cluster.remove(&get_id_given_addr(address)) else {
            return Ok(RoleChange::None);
        };

        Ok(RoleChange::None)
    }

    async fn handle_request(
        &mut self,
        connection: &mut Connection,
        db: &mut Database,
        req_id: u32,
        op: Operation,
        client_addr: SocketAddr,
    ) -> AppResult<()> {
        // Store the operation locally.
        self.operations.insert(
            self.current_op_id,
            PendingOperation::new(op.clone(), client_addr, req_id),
        );

        if self.cluster.len() == 1 {
            db.send(DatabaseCmd::Execute {
                op_id: self.current_op_id,
                operation: op.clone(),
            });
            return Ok(());
        }
        for (node_id, addr) in &self.cluster {
            if *node_id == self.id {
                continue;
            }

            let msg = Message::Log {
                op_id: self.current_op_id,
                op: op.clone(),
            };
            connection.send(msg, addr).await?;
        }

        self.current_op_id += 1;

        Ok(())
    }

    async fn handle_response(
        &mut self,
        _connection: &mut Connection,
        _station: &mut Station,
        _req_id: u32,
        _op_result: OperationResult,
    ) -> AppResult<()> {
        todo!()
    }

    async fn handle_role_query(
        &mut self,
        connection: &mut Connection,
        addr: SocketAddr,
    ) -> AppResult<()> {
        let role_msg = Message::RoleResponse {
            node_id: get_id_given_addr(self.address),
            role: common::NodeRole::Leader,
        };
        connection.send(role_msg, &addr).await?;
        Ok(())
    }

    async fn handle_log(
        &mut self,
        _connection: &mut Connection,
        _db: &mut Database,
        _op_id: u32,
        _new_op: Operation,
    ) -> AppResult<()> {
        todo!();
    }

    // CHANGED SIGNATURE: now receives `db: &mut Database`
    async fn handle_ack(&mut self, _connection: &mut Connection, db: &mut Database, op_id: u32) {
        let Some(pending) = self.operations.get_mut(&op_id) else {
            return; // TODO: handle this case (unknown op_id).
        };

        pending.ack_count += 1;
        if pending.ack_count != ((self.cluster.len() as f32 - 1f32) / 2f32).round() as usize
            && self.cluster.len() - 1 != 1
        {
            return;
        }

        // Majority reached → execute the operation in the actor world.
        db.send(DatabaseCmd::Execute {
            op_id,
            operation: pending.op.clone(),
        });
        // The async response arrives via ActorEvent::OperationResult.
    }

    async fn handle_operation_result(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        _database: &mut Database,
        op_id: u32,
        operation: Operation,
        result: OperationResult,
    ) -> AppResult<()> {
        let pending = if op_id != 0 {
            self.operations
                .remove(&op_id)
                .expect("leader received the result of an unexisting operation")
        } else {
            PendingOperation {
                op: operation.clone(),
                client_addr: self.address,
                request_id: 0,
                ack_count: 0,
            }
        };

        match operation {
            Operation::GetDatabase { addr } => {
                // This OperationResult comes from the ActorRouter as DatabaseSnapshot.
                if let OperationResult::DatabaseSnapshot(snapshot) = result {
                    let msg = Message::ClusterView {
                        members: self.cluster.clone().into_iter().collect(),
                        leader_addr: self.address,
                        database: snapshot.clone(),
                    };
                    if connection.send(msg.clone(), &addr).await.is_err() {
                        connection.send(msg, &addr).await
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }

            Operation::ReplaceDatabase { .. } => Ok(()),

            // ======================================
            // OTHER OPERATIONS (legacy behavior)
            // ======================================
            op => {
                if pending.client_addr == self.address {
                    // Local station case: only Charge matters for replying.
                    if let Operation::Charge { .. } = op {
                        if let OperationResult::Charge(charge_res) = result {
                            let (allowed, error) = match charge_res {
                                ChargeResult::Ok => (true, None),
                                ChargeResult::Failed(e) => (false, Some(e)),
                            };

                            let msg = NodeToStationMsg::ChargeResult {
                                request_id: pending.request_id,
                                allowed,
                                error,
                            };

                            station.send(msg).await?;
                        }
                    }

                    return Ok(());
                }

                // External client: return the full OperationResult.
                connection
                    .send(
                        Message::Response {
                            req_id: pending.request_id,
                            op_result: result,
                        },
                        &pending.client_addr,
                    )
                    .await
            }
        }
    }

    async fn handle_election(
        &mut self,
        connection: &mut Connection,
        candidate_id: u64,
        candidate_addr: SocketAddr,
    ) -> AppResult<RoleChange> {
        if self.id > candidate_id {
            let reply = Message::ElectionOk {
                responder_id: self.id,
            };
            if let Err(e) = connection.send(reply, &candidate_addr).await {
                println!(
                    "[LEADER {}] Failed to send ElectionOk to {:?}: {:?}",
                    self.id, candidate_addr, e
                );
            }

            // As in a replica, start our own election (there may be a higher ID).
            self.start_election(connection).await
        } else {
            Ok(RoleChange::None)
        }
    }

    async fn handle_election_ok(&mut self, _connection: &mut Connection, _: u64) {
        // We know a larger-process is alive; wait for Coordinator.
        self.election_in_progress = false;
        self.election_start = None;
    }

    async fn handle_coordinator(
        &mut self,
        _connection: &mut Connection,
        leader_id: u64,
        leader_addr: SocketAddr,
    ) -> AppResult<RoleChange> {
        // Reset election state (there is a coordinator)
        self.election_in_progress = false;
        self.election_start = None;

        // If someone with ID >= mine announces leadership, demote to replica.
        if leader_id >= self.id {
            return Ok(RoleChange::DemoteToReplica {
                new_leader_addr: leader_addr,
            });
        }
        Ok(RoleChange::None)
    }

    /// Same logic as in Replica, but if there are no higher IDs and we win,
    /// simply reaffirm leadership (no RoleChange).
    async fn start_election(&mut self, connection: &mut Connection) -> AppResult<RoleChange> {
        if self.election_in_progress {
            return Ok(RoleChange::None);
        }

        let mut sent_any = false;

        for (&node_id, addr) in &self.cluster {
            if node_id > self.id {
                sent_any = true;

                if let Err(e) = connection
                    .send(
                        Message::Election {
                            candidate_id: self.id,
                            candidate_addr: self.address,
                        },
                        addr,
                    )
                    .await
                {
                    println!(
                        "[LEADER {}] Failed to send Election to {:?}: {:?}",
                        self.id, addr, e
                    );
                }
            }
        }

        if sent_any {
            self.election_in_progress = true;
            self.election_start = Some(Instant::now());
            Ok(RoleChange::None)
        } else {
            self.election_in_progress = false;
            self.election_start = None;
            // Reaffirm leadership; no RoleChange.
            self.anounce_coordinator(connection).await
        }
    }

    /// Called when we receive a `Message::Join` through the generic Node dispatcher.
    async fn handle_join(
        &mut self,
        connection: &mut Connection,
        database: &mut Database,
        addr: SocketAddr,
    ) -> AppResult<()> {
        database.send(DatabaseCmd::Execute {
            op_id: 0,
            operation: Operation::GetDatabase { addr },
        });

        let node_id = get_id_given_addr(addr);
        self.cluster.insert(node_id, addr);

        // Send the update to the replicas
        for replica_addr in self.cluster.values() {
            if replica_addr == &self.address || replica_addr == &addr {
                continue;
            }

            connection
                .send(
                    Message::ClusterUpdate {
                        new_member: (node_id, addr),
                    },
                    replica_addr,
                )
                .await?;
        }
        // Note: the new node runs Bully from Replica::start.
        Ok(())
    }

    async fn handle_cluster_update(
        &mut self,
        _connection: &mut Connection,
        _new_member: (u64, SocketAddr),
    ) {
        // only leaders send cluster updates
    }

    async fn handle_cluster_view(
        &mut self,
        connection: &mut Connection,
        database: &mut Database,
        members: Vec<(u64, SocketAddr)>,
        _leader_addr: SocketAddr,
        snapshot: DatabaseSnapshot,
    ) -> AppResult<()> {
        database.send(DatabaseCmd::Execute {
            op_id: 0,
            operation: Operation::ReplaceDatabase { snapshot },
        });

        self.cluster.clear();
        for (id, addr) in members {
            self.cluster.insert(id, addr);
        }

        println!(
            "[LEADER {}] I Joined the cluster of {:?} members",
            self.id,
            self.cluster.len()
        );
        
        self.start_election(connection).await?;

        Ok(())
    }

    /// Election timeout logic for Leader: if no ElectionOk arrives within 2s
    /// and the election was active, reaffirm leadership.
    async fn handle_election_timeout(
        &mut self,
        connection: &mut Connection,
    ) -> AppResult<RoleChange> {
        if !self.election_in_progress {
            return Ok(RoleChange::None);
        }

        let Some(start) = self.election_start else {
            return Ok(RoleChange::None);
        };

        if start.elapsed() >= Duration::from_secs(2) {
            self.election_in_progress = false;
            self.election_start = None;
            self.anounce_coordinator(connection).await?;
        }

        Ok(RoleChange::None)
    }
}

impl Leader {
    /// Convert this Leader into a Replica with the given new leader address.
    /// Consumes self and returns a Replica.
    pub fn into_replica(self, new_leader_addr: SocketAddr) -> Replica {
        // Discard pending operations; replica does not need them.
        let operations: HashMap<u32, Operation> = HashMap::new();

        Replica::from_existing(
            self.id,
            self.coords,
            self.address,
            new_leader_addr,
            self.cluster,
            operations,
            self.is_offline,
            self.offline_queue,
        )
    }

    /// Create a new Leader from existing node state (used for Replica promotion)
    pub fn from_existing(
        id: u64,
        current_op_id: u32,
        coords: (f64, f64),
        address: SocketAddr,
        members: HashMap<u64, SocketAddr>,
        _operations_from_replica: HashMap<u32, Operation>,
        is_offline: bool,
        offline_queue: VecDeque<Operation>,
    ) -> Self {
        let operations = HashMap::new();
        // Old committed ops are already present in the actor system.

        Self {
            id,
            current_op_id,
            coords,
            address,
            cluster: members,
            operations,
            is_offline,
            offline_queue,
            election_in_progress: false,
            election_start: None,
        }
    }

    /// Start a Leader node:
    ///
    /// - boots the ConnectionManager (TCP),
    /// - boots the actor system wrapped in `Database` (Actix in a dedicated thread),
    /// - starts the Station abstraction (pump simulator),
    /// - seeds the cluster membership with self (other nodes will join dynamically),
    /// - and delegates the main loop to `run_node_runtime`, which handles role changes
    ///   reusing Connection/Database/Station by reference.
    pub async fn start(
        address: SocketAddr,
        coords: (f64, f64),
        max_conns: usize,
        pumps: usize,
    ) -> AppResult<()> {
        // Start the actor-based "database" subsystem (ActorRouter + Actix system).
        let mut db = super::database::Database::start().await?;

        // Start the shared Station abstraction (stdin-based simulator).
        let mut station = Station::start(pumps).await?;

        // Start the TCP ConnectionManager for this node.
        let mut connection = Connection::start(address, max_conns).await?;

        // Seed membership: leader only. Other members will be
        // registered via Join / ClusterView messages.
        let self_id = get_id_given_addr(address);
        let mut members: HashMap<u64, SocketAddr> = HashMap::new();
        members.insert(self_id, address);

        let leader = Self {
            id: self_id,
            current_op_id: 1,
            coords,
            address,
            cluster: members,
            operations: HashMap::new(),
            is_offline: false,
            offline_queue: VecDeque::new(),
            election_in_progress: false,
            election_start: None,
        };

        let runtime = NodeRuntime::Leader(leader);
        run_node_runtime(runtime, &mut connection, &mut db, &mut station).await
    }
}

  

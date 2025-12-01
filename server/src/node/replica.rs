//! Replica node implementation and logic.
//!
//! The Replica receives logs from the leader (`Message::Log`), stores them,
//! commits operations against the actor-based database (via `Database`), and
//! acknowledges the leader when a committed operation completes.
//!
//! For requests originating from the local station:
//! - If ONLINE, proxy the request to the leader with `Message::Request`.
//! - If OFFLINE, enqueue the operation in `offline_queue` and reply OK to the station.

use super::{
    database::{Database, DatabaseCmd},
    leader::Leader,
    node::{run_node_runtime, Node, NodeRuntime, RoleChange},
    utils::get_id_given_addr,
};
use crate::errors::AppResult;
use common::{
    operation::{DatabaseSnapshot, Operation},
    operation_result::{ChargeResult, OperationResult},
    Connection, Message, NodeToStationMsg, Station,
};
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    time::{Duration, Instant},
};

/// Internal pending charge from the station (unused placeholder).
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct StationPendingCharge {
    pump_id: u32,
    account_id: u64,
    card_id: u64,
    amount: f32,
}

/// Internal representation of an offline queued charge (unused placeholder).
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct OfflineQueuedCharge {
    request_id: u64,
    account_id: u64,
    card_id: u64,
    amount: f32,
}

/// Replica role state.
pub struct Replica {
    id: u64,
    coords: (f64, f64),
    address: SocketAddr,
    leader_addr: SocketAddr,
    cluster: HashMap<u64, SocketAddr>,
    operations: HashMap<u32, Operation>,
    is_offline: bool,
    offline_queue: VecDeque<Operation>,
    #[allow(dead_code)]
    start_time: Instant,

    // Bully election state
    election_in_progress: bool,
    election_start: Option<Instant>,
}

impl Node for Replica {
    fn is_offline(&self) -> bool {
        self.is_offline
    }

    async fn get_status(&self) -> String {
        // Return cluster size and leader address.
        format!(
            "Cluster: {:?}, Leader: {:?}",
            self.cluster.len(),
            self.leader_addr
        )
    }

    fn log_offline_operation(&mut self, op: Operation) {
        self.offline_queue.push_back(op);
    }

    fn get_address(&self) -> SocketAddr {
        self.address
    }

    async fn handle_connection_lost_with(
        &mut self,
        connection: &mut Connection,
        lost_address: SocketAddr,
    ) -> AppResult<RoleChange> {
        let lost_id = get_id_given_addr(lost_address);

        if self.cluster.contains_key(&lost_id) {
            self.cluster.retain(|_, &mut addr| addr != lost_address);
        }

        // If the leader was lost, start an election.
        if lost_address == self.leader_addr {
            return self.start_election(connection).await;
        }

        Ok(RoleChange::None)
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

            if connection
                .send(Message::Join { addr: self.address }, node_addr)
                .await
                .is_ok()
            {
                break;
            } else {
                connection
                    .send(Message::Join { addr: self.address }, node_addr)
                    .await?;
            }
        }

        Ok(())
    }

    async fn anounce_coordinator(&mut self, connection: &mut Connection) -> AppResult<RoleChange> {
        // Broadcast Coordinator announcement to all peers.
        for (node, addr) in &self.cluster {
            if *node == self.id {
                continue; // Skip announcing to ourselves
            }

            if let Err(e) = connection
                .send(
                    Message::Coordinator {
                        leader_id: self.id,
                        leader_addr: self.address,
                    },
                    addr,
                )
                .await
            {
                println!(
                    "[REPLICA {}] Failed to send Coordinator to {:?}: {:?}",
                    self.id, addr, e
                );
            }
        }

        // Handle our own promotion to leader if applicable.
        self.handle_coordinator(connection, self.id, self.address)
            .await
    }

    /// Start a Bully election:
    /// - If there are higher IDs, send Election and start a timeout.
    /// - If there are no higher IDs, immediately self-promote to leader.
    async fn start_election(&mut self, connection: &mut Connection) -> AppResult<RoleChange> {
        if self.election_in_progress {
            return Ok(RoleChange::None);
        }
        let mut sent_any = false;

        for (&node_id, replica_addr) in &self.cluster {
            if node_id > self.id {
                sent_any = true;
                if let Err(e) = connection
                    .send(
                        Message::Election {
                            candidate_id: self.id,
                            candidate_addr: self.address,
                        },
                        replica_addr,
                    )
                    .await
                {
                    println!(
                        "[REPLICA {}] Failed to send Election to {:?}: {:?}",
                        self.id, replica_addr, e
                    );
                }
            }
        }

        if sent_any {
            // At least one node has a higher ID: election with timeout.
            self.election_in_progress = true;
            self.election_start = Some(Instant::now());
            Ok(RoleChange::None)
        } else {
            // No higher IDs → promote to leader immediately.
            self.election_in_progress = false;
            self.election_start = None;

            // Broadcast Coordinator and return promotion result.
            self.anounce_coordinator(connection).await
        }
    }

    async fn handle_operation_result(
        &mut self,
        connection: &mut Connection,
        _station: &mut Station,
        database: &mut Database,
        op_id: u32,
        operation: Operation,
        _result: OperationResult,
    ) -> AppResult<()> {
        // If this operation was ReplaceDatabase, do not ACK the leader.
        if let Operation::ReplaceDatabase { .. } = operation {
            while let Some(op) = self.offline_queue.pop_front() {
                self.handle_request(connection, database, 0, op, self.address)
                    .await?
            }
            return Ok(());
        }

        // For normal operations, send an Ack to the leader when done.
        connection
            .send(Message::Ack { op_id: op_id + 1 }, &self.leader_addr)
            .await
    }

    async fn handle_request(
        &mut self,
        connection: &mut Connection,
        _database: &mut Database,
        op_id: u32,
        op: Operation,
        addr: SocketAddr,
    ) -> AppResult<()> {
        // Proxy the request to the leader.
        connection
            .send(
                Message::Request {
                    req_id: op_id,
                    addr,
                    op,
                },
                &self.leader_addr,
            )
            .await?;

        Ok(())
    }

    async fn handle_response(
        &mut self,
        _connection: &mut Connection,
        station: &mut Station,
        req_id: u32,
        op_result: OperationResult,
    ) -> AppResult<()> {
        if req_id == 0 {
            return Ok(());
        }
        match op_result {
            OperationResult::Charge(cr) => {
                let (allowed, error) = match cr {
                    ChargeResult::Ok => (true, None),
                    ChargeResult::Failed(err) => (false, Some(err)),
                };

                station
                    .send(NodeToStationMsg::ChargeResult {
                        request_id: req_id,
                        allowed,
                        error,
                    })
                    .await?;
            }

            other => {
                let _ = other;
            }
        }

        Ok(())
    }

    async fn handle_role_query(
        &mut self,
        connection: &mut Connection,
        addr: SocketAddr,
    ) -> AppResult<()> {
        let role_msg = Message::RoleResponse {
            node_id: get_id_given_addr(self.address),
            role: common::NodeRole::Replica,
        };
        connection.send(role_msg, &addr).await?;
        Ok(())
    }

    async fn handle_log(
        &mut self,
        connection: &mut Connection,
        db: &mut Database,
        op_id: u32,
        new_op: Operation,
    ) -> AppResult<()> {
        // Store the log and commit the previous operation.
        self.operations.insert(op_id, new_op);

        if op_id == 1 {
            return connection
                .send(Message::Ack { op_id: 1 }, &self.leader_addr)
                .await;
        }

        // Commit operation (op_id - 1) via the Database abstraction.
        self.commit_operation(db, op_id - 1).await
    }

    async fn handle_ack(&mut self, _connection: &mut Connection, _db: &mut Database, _id: u32) {
        // Replicas should not receive ACKs in normal flow.
        todo!();
    }

    async fn handle_election(
        &mut self,
        connection: &mut Connection,
        candidate_id: u64,
        candidate_addr: SocketAddr,
    ) -> AppResult<RoleChange> {
        if self.id > candidate_id {
            if let Err(e) = connection
                .send(
                    Message::ElectionOk {
                        responder_id: self.id,
                    },
                    &candidate_addr,
                )
                .await
            {
                println!(
                    "[REPLICA {}] Failed to send ElectionOk to {:?}: {:?}",
                    self.id, candidate_addr, e
                );
            }

            // If we are larger, start our own election.
            self.start_election(connection).await
        } else {
            Ok(RoleChange::None)
        }
    }

    async fn handle_election_ok(&mut self, _connection: &mut Connection, _responder_id: u64) {
        // A larger node responded: stop our timeout-driven self-promotion.
        self.election_in_progress = false;
        self.election_start = None;
    }

    async fn handle_coordinator(
        &mut self,
        _connection: &mut Connection,
        leader_id: u64,
        leader_addr: SocketAddr,
    ) -> AppResult<RoleChange> {
        // Reset election state (we have a coordinator).
        self.election_in_progress = false;
        self.election_start = None;

        // If the announcement is from ourselves, promote.
        if leader_id == self.id {
            return Ok(RoleChange::PromoteToLeader);
        }

        if leader_id < self.id {
            panic!("leader_id should not be less than self");
        }

        // Update the current leader pointer.
        self.leader_addr = leader_addr;
        println!(
            "[REPLICA {}] New Leader elected: {}",
            self.id, leader_id
        );
        Ok(RoleChange::None)
    }

    async fn handle_join(
        &mut self,
        connection: &mut Connection,
        _database: &mut Database,
        addr: SocketAddr,
    ) -> AppResult<()> {
        connection
            .send(Message::Join { addr }, &self.leader_addr)
            .await
    }

    async fn handle_cluster_update(
        &mut self,
        _connection: &mut Connection,
        new_member: (u64, SocketAddr),
    ) {
        println!("[REPLICA {}] New member joined: {:?}", self.id, new_member.0);
        self.cluster.insert(new_member.0, new_member.1);
    }

    async fn handle_cluster_view(
        &mut self,
        connection: &mut Connection,
        database: &mut Database,
        members: Vec<(u64, SocketAddr)>,
        leader_addr: SocketAddr,
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

        self.leader_addr = leader_addr;

        println!(
            "[REPLICA {}] I Joined the cluster of {:?} member",
            self.id,
            self.cluster.len()
        );

        if get_id_given_addr(leader_addr) > self.id {
            self.start_election(connection).await?;
        }

        Ok(())
    }

    /// Election timeout handler:
    /// If enough time passes without ElectionOk responses, self-promote (Bully).
    async fn handle_election_timeout(
        &mut self,
        connection: &mut Connection,
    ) -> AppResult<RoleChange> {
        if !self.election_in_progress {
            // No election in progress.
            return Ok(RoleChange::None);
        }

        let Some(start) = self.election_start else {
            return Ok(RoleChange::None);
        };

        // If more than 2 seconds passed without ElectionOk → self-promote.
        if start.elapsed() >= Duration::from_secs(2) {
            self.election_in_progress = false;
            self.election_start = None;

            // Broadcast Coordinator and handle promotion.
            return self.anounce_coordinator(connection).await;
        }

        Ok(RoleChange::None)
    }
}

impl Replica {
    /// Convert this Replica into a Leader.
    pub fn into_leader(self) -> Leader {
        // Use the highest known op_id as the current op id when promoting.
        let current_op_id = self.operations.keys().max().copied().unwrap_or(1);

        Leader::from_existing(
            self.id,
            current_op_id,
            self.coords,
            self.address,
            self.cluster,
            self.operations,
            self.is_offline,
            self.offline_queue,
        )
    }

    /// Create a Replica from existing state (used when demoting a Leader).
    pub fn from_existing(
        id: u64,
        coords: (f64, f64),
        address: SocketAddr,
        leader_addr: SocketAddr,
        members: HashMap<u64, SocketAddr>,
        operations: HashMap<u32, Operation>,
        is_offline: bool,
        offline_queue: VecDeque<Operation>,
    ) -> Self {
        Self {
            id,
            coords,
            address,
            leader_addr,
            cluster: members,
            operations,
            is_offline,
            offline_queue,
            start_time: Instant::now(),
            election_in_progress: false,
            election_start: None,
        }
    }

    async fn commit_operation(&mut self, db: &mut Database, op_id: u32) -> AppResult<()> {
        let Some(op) = self.operations.remove(&op_id) else {
            return Ok(());
        };

        // Fire-and-forget to the actor world via Database.
        db.send(DatabaseCmd::Execute {
            op_id,
            operation: op,
        });

        Ok(())
    }

    /// Start a Replica node and run its main loop.
    ///
    /// Steps:
    /// - Boot the actor-based Database (Actix in dedicated thread).
    /// - Start the Station simulator.
    /// - Start the network ConnectionManager.
    /// - Seed membership with leader and send an initial Join to the leader.
    /// - Enter the generic runtime `run_node_runtime`.
    pub async fn start(
        address: SocketAddr,
        leader_addr: SocketAddr,
        coords: (f64, f64),
        max_conns: usize,
        pumps: usize,
    ) -> AppResult<()> {
        // Start the actor-based "database" subsystem.
        let mut db = super::database::Database::start().await?;

        // Start the Station simulator.
        let mut station = Station::start(pumps).await?;

        // Start network connection.
        let mut connection = Connection::start(address, max_conns).await?;

        // Seed membership with the leader.
        let id = get_id_given_addr(address);

        let mut members: HashMap<u64, SocketAddr> = HashMap::new();

        let leader_id = get_id_given_addr(leader_addr);
        members.insert(leader_id, leader_addr);

        let replica = Self {
            id,
            coords,
            address,
            leader_addr,
            cluster: members,
            operations: HashMap::new(),
            is_offline: false,
            offline_queue: VecDeque::new(),
            start_time: Instant::now(),
            election_in_progress: false,
            election_start: None,
        };

        // Send an initial Join to the leader to obtain ClusterView.
        let _ = connection
            .send(
                Message::Join {
                    addr: replica.address,
                },
                &replica.leader_addr,
            )
            .await;

        let runtime = NodeRuntime::Replica(replica);
        run_node_runtime(runtime, &mut connection, &mut db, &mut station).await
    }
}
 

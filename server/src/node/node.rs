//! Node runtime and trait definitions.
//!
//! This module defines the `Node` trait implemented by concrete roles
//! (`Leader`, `Replica`) and provides the generic runtime that drives a node.
//!
//! The node coordinates three subsystems:
//! - the network layer (`Connection`) for inter-node and client communication,
//! - the actor-based logical database (`Database`) for business logic,
//! - the local station simulator (`Station`) for pump interactions.
//!
//! The core event loop (in `Node::run`) multiplexes:
//! - messages from other nodes (`Message`),
//! - messages from the local station (`StationToNodeMsg`),
//! - actor events produced by `Database` (`ActorEvent`).
//!
//! Role transitions (promote/demote) are signalled via `RoleChange` and handled
//! by `run_node_runtime`, which reuses the same `Connection` / `Database` / `Station`
//! while swapping role state objects.

use super::actors::ActorEvent;
use super::database::Database;
use crate::errors::AppResult;
use crate::node::utils::get_id_given_addr;
use common::operation::{DatabaseSnapshot, Operation};
use common::operation_result::OperationResult;
use common::{AppError, Connection, Message, NodeToStationMsg, Station, StationToNodeMsg};
use std::net::SocketAddr;
use tokio::select;

// Runtime role types
use super::{leader::Leader, replica::Replica};

/// Signal to indicate whether a role change should occur.
#[derive(Debug, Clone)]
pub enum RoleChange {
    None,
    PromoteToLeader,
    DemoteToReplica { new_leader_addr: SocketAddr },
}

/// Node behaviour abstraction.
///
/// Implement this trait for different roles (Leader, Replica). The trait
/// encapsulates handlers for incoming messages, station events and actor events.
pub trait Node {
    #[allow(dead_code)]
    async fn get_status(&self) -> String;

    // -----------------------
    // Messages coming from the network (Connection)
    // -----------------------
    async fn handle_request(
        &mut self,
        connection: &mut Connection,
        database: &mut Database,
        req_id: u32,
        op: Operation,
        client_addr: SocketAddr,
    ) -> AppResult<()>;

    async fn handle_response(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        req_id: u32,
        op_result: OperationResult,
    ) -> AppResult<()>;

    async fn handle_role_query(
        &mut self,
        connection: &mut Connection,
        addr: SocketAddr,
    ) -> AppResult<()>;

    /// Handle a replicated log entry.
    ///
    /// `db` is provided so implementations (e.g., Replica) can commit the
    /// operation into the actor-based "database" without taking ownership.
    async fn handle_log(
        &mut self,
        connection: &mut Connection,
        db: &mut Database,
        op_id: u32,
        new_op: Operation,
    ) -> AppResult<()>;

    async fn handle_ack(&mut self, connection: &mut Connection, db: &mut Database, op_id: u32);

    /// Handle the result of an operation that returned from the actor world.
    async fn handle_operation_result(
        &mut self,
        connection: &mut Connection,
        _station: &mut Station,
        database: &mut Database,
        op_id: u32,
        _operation: Operation,
        _result: OperationResult,
    ) -> AppResult<()>;

    /// Default dispatcher for actor events.
    ///
    /// Extracts `OperationResult` from `ActorEvent` and forwards it to
    /// `handle_operation_result`.
    async fn handle_actor_event(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        database: &mut Database,
        event: ActorEvent,
    ) {
        match event {
            ActorEvent::OperationResult {
                op_id,
                operation,
                result,
                ..
            } => {
                if let Err(e) = self
                    .handle_operation_result(
                        connection, station, database, op_id, operation, result,
                    )
                    .await
                {
                    eprintln!("[NODE] error handling actor event: {e:?}");
                }
            }
            ActorEvent::Debug(msg) => {
                let _ = msg;
            }
        }
    }

    fn is_offline(&self) -> bool;
    fn log_offline_operation(&mut self, op: Operation);
    fn get_address(&self) -> SocketAddr;

    /// Handle a charge request originating from the local station (pump).
    ///
    /// Default behaviour:
    /// - If the node is offline, enqueue the operation and immediately
    ///   acknowledge the station (allowed = true).
    /// - Otherwise build a `Charge` operation and forward it to `handle_request`.
    async fn handle_charge_request(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        database: &mut Database,
        account_id: u64,
        card_id: u64,
        amount: f32,
        request_id: u32,
    ) -> AppResult<RoleChange> {
        if self.is_offline() {
            self.log_offline_operation(Operation::Charge {
                account_id,
                card_id,
                amount,
                from_offline_station: true,
            });
            let msg = NodeToStationMsg::ChargeResult {
                request_id,
                allowed: true,
                error: None,
            };
            let _ = station.send(msg).await;
            return Ok(RoleChange::None);
        }

        let op = Operation::Charge {
            account_id,
            card_id,
            amount,
            from_offline_station: false,
        };

        match self
            .handle_request(connection, database, request_id, op, self.get_address())
            .await
        {
            Ok(()) => Ok(RoleChange::None),
            Err(e) => Err(e)?,
        }
    }

    async fn handle_connection_lost_with(
        &mut self,
        connection: &mut Connection,
        address: SocketAddr,
    ) -> AppResult<RoleChange>;

    /// Handle transition to OFFLINE mode.
    ///
    /// Concrete roles (Leader / Replica) override this to:
    /// - flip internal offline flags,
    /// - emit diagnostics to the station,
    /// - adapt cluster behaviour as needed.
    async fn handle_disconnect_node(&mut self, connection: &mut Connection);

    /// Handle transition back to ONLINE mode.
    ///
    /// Concrete roles override this to:
    /// - flip internal offline flags,
    /// - replay queued offline operations into the actor layer,
    /// - notify the station with debug messages.
    async fn handle_connect_node(&mut self, connection: &mut Connection) -> AppResult<()>;

    /// Default handler for station-originated messages.
    ///
    /// Dispatches:
    /// - `ChargeRequest` → `handle_charge_request`
    /// - `DisconnectNode` → `handle_disconnect_node`
    /// - `ConnectNode` → `handle_connect_node`
    async fn handle_station_msg(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        database: &mut Database,
        msg: StationToNodeMsg,
    ) -> AppResult<RoleChange> {
        match msg {
            StationToNodeMsg::ChargeRequest {
                account_id,
                card_id,
                amount,
                request_id,
            } => {
                self.handle_charge_request(
                    connection, station, database, account_id, card_id, amount, request_id,
                )
                .await
            }
            StationToNodeMsg::DisconnectNode => {
                self.handle_disconnect_node(connection).await;
                Ok(RoleChange::None)
            }
            StationToNodeMsg::ConnectNode => {
                self.handle_connect_node(connection).await?;
                Ok(RoleChange::None)
            }
        }
    }

    // === Bully / leader election hooks ===
    async fn handle_election(
        &mut self,
        connection: &mut Connection,
        candidate_id: u64,
        candidate_addr: SocketAddr,
    ) -> AppResult<RoleChange>;

    async fn handle_election_ok(&mut self, connection: &mut Connection, responder_id: u64);

    async fn handle_coordinator(
        &mut self,
        connection: &mut Connection,
        leader_id: u64,
        leader_addr: SocketAddr,
    ) -> AppResult<RoleChange>;

    async fn anounce_coordinator(&mut self, connection: &mut Connection) -> AppResult<RoleChange>;

    /// Start a Bully election.
    ///
    /// Returns:
    /// - `RoleChange::None` if the election is started/ongoing,
    /// - `RoleChange::PromoteToLeader` if this node should promote itself,
    /// - `RoleChange::None` if already leader and reaffirming leadership.
    async fn start_election(&mut self, connection: &mut Connection) -> AppResult<RoleChange>;

    // === Cluster membership hooks (Join / ClusterView) ===
    async fn handle_join(
        &mut self,
        connection: &mut Connection,
        database: &mut Database,
        addr: SocketAddr,
    ) -> AppResult<()>;

    async fn handle_cluster_view(
        &mut self,
        connection: &mut Connection,
        database: &mut Database,
        members: Vec<(u64, SocketAddr)>,
        leader_addr: SocketAddr,
        snapshot: DatabaseSnapshot,
    ) -> AppResult<()>;

    async fn handle_cluster_update(
        &mut self,
        connection: &mut Connection,
        new_member: (u64, SocketAddr),
    );

    /// Default handler for any node-to-node `Message`.
    ///
    /// Returns `RoleChange` to indicate if a role transition should occur.
    async fn handle_node_msg(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        db: &mut Database, // pass Database down so Ack / Log can use it
        msg: Message,
    ) -> AppResult<RoleChange> {
        let role_change = match msg {
            Message::Request {
                req_id: op_id,
                op,
                addr,
            } => {
                self.handle_request(connection, db, op_id, op, addr).await?;
                RoleChange::None
            }
            Message::Log { op_id, op } => {
                self.handle_log(connection, db, op_id, op).await?;
                RoleChange::None
            }
            Message::Ack { op_id } => {
                self.handle_ack(connection, db, op_id).await;
                RoleChange::None
            }
            Message::Election {
                candidate_id,
                candidate_addr,
            } => {
                self.handle_election(connection, candidate_id, candidate_addr)
                    .await?
            }
            Message::ElectionOk { responder_id } => {
                self.handle_election_ok(connection, responder_id).await;
                RoleChange::None
            }
            Message::Coordinator {
                leader_id,
                leader_addr,
            } => {
                let rc = self
                    .handle_coordinator(connection, leader_id, leader_addr)
                    .await?;
                rc
            }
            Message::Join { addr } => {
                self.handle_join(connection, db, addr).await?;
                RoleChange::None
            }
            Message::ClusterView {
                members,
                leader_addr,
                database,
            } => {
                self.handle_cluster_view(connection, db, members, leader_addr, database)
                    .await?;
                RoleChange::None
            }
            Message::ClusterUpdate { new_member } => {
                self.handle_cluster_update(connection, new_member).await;
                RoleChange::None
            }
            Message::Response { req_id, op_result } => {
                self.handle_response(connection, station, req_id, op_result)
                    .await?;
                RoleChange::None
            }
            Message::RoleQuery { addr } => {
                self.handle_role_query(connection, addr).await?;
                RoleChange::None
            }
            _ => todo!(),
        };

        Ok(role_change)
    }

    /// Election timeout hook (default: no-op).
    async fn handle_election_timeout(
        &mut self,
        _connection: &mut Connection,
    ) -> AppResult<RoleChange> {
        Ok(RoleChange::None)
    }

    /// Main async event loop for any node role (Leader / Replica).
    ///
    /// Receives references to Connection / Database / Station and returns a
    /// `RoleChange` when a role transition is required.
    async fn run(
        &mut self,
        connection: &mut Connection,
        db: &mut Database,
        station: &mut Station,
    ) -> AppResult<RoleChange> {
        // Interval for periodic election timeout checks.
        let mut election_check_interval =
            tokio::time::interval(std::time::Duration::from_millis(500));

        loop {
            let mut result: AppResult<RoleChange> = Ok(RoleChange::None);

            select! {
                // node messages
                node_msg = connection.recv() => {
                    match node_msg {
                        Ok(msg) => {
                            result = self.handle_node_msg(connection, station, db, msg).await;
                        }
                        Err(e) => {
                            result = Err(e);
                        },
                    }
                }
                // station messages
                pump_msg = station.recv() => {
                    match pump_msg {
                        Some(msg) => {
                            result = self.handle_station_msg(connection, station, db, msg).await;
                        }
                        None => panic!("[FATAL] station went down"),
                    }
                }
                // database events
                actor_evt = db.recv() => {
                    match actor_evt {
                        Some(evt) => {
                            self.handle_actor_event(connection, station, db, evt).await;
                        }
                        None => panic!("[FATAL] database went down"),
                    }
                }
                // periodic election timeout check
                _ = election_check_interval.tick() => {
                    result = self.handle_election_timeout(connection).await;
                }
            }

            match result {
                Ok(role_change) => match role_change {
                    RoleChange::None => {}
                    RoleChange::PromoteToLeader => return Ok(RoleChange::PromoteToLeader),
                    RoleChange::DemoteToReplica { new_leader_addr } => {
                        return Ok(RoleChange::DemoteToReplica { new_leader_addr })
                    }
                },
                Err(AppError::ConnectionLostWith { address }) => {
                    match self.handle_connection_lost_with(connection, address).await {
                        Ok(RoleChange::PromoteToLeader) => return Ok(RoleChange::PromoteToLeader),
                        Ok(RoleChange::DemoteToReplica { new_leader_addr }) => {
                            return Ok(RoleChange::DemoteToReplica { new_leader_addr })
                        }
                        _ => {}
                    }
                }
                Err(AppError::ConnectionLost) => {
                    continue;
                }
                Err(AppError::ConnectionRefused { address: _ }) => {
                    continue;
                }
                x => {
                    println!("[NODE] stopping run because of: {x:?}");
                    return x;
                }
            }
        }
    }
}

/// Runtime that may be operating as Leader or Replica.
pub enum NodeRuntime {
    Leader(Leader),
    Replica(Replica),
}

/// Top-level loop that maintains the node and handles role changes,
/// reusing the same Connection / Database / Station resources.
///
/// - Create `Connection`, `Database`, `Station` once.
/// - Swap `Leader` / `Replica` state objects on role change while keeping
///   the same shared resources.
pub async fn run_node_runtime(
    mut node: NodeRuntime,
    connection: &mut Connection,
    db: &mut Database,
    station: &mut Station,
) -> AppResult<()> {
    loop {
        let rc = match &mut node {
            NodeRuntime::Leader(leader) => leader.run(connection, db, station).await?,
            NodeRuntime::Replica(replica) => replica.run(connection, db, station).await?,
        };

        match rc {
            RoleChange::None => {
                // The internal run finished normally: stop the node.
                return Ok(());
            }
            RoleChange::PromoteToLeader => {
                node = match node {
                    NodeRuntime::Replica(replica) => {
                        println!("[LEADER] Now I am LEADER");
                        NodeRuntime::Leader(replica.into_leader())
                    },
                    NodeRuntime::Leader(leader) => {
                        // Already leader; keep it.
                        NodeRuntime::Leader(leader)
                    }
                };
            }
            RoleChange::DemoteToReplica { new_leader_addr } => {
                node = match node {
                    NodeRuntime::Leader(leader) => {
                        println!("[REPLICA] Now I am REPLICA with Leader {}", get_id_given_addr(new_leader_addr));
                        NodeRuntime::Replica(leader.into_replica(new_leader_addr))
                    }
                    NodeRuntime::Replica(replica) => {
                        // Shouldn't normally happen; keep the existing replica.
                        NodeRuntime::Replica(replica)
                    }
                };
            }
        }
    }
}

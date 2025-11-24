use super::actors::ActorEvent;
use super::database::Database;
use crate::errors::AppResult;
use crate::node::database;
use actix::dev::MessageResponse;
use common::operation::Operation;
use common::operation_result::OperationResult;
use common::{Connection, Message, NodeToStationMsg, Station, StationToNodeMsg};
use std::net::SocketAddr;
use tokio::select;
use tokio::time::Duration;

/// Signal to indicate if a role change should occur.
#[derive(Debug, Clone)]
pub enum RoleChange {
    None,
    PromoteToLeader,
    DemoteToReplica { new_leader_addr: SocketAddr },
}

pub trait Node {
    // messages from connection
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
    /// `db` is passed so implementations (e.g., Replica) can commit the
    /// operation into the actor-based "database" without owning Database.
    async fn handle_log(
        &mut self,
        connection: &mut Connection,
        db: &mut Database,
        op_id: u32,
        op: Operation,
    );

    // CHANGED: now it also receives a mutable reference to Database.
    async fn handle_ack(&mut self, connection: &mut Connection, db: &mut Database, op_id: u32);

    /// Resultado de una operación que ya pasó por el mundo de actores
    /// (Router + Account + Card) y volvió como OperationResult.
    async fn handle_operation_result(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        op_id: u32,
        operation: Operation,
        result: OperationResult,
    );

    /// Default dispatcher for Actor events.
    ///
    /// Ahora saca el `OperationResult` del ActorEvent y se lo pasa al
    /// `handle_operation_result`.
    async fn handle_actor_event(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        event: ActorEvent,
    ) {
        match event {
            ActorEvent::OperationResult {
                op_id,
                operation,
                result,
                // dejamos success/error en el enum pero no los necesitamos acá
                ..
            } => {
                self.handle_operation_result(connection, station, op_id, operation, result)
                    .await;
            }
            ActorEvent::Debug(msg) => {
                // Por ahora lo ignoramos, o podrías loggear acá.
                // e.g. println!("[NODE][ACTORS] {msg}");
                let _ = msg; // para evitar warning de variable sin usar.
            }
        }
    }

    fn is_offline(&self) -> bool;
    fn log_offline_operation(&mut self, op: Operation);
    fn get_address(&self) -> SocketAddr;

    async fn handle_charge_request(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        database: &mut Database,    
        account_id: u64,
        card_id: u64,
        amount: f32,
        request_id: u32,
    ) {
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
            if let Err(_e) = station.send(msg).await {}
            return;
        }

        let op = Operation::Charge {
            account_id,
            card_id,
            amount,
            from_offline_station: false,
        };

        self.handle_request(connection, database,request_id, op, self.get_address())
            .await
            .unwrap();
    }

    /// Default OFFLINE transition handler.
    ///
    /// Concrete nodes (Leader / Replica) override this to:
    /// - flip their `is_offline` flag,
    /// - emit debug messages to the Station,
    /// - adjust cluster behavior.
    async fn handle_disconnect_node(&mut self) {
        /* Example (Leader):
        if self.is_offline {
            let _ = self
                .station.send(NodeToStationMsg::Debug(
                    "[Leader] Node is already in OFFLINE mode; pump operations are auto-approved."
                        .to_string(),
                ))
                .await;
        } else {
            self.is_offline = true;
            let _ = self
                .station.send(NodeToStationMsg::Debug(
                    "[Leader] Node switched to OFFLINE mode. Cluster traffic will be ignored and pump operations will be queued and auto-approved."
                        .to_string(),
                ))
                .await;
        }
        */
    }

    /// Default ONLINE transition handler.
    ///
    /// Concrete nodes (Leader / Replica) override this to:
    /// - flip their `is_offline` flag,
    /// - replay queued offline operations into the actor layer,
    /// - notify the Station via debug messages.
    async fn handle_connect_node(&mut self) {
        /* Example (Leader):
        if !self.is_offline {
            let _ = self
                .station.send(NodeToStationMsg::Debug(
                    "[Leader] Node is already in ONLINE mode; pump operations go through normal verification."
                        .to_string(),
                ))
                .await;
        } else {
            self.is_offline = false;
            let queued = self.offline_queue.len();
            while let Some(OfflineQueuedCharge {
                request_id,
                account_id,
                card_id,
                amount,
            }) = self.offline_queue.pop_front()
            {
                let op = Operation::Charge {
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
                .station.send(NodeToStationMsg::Debug(
                    format!(
                        "[Leader] Node switched back to ONLINE mode. Replayed {} queued offline operations into the actor system (they were already confirmed to the station).",
                        queued
                    ),
                ))
                .await;
        }
        */
    }

    /// Default handler for high-level Station messages.
    ///
    /// Dispatches:
    /// - `ChargeRequest` into `handle_charge_request`,
    /// - `DisconnectNode` into `handle_disconnect_node`,
    /// - `ConnectNode` into `handle_connect_node`.
    async fn handle_station_msg(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        database: &mut Database,
        msg: StationToNodeMsg,
    ) {
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
                .await;
            }
            StationToNodeMsg::DisconnectNode => {
                self.handle_disconnect_node().await;
            }
            StationToNodeMsg::ConnectNode => {
                self.handle_connect_node().await;
            }
        }
    }

    // === Bully / leader election hooks ===
    async fn handle_election(
        &mut self,
        connection: &mut Connection,
        candidate_id: u64,
        candidate_addr: SocketAddr,
    );

    async fn handle_election_ok(&mut self, connection: &mut Connection, responder_id: u64);

    async fn handle_coordinator(
        &mut self,
        connection: &mut Connection,
        leader_id: u64,
        leader_addr: SocketAddr,
    ) -> RoleChange;

    async fn start_election(&mut self, connection: &mut Connection);

    // === Cluster membership hooks (Join / ClusterView) ===

    async fn handle_join(&mut self, connection: &mut Connection, addr: SocketAddr);

    async fn handle_cluster_view(&mut self, members: Vec<(u64, SocketAddr)>);

    async fn handle_cluster_update(
        &mut self,
        connection: &mut Connection,
        new_member: (u64, SocketAddr),
    );

    /// Default handler for any node-to-node Message.
    /// Returns RoleChange to signal if a role transition should occur.
    async fn handle_node_msg(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        db: &mut Database, // CHANGED: pass Database down so Ack / Log can use it
        msg: Message,
    ) -> AppResult<RoleChange> {
        let role_change = match msg {
            Message::Request {
                req_id: op_id,
                op,
                addr,
            } => {
                self.handle_request(connection, db,op_id, op, addr).await?;
                RoleChange::None
            }
            Message::Log { op_id, op } => {
                self.handle_log(connection, db, op_id, op).await;
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
                    .await;
                RoleChange::None
            }
            Message::ElectionOk { responder_id } => {
                self.handle_election_ok(connection, responder_id).await;
                RoleChange::None
            }
            Message::Coordinator {
                leader_id,
                leader_addr,
            } => {
                self.handle_coordinator(connection, leader_id, leader_addr)
                    .await
            }
            Message::Join { addr } => {
                self.handle_join(connection, addr).await;
                RoleChange::None
            }
            Message::ClusterView { members } => {
                self.handle_cluster_view(members).await;
                RoleChange::None
            }
            Message::ClusterUpdate { new_member } => {
                self.handle_cluster_update(connection, new_member).await;
                RoleChange::None
            }
            Message::Response { req_id, op_result } => {
                let _ = self.handle_response(connection, station, req_id, op_result)
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

    /// Main async event loop for any node role (Leader / Replica / Station-backed node).
    /// Returns a RoleChange signal if the node should switch roles.
    async fn run(
        &mut self,
        mut connection: Connection,
        mut db: Database,
        mut station: Station,
    ) -> AppResult<RoleChange> {
        use tokio::time::{interval, Instant};
        let mut check = interval(Duration::from_millis(100));
        let liveness_threshold = Duration::from_millis(300);
        let mut last_seen = Instant::now();

        loop {
            select! {
                // periodic tick to check liveness
                // _ = check.tick() => {
                //     let elapsed = last_seen.elapsed();
                //     if elapsed >= liveness_threshold {
                //         self.start_election(&mut connection).await;
                //         last_seen = Instant::now();
                //     }
                // }

                // === Node-to-node messages (Raft / Bully / Cluster membership) ===
                node_msg = connection.recv() => {
                    match node_msg {
                        Ok(msg) => {
                            last_seen = Instant::now();
                            let role_change = self.handle_node_msg(&mut connection, &mut station, &mut db, msg).await?;
                            match role_change {
                                RoleChange::None => {},
                                change => {
                                    println!("[NODE] Role change detected: {:?}", change);
                                    return Ok(change);
                                }
                            }
                        }
                        Err(_e) => {
                            // TODO: manejar errores de red si querés algo más fino
                        }
                    }
                }

                // === Station (pump simulator) messages ===
                pump_msg = station.recv() => {
                    match pump_msg {
                        Some(msg) => {
                            self.handle_station_msg(&mut connection, &mut station,&mut db, msg).await;
                        }
                        None => {
                            // Station side closed its channel.
                            todo!();
                        }
                    }
                }

                // === Actor events (OperationResult, etc.) ===
                actor_evt = db.recv() => {
                    match actor_evt {
                        Some(evt) => {
                            self.handle_actor_event(&mut connection, &mut station, evt).await;
                        }
                        None => {
                            // Actor system stopped; for now, we treat it as a fatal condition.
                            todo!();
                        }
                    }
                }
            }
        }
    }
}

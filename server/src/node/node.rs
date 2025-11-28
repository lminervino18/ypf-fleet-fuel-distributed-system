use super::actors::ActorEvent;
use super::database::Database;
use crate::errors::AppResult;
use common::operation::Operation;
use common::operation_result::OperationResult;
use common::{AppError, Connection, Message, NodeToStationMsg, Station, StationToNodeMsg};
use std::net::SocketAddr;
use tokio::select;

// Para el runtime de cambio de rol
use super::{leader::Leader, replica::Replica};

/// Signal to indicate if a role change should occur.
#[derive(Debug, Clone)]
pub enum RoleChange {
    None,
    PromoteToLeader,
    DemoteToReplica { new_leader_addr: SocketAddr },
}

pub trait Node {
    async fn get_status(&self) -> String;

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
        new_op: Operation,
    ) -> AppResult<()>;

    async fn handle_ack(&mut self, connection: &mut Connection, db: &mut Database, op_id: u32);

    /// Resultado de una operación que ya pasó por el mundo de actores
    /// (Router + Account + Card) y volvió como OperationResult.
    async fn handle_operation_result(
        &mut self,
        connection: &mut Connection,
        _station: &mut Station,
        op_id: u32,
        _operation: Operation,
        _result: OperationResult,
    ) -> AppResult<()>;

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
                ..
            } => {
                if let Err(e) = self
                    .handle_operation_result(connection, station, op_id, operation, result)
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

    /// Default OFFLINE transition handler.
    ///
    /// Concrete nodes (Leader / Replica) override this to:
    /// - flip their `is_offline` flag,
    /// - emit debug messages to the Station,
    /// - adjust cluster behavior.
    async fn handle_disconnect_node(&mut self, connection: &mut Connection);

    /// Default ONLINE transition handler.
    ///
    /// Concrete nodes (Leader / Replica) override this to:
    /// - flip their `is_offline` flag,
    /// - replay queued offline operations into the actor layer,
    /// - notify the Station via debug messages.
    async fn handle_connect_node(&mut self, connection: &mut Connection) -> AppResult<()>;

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

    /// Inicia una elección Bully.
    /// Devuelve:
    /// - `RoleChange::None` si solo arranca/continúa elección.
    /// - `RoleChange::PromoteToLeader` si se autoproclama líder (era réplica).
    /// - `RoleChange::None` si se autoproclama líder pero ya era Leader.
    async fn start_election(&mut self, connection: &mut Connection) -> AppResult<RoleChange>;

    // === Cluster membership hooks (Join / ClusterView) ===

    async fn handle_join(&mut self, connection: &mut Connection, addr: SocketAddr)
        -> AppResult<()>;

    async fn handle_cluster_view(
        &mut self,
        connection: &mut Connection,
        database: &mut Database,
        members: Vec<(u64, SocketAddr)>,
    ) -> AppResult<()>;

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
                println!("{}", self.get_status().await);
                rc
            }
            Message::Join { addr } => {
                self.handle_join(connection, addr).await?;
                RoleChange::None
            }
            Message::ClusterView { members } => {
                self.handle_cluster_view(connection, db, members).await?;
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

    async fn handle_run_result(
        &mut self,
        connection: &mut Connection,
        result: AppResult<RoleChange>,
    ) -> AppResult<RoleChange> {
        match result {
            Ok(_) => Ok(RoleChange::None), // ok está handleado en el loop
            Err(AppError::ConnectionLostWith { address }) => {
                let role_change = self
                    .handle_connection_lost_with(connection, address)
                    .await?;
                match role_change {
                    RoleChange::None => Ok(RoleChange::None),
                    RoleChange::PromoteToLeader => Ok(role_change),
                    RoleChange::DemoteToReplica { .. } => Ok(role_change),
                }
            }
            Err(e) => Err(e)?,
        }
    }

    /// Hook de timeout de elección (por defecto, no hace nada).
    async fn handle_election_timeout(
        &mut self,
        _connection: &mut Connection,
    ) -> AppResult<RoleChange> {
        Ok(RoleChange::None)
    }

    /// Main async event loop for any node role (Leader / Replica / Station-backed node).
    /// Ahora recibe referencias a Connection / Database / Station y devuelve
    /// un RoleChange si hay que cambiar de rol.
    async fn run(
        &mut self,
        connection: &mut Connection,
        db: &mut Database,
        station: &mut Station,
    ) -> AppResult<RoleChange> {
        // Intervalo para chequeos periódicos de timeout de elección
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
                            self.handle_actor_event(connection, station, evt).await;
                        }
                        None => panic!("[FATAL] database went down"),
                    }
                }
                // chequeo periódico de timeout de elección
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
                    println!("[NODE] stopping run beacause of: {x:?}");
                    return x;
                }
            }
        }
    }
}

/// Runtime que puede estar corriendo como Leader o Replica.
pub enum NodeRuntime {
    Leader(Leader),
    Replica(Replica),
}

/// Loop que mantiene vivo el nodo y maneja cambios de rol
/// reutilizando SIEMPRE las mismas Connection / Database / Station.
///
/// - Se crea una sola vez `Connection`, `Database`, `Station`.
/// - `Leader` y `Replica` se recrean al volar de rol, pero con las
///   mismas referencias a esos recursos.
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
                // El run interno terminó "normalmente": apagamos el nodo.
                return Ok(());
            }
            RoleChange::PromoteToLeader => {
                node = match node {
                    NodeRuntime::Replica(replica) => {
                        println!("[RUNTIME] Promoting Replica to Leader");
                        NodeRuntime::Leader(replica.into_leader())
                    }
                    NodeRuntime::Leader(leader) => {
                        // Ya somos líder; no cambiamos nada.
                        NodeRuntime::Leader(leader)
                    }
                };
            }
            RoleChange::DemoteToReplica { new_leader_addr } => {
                node = match node {
                    NodeRuntime::Leader(leader) => {
                        println!(
                            "[RUNTIME] Demoting Leader to Replica with new leader at {}",
                            new_leader_addr
                        );
                        NodeRuntime::Replica(leader.into_replica(new_leader_addr))
                    }
                    NodeRuntime::Replica(replica) => {
                        // No debería pasar, pero por si acaso dejamos la réplica como está.
                        println!(
                            "[RUNTIME] DemoteToReplica received while already Replica; ignoring"
                        );
                        NodeRuntime::Replica(replica)
                    }
                };
            }
        }
    }
}

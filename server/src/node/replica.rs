use super::{
    database::{Database, DatabaseCmd},
    leader::Leader,
    node::Node,
    utils::get_id_given_addr,
};
use crate::{
    errors::AppResult,
    node::node::RoleChange,
};
use common::{
    operation::Operation,
    operation_result::{ChargeResult, OperationResult},
    AppError, Connection, Message, NodeToStationMsg, Station,
};

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    time::{Duration, Instant},
};

/// Replica node.
///
/// Misma idea que el Leader:
/// - recibe logs del líder (`Message::Log`),
/// - guarda las operaciones,
/// - las va “commit-eando” contra el sistema de actores (vía `Database`),
/// - cuando el router termina una op, la réplica le manda un `Ack` al líder.
///
/// Para las requests que vienen de la estación:
/// - si está ONLINE, las proxy-a al líder con `Message::Request`,
/// - si está OFFLINE, las mete en `offline_queue` y contesta OK a la estación.
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct StationPendingCharge {
    pump_id: u32,
    account_id: u64,
    card_id: u64,
    amount: f32,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct OfflineQueuedCharge {
    request_id: u64,
    account_id: u64,
    card_id: u64,
    amount: f32,
}

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

    // Estado de la elección Bully
    election_in_progress: bool,
    election_start: Option<Instant>,
}

impl Node for Replica {
    fn is_offline(&self) -> bool {
        self.is_offline
    }

    async fn get_status(&self) -> String{
        //give mi cluster and leader, only that
        format!("Cluster: {:?}, Leader: {:?}", self.cluster.len(), self.leader_addr)
    }
    
    fn log_offline_operation(&mut self, op: Operation) {
        self.offline_queue.push_back(op);
    }

    fn get_address(&self) -> SocketAddr {
        self.address
    }

    /// Handle a lost connection to a peer.
    async fn handle_connection_lost_with(
        &mut self,
        connection: &mut Connection,
        lost_address: SocketAddr,
    ) -> AppResult<RoleChange> {

        // Si se cayó el líder (o algún miembro del cluster), arrancamos elección
        if lost_address == self.leader_addr
        {
            println!(
                "[REPLICA {}] Leader/peer {:?} is DOWN, starting election",
                self.id, lost_address
            );
            return self.start_election(connection).await;
        }

        if self.cluster.contains_key(&get_id_given_addr(lost_address)) {
            println!(
                "[REPLICA {}] Peer {:?} is DOWN, removing from cluster",
                self.id, lost_address
            );
            self.cluster.retain(|_, &mut addr| addr != lost_address);
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
        match connection
            .send(Message::Join { addr: self.address }, &self.leader_addr)
            .await
        {
            Err(AppError::ConnectionLostWith {
                address: _leader_addr,
            }) => {
                connection
                    .send(Message::Join { addr: self.address }, &self.leader_addr)
                    .await?
            }
            x => x?,
        }

        Ok(())
    }

    async fn anounce_coordinator(
        &mut self,
        connection: &mut Connection,
    ) -> AppResult<RoleChange> {
        // Broadcast de Coordinator a todo el cluster
        println!("mi cluster is {:?}", self.cluster);
        for (node, addr) in &self.cluster {
            if *node == self.id {
                continue; // Skip announcing to ourselves
            }

            println!(
                "[REPLICA {}] -> sending Coordinator to {:?} (id={})",
                self.id, addr, node
            );

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

        // Handle our own promoción a leader
        self.handle_coordinator(connection, self.id, self.address)
            .await
    }

    /// Bully election:
    /// - si hay IDs mayores, mando Election y arranco timeout
    /// - si NO hay IDs mayores, me proclamo líder directo.
    async fn start_election(
        &mut self,
        connection: &mut Connection,
    ) -> AppResult<RoleChange> {
        if self.election_in_progress {
            println!(
                "[REPLICA {}] start_election called but election already in progress",
                self.id
            );
            return Ok(RoleChange::None);
        }

        println!(
            "[REPLICA {}] Starting election, checking for higher-ID nodes...",
            self.id
        );

        let mut sent_any = false;

        for (&node_id, replica_addr) in &self.cluster {
            if node_id > self.id {
                sent_any = true;
                println!(
                    "[REPLICA {}] -> sending Election to {:?} (id={})",
                    self.id, replica_addr, node_id
                );
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
            // Hay al menos un nodo con ID mayor → elección normal con timeout
            self.election_in_progress = true;
            self.election_start = Some(Instant::now());
            Ok(RoleChange::None)
        } else {
            // No hay ningún nodo con ID mayor → soy el más grande → líder YA
            println!(
                "[REPLICA {}] No higher-ID nodes found. I am the highest → announcing myself as COORDINATOR",
                self.id
            );
            self.election_in_progress = false;
            self.election_start = None;

            // Esto manda Coordinator a todos y luego llama a handle_coordinator(self.id, self.address),
            // que devuelve RoleChange::PromoteToLeader.
            self.anounce_coordinator(connection).await
        }
    }

    async fn handle_operation_result(
        &mut self,
        connection: &mut Connection,
        _station: &mut Station,
        op_id: u32,
        _operation: Operation,
        _result: OperationResult,
    ) -> AppResult<()> {
        // Igual que antes: cuando termina la operación en la réplica,
        // mandamos un Ack al líder.
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
        // redirijo al líder
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
        // Guardamos el log y commiteamos la operación anterior.
        self.operations.insert(op_id, new_op);
        if op_id == 0 {
            return connection
                .send(Message::Ack { op_id: 0 }, &self.leader_addr)
                .await;
        }

        // Igual que antes, pero ahora via `Database` en vez de `router`.
        self.commit_operation(db, op_id - 1).await
    }

    async fn handle_ack(&mut self, _connection: &mut Connection, _db: &mut Database, _id: u32) {
        // Replicas no deberían recibir ACKs.
        todo!();
    }

    async fn handle_election(
        &mut self,
        connection: &mut Connection,
        candidate_id: u64,
        candidate_addr: SocketAddr,
    ) -> AppResult<RoleChange> {
        if self.id > candidate_id {
            println!(
                "[REPLICA {}] Received Election from {}. Replying ElectionOk and starting own election",
                self.id,
                get_id_given_addr(candidate_addr)
            );
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

            // Yo soy más grande, así que también inicio mi propia elección
            self.start_election(connection).await
        } else {
            println!(
                "[REPLICA {}] Received Election from {}, not replying (candidate has >= ID)",
                self.id,
                get_id_given_addr(candidate_addr)
            );
            Ok(RoleChange::None)
        }
    }

    async fn handle_election_ok(
        &mut self,
        _connection: &mut Connection,
        responder_id: u64,
    ) {
        println!(
            "[REPLICA {}] Received ElectionOk from {}. Waiting for Coordinator...",
            self.id, responder_id
        );
        // Si alguien más grande respondió, ya no me auto-proclamo líder por timeout.
        self.election_in_progress = false;
        self.election_start = None;
    }

    async fn handle_coordinator(
        &mut self,
        _connection: &mut Connection,
        leader_id: u64,
        leader_addr: SocketAddr,
    ) -> AppResult<RoleChange> {
        // Resetear estado de elección (ya hay coordinador)
        self.election_in_progress = false;
        self.election_start = None;

        // Check if we should promote to leader
        if leader_id == self.id {
            println!(
                "[REPLICA {}] Election complete: I AM THE LEADER promoting to LEADER",
                self.id
            );
            return Ok(RoleChange::PromoteToLeader);
        }

        if leader_id < self.id {
            panic!("leader_id should not be less than self");
        }

        println!(
            "[REPLICA {}] Election complete: new LEADER is {} at {}",
            self.id, leader_id, leader_addr
        );

        // Update our local "current leader" pointer
        self.leader_addr = leader_addr;
        Ok(RoleChange::None)
    }

    async fn handle_join(
        &mut self,
        connection: &mut Connection,
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
        self.cluster.insert(new_member.0, new_member.1);
        println!("[REPLICA] handle cluster update: {new_member:?}");
        println!(
            "[REPLICA] ============ current members {:?}: {:?}",
            self.cluster.len(),
            self.cluster
        );
    }

    async fn handle_cluster_view(
        &mut self,
        connection: &mut Connection,
        database: &mut Database,
        members: Vec<(u64, SocketAddr)>,
    ) -> AppResult<()> {
        while let Some(op) = self.offline_queue.pop_front() {
            self.handle_request(connection, database, 0, op, self.address)
                .await?
        }

        self.cluster.clear();
        for (id, addr) in members {
            self.cluster.insert(id, addr);
        }

        println!(
            "[REPLICA {}] Cluster Update, Cluster Members: {:?}",
            self.id, self.cluster
        );
        Ok(())
    }

    /// Timeout handler: si pasó el tiempo y nadie respondió con ElectionOk,
    /// nos auto-proclamamos líder (Bully).
    async fn handle_election_timeout(
        &mut self,
        connection: &mut Connection,
    ) -> AppResult<RoleChange> {
        if !self.election_in_progress {
            // No hay elección en curso
            return Ok(RoleChange::None);
        }

        let Some(start) = self.election_start else {
            return Ok(RoleChange::None);
        };

        // Si pasaron más de 2 segundos sin ElectionOk → me proclamo líder
        if start.elapsed() >= Duration::from_secs(2) {
            println!(
                "[REPLICA {}] Election timeout exceeded. No ElectionOk received → announcing myself as COORDINATOR",
                self.id
            );
            self.election_in_progress = false;
            self.election_start = None;

            // Esto manda Coordinator a todos y luego llama a handle_coordinator(self.id, self.address),
            // que devuelve RoleChange::PromoteToLeader.
            return self.anounce_coordinator(connection).await;
        }

        Ok(RoleChange::None)
    }
}

impl Replica {
    /// Convert this Replica into a Leader.
    /// Consumes self and returns a Leader.
    pub fn into_leader(self) -> Leader {
        use super::leader::Leader;

        // When promoting, we use the replica's last op_id as current_op_id
        let current_op_id = self.operations.keys().max().copied().unwrap_or(0);

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

    /// Create a new Replica from existing node state (used for Leader demotion)
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

        // Igual que en Leader: fire-and-forget al actor world, ahora vía `Database`.
        db.send(DatabaseCmd::Execute {
            op_id,
            operation: op,
        });

        Ok(())
    }

    /// Continue running as Replica after being demoted from Leader.
    /// Reuses existing state from the leader.
    pub async fn run_from_leader(
        mut replica: Replica,
        address: SocketAddr,
        coords: (f64, f64),
        max_conns: usize,
        pumps: usize,
    ) -> AppResult<()> {
        // Recreate resources that were consumed by leader.run()
        let db = super::database::Database::start().await?;
        let station = Station::start(pumps).await?;
        let connection = Connection::start(address, max_conns).await?;

        // Loop para manejar cambios de rol
        loop {
            let role_change = replica.run(connection, db, station).await?;
            match role_change {
                RoleChange::PromoteToLeader => {
                    let leader = replica.into_leader();
                    return Box::pin(Leader::run_from_replica(
                        leader, address, coords, max_conns, pumps,
                    ))
                    .await;
                }
                x => {
                    println!("unknown result: {x:?}");
                    return Ok(());
                }
            }
        }
    }

    /// - boots the ConnectionManager (TCP),
    /// - boots the actor system wrapped in `Database` (Actix in a dedicated thread),
    /// - starts the Station abstraction (which internally spawns the simulator with `pumps` pumps on stdin),
    /// - seeds the cluster membership (self + leader),
    /// - sends an initial Join message to the leader,
    /// - then evalúa Bully (puede autoproclamarse líder),
    /// - y entra al loop principal como Replica o como Leader.
    pub async fn start(
        address: SocketAddr,
        leader_addr: SocketAddr,
        coords: (f64, f64),
        max_conns: usize,
        pumps: usize,
    ) -> AppResult<()> {
        // Start the actor-based "database" subsystem (ActorRouter + Actix system hidden inside).
        let db = super::database::Database::start().await?;

        // Start the reusable Station abstraction (stdin-based simulator runs inside it).
        let station = Station::start(pumps).await?;

        // Start network connection.
        let mut connection = Connection::start(address, max_conns).await?;

        // Seed membership: only leader at first.
        let id = get_id_given_addr(address);

        println!(
            "[REPLICA {}] Started at {}, leader at {}",
            id, address, leader_addr
        );
        let mut members: HashMap<u64, SocketAddr> = HashMap::new();

        let leader_id = get_id_given_addr(leader_addr);
        members.insert(leader_id, leader_addr);

        let mut replica = Self {
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

        // Join inicial contra el líder para que rebroadcastée el ClusterView.
        let _ = connection
            .send(
                Message::Join {
                    addr: replica.address,
                },
                &replica.leader_addr,
            )
            .await;

        // para cambios de rol
        // Usamos el loop genérico de `Node::run`, igual que el Leader:
        loop {
            let role_change = replica.run(connection, db, station).await?;
            match role_change {
                RoleChange::PromoteToLeader => {
                    println!("[REPLICA] Converting to Leader...");
                    let leader = replica.into_leader();
                    return Box::pin(Leader::run_from_replica(
                        leader, address, coords, max_conns, pumps,
                    ))
                    .await;
                }
                _ => {
                    // if there's no role change, it means run() ended for another reason
                    return Ok(());
                }
            }
        }
    }

    /// Test helpers to access private fields.
    #[cfg(test)]
    pub fn test_get_id(&self) -> u64 {
        self.id
    }
    #[cfg(test)]
    pub fn test_get_leader_id(&self) -> u64 {
        get_id_given_addr(self.leader_addr)
    }
    #[cfg(test)]
    pub fn test_get_members(&self) -> HashMap<u64, SocketAddr> {
        self.cluster.clone()
    }
    #[cfg(test)]
    pub fn test_get_operations(&self) -> HashMap<u32, Operation> {
        self.operations.clone()
    }
}

use super::{
    database::{Database, DatabaseCmd},
    election::bully::Bully,
    leader::Leader,
    node::Node,
    utils::get_id_given_addr,
};
use crate::{
    errors::AppResult,
    node::{database, node::RoleChange},
};
use common::{
    operation::Operation,
    operation_result::{ChargeResult, OperationResult},
    AppError, Connection, Message, NodeToStationMsg, Station, StationToNodeMsg,
};

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::Arc,
};
use tokio::sync::Mutex;

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
#[derive(Debug, Clone)]
struct StationPendingCharge {
    pump_id: u32,
    account_id: u64,
    card_id: u64,
    amount: f32,
}

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
    bully: Arc<Mutex<Bully>>,
    operations: HashMap<u32, Operation>,
    is_offline: bool,
    offline_queue: VecDeque<Operation>,
    // NOTE: no `router` field anymore; we talk to the actor system via `Database`.
}

impl Node for Replica {
    fn is_offline(&self) -> bool {
        self.is_offline
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
        address: SocketAddr,
    ) -> AppResult<RoleChange> {
        /* let Some(_dead_node) = self.cluster.remove(&get_id_given_addr(address)) else {
            return Ok(RoleChange::None); // me voy sin preguntar
        }; */

        if address == self.leader_addr {
            println!(
                "[REPLICA {}] Se cayó el líder, arranco leader election",
                self.id
            );
            //return self.start_election(connection).await;
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
        println!(
            "[REPLICA {}] Reconnected to the network, sending Join to leader",
            self.id
        );
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
    async fn anounce_coordinator(&mut self, connection: &mut Connection) -> AppResult<RoleChange> {
        println!("[REPLICA {}] announcing myself as coordinator", self.id);
        for node in self.cluster.values() {
            if node == &self.address {
                continue;
            }

            connection
                .send(
                    Message::Coordinator {
                        leader_id: self.id,
                        leader_addr: self.address,
                    },
                    node,
                )
                .await?;
        }

        self.handle_coordinator(connection, self.id, self.address)
            .await
    }

    async fn start_election(&mut self, connection: &mut Connection) -> AppResult<RoleChange> {
        println!("[REPLICA {}] starting election", self.id);
        let mut greater_ids = 0;
        for (id, addr) in &self.cluster {
            if *id > self.id
                && connection
                    .send(
                        Message::Election {
                            candidate_id: self.id,
                            candidate_addr: self.address,
                        },
                        addr,
                    )
                    .await
                    .is_ok()
            {
                println!(
                    "[REPLICA {}] could send election to a higher role: {}",
                    self.id, id
                );
                greater_ids += 1;
            }
        }

        if greater_ids > 0 {
            return Ok(RoleChange::None);
        }

        self.anounce_coordinator(connection).await
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
        connection: &mut Connection,
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

            // Por ahora ignoramos otros resultados (Limit*, AccountQuery, etc.)
            // Podés loguearlos o manejarlos según lo vayas necesitando.
            other => {
                // Ejemplo de log (si querés dejar algo):
                // eprintln!("[Node] handle_response: OperationResult inesperado: {:?}", other);
            }
        }

        Ok(())
    }

    async fn handle_role_query(
        &mut self,
        connection: &mut Connection,
        addr: SocketAddr,
    ) -> AppResult<()> {
        println!("[REPLICA] Received role query from {:?}", addr);
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
        println!("[REPLICA] Received Log for op_id={op_id}: {new_op:?}");
        // Guardamos el log y commiteamos la operación anterior.
        self.operations.insert(op_id, new_op);
        if op_id == 0 {
            return connection
                .send(Message::Ack { op_id: 0 }, &self.leader_addr)
                .await;
        }

        // Igual que antes, pero ahora via `Database` en vez de `router`.
        // Ignoramos el Result como ya hacíamos antes.
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
        println!(
            "[REPLICA {}] received election message from {}",
            self.id, candidate_addr
        );
        if candidate_id > self.id {
            todo!("election message should never come from a greater id");
        }

        connection
            .send(
                Message::ElectionOk {
                    responder_id: self.id,
                },
                &candidate_addr,
            )
            .await?;

        self.start_election(connection).await
    }

    async fn handle_election_ok(&mut self, _connection: &mut Connection, responder_id: u64) {
        println!("[REPLICA {}] received election OK message", self.id);
        // no hago nada porque en realidad si no me "iba a llegar" ya crasheé antes, el tema es si
        // el otro nodo crashea justo después de recibir mi mensaje de election (no va a pasar en
        // nuestra demo ;)   )
    }

    async fn handle_coordinator(
        &mut self,
        _connection: &mut Connection,
        leader_id: u64,
        leader_addr: SocketAddr,
    ) -> AppResult<RoleChange> {
        println!(
            "[REPLICA {}] AAAAAAAAAAAA received coordinator message from {}",
            self.id, leader_addr
        );
        if leader_id == self.id {
            println!(
                "[REPLICA {}] coordinator msg id is self.id, address: {}",
                self.id, leader_addr
            );
            return Ok(RoleChange::PromoteToLeader);
        }

        // TODO: por ahí ya q somos réplica confiamos en el algoritmo e ignoramos el caso de q el
        // id sea menor al nuestro
        if leader_id < self.id {
            todo!("leader_id should not be less than self");
        }

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
        println!("[REPLICA] current members: {:?}", self.cluster.len());
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

        println!("[REPLICA] handled cluster view: {:?}", self.cluster);
        Ok(())
    }
}

impl Replica {
    /// Convert this Replica into a Leader.
    /// Consumes self and returns a Leader.
    pub fn into_leader(self) -> Leader {
        use super::leader::Leader;

        // When promoting, we use the replica's last op_id as current_op_id
        let current_op_id = self.operations.keys().max().copied().unwrap_or(0);
        // let only_last_op = self
        //     .operations
        //     .get(&current_op_id)
        //     .cloned()
        //     .map(|op| {
        //         let mut map = HashMap::new();
        //         map.insert(current_op_id, op);
        //         map
        //     })
        //     .unwrap_or_default();

        Leader::from_existing(
            self.id,
            current_op_id,
            self.coords,
            self.address,
            self.cluster,
            self.bully,
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
        bully: Arc<Mutex<Bully>>,
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
            bully,
            operations,
            is_offline,
            offline_queue,
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
            println!("[REPLICA] run stopped, changing role");
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
    /// - then enters the main async event loop.
    /// If promoted to Leader, converts itself and continues running as Leader.
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

        // Seed membership: self + leader.
        let id = get_id_given_addr(address);
        let mut members: HashMap<u64, SocketAddr> = HashMap::new();
        // members.insert(id, address);

        let leader_id = get_id_given_addr(leader_addr);
        members.insert(leader_id, leader_addr);

        let mut replica = Self {
            id,
            coords,
            address,
            leader_addr,
            cluster: members,
            bully: Arc::new(Mutex::new(Bully::new(id, address))),
            operations: HashMap::new(),
            is_offline: false,
            offline_queue: VecDeque::new(),
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
        // - own `connection`, `db` y `station`.
        loop {
            let role_change = replica.run(connection, db, station).await?;
            match role_change {
                super::node::RoleChange::PromoteToLeader => {
                    println!("[REPLICA] Converting to Leader...");
                    // con Pin le digo al compilador que el Future se quedara en la misma dirección de memoria y que las referencias internas son validas
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

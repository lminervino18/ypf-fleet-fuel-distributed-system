use super::{
    database::{Database, DatabaseCmd},
    election::bully::Bully,
    node::Node,
    utils::get_id_given_addr,
};
use crate::errors::AppResult;
use common::{
    operation::Operation,
    operation_result::{OperationResult, ChargeResult},
    Connection,
    Message,
    NodeToStationMsg,
    Station,
    StationToNodeMsg,
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
    members: HashMap<u64, SocketAddr>,
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

    async fn handle_operation_result(
        &mut self,
        connection: &mut Connection,
        _station: &mut Station,
        op_id: u32,
        _operation: Operation,
        _result: OperationResult,
    ) {
        // Igual que antes: cuando termina la operación en la réplica,
        // mandamos un Ack al líder.
        connection
            .send(Message::Ack { op_id: op_id + 1 }, &self.leader_addr)
            .await
            .unwrap();
    }

    async fn handle_request(
        &mut self,
        connection: &mut Connection,
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


    async fn handle_log(
        &mut self,
        connection: &mut Connection,
        db: &mut Database,
        op_id: u32,
        new_op: Operation,
    ) {
        println!("[REPLICA] Received Log for op_id={}: {:?}", op_id, new_op);
        // Guardamos el log y commiteamos la operación anterior.
        self.operations.insert(op_id, new_op);
        if op_id == 0 {
            connection.send(Message::Ack { op_id: 0 }, &self.leader_addr).await.unwrap();
            return; // si es la primera no hay op previamente loggeada
        }

        // Igual que antes, pero ahora via `Database` en vez de `router`.
        // Ignoramos el Result como ya hacíamos antes.
        self.commit_operation(db, op_id - 1).await.unwrap();
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
    ) {
        // If we have higher id, reply ElectionOk and start our own election.
        let should_reply = {
            let b = self.bully.lock().await;
            b.should_reply_ok(candidate_id)
        };
        if should_reply {
            let reply = Message::ElectionOk {
                responder_id: self.id,
            };
            let _ = connection.send(reply, &candidate_addr).await;

            // Start own election immediately.
            self.start_election(connection).await;
        }
    }

    async fn handle_election_ok(&mut self, _connection: &mut Connection, responder_id: u64) {
        let mut b = self.bully.lock().await;
        b.on_election_ok(responder_id);
    }

    async fn handle_coordinator(
        &mut self,
        _connection: &mut Connection,
        leader_id: u64,
        leader_addr: SocketAddr,
    ) {
        let mut b = self.bully.lock().await;
        b.on_coordinator(leader_id, leader_addr);
        // Update our local "current leader" pointer as well.
        self.leader_addr = leader_addr;

        // TODO: promoción a líder si corresponde.
    }

    async fn start_election(&mut self, connection: &mut Connection) {
        // Build peer_ids map from the current membership (excluding self).
        let mut peer_ids = HashMap::new();
        for (peer_id, addr) in &self.members {
            if *peer_id == self.id {
                continue;
            }

            peer_ids.insert(*peer_id, *addr);
        }

        crate::node::election::bully::conduct_election(
            &self.bully,
            connection,
            peer_ids,
            self.id,
            self.address,
        )
        .await;
    }

    async fn handle_join(&mut self, _connection: &mut Connection, _addr: SocketAddr) {
        // No-op for replicas.
    }

    async fn handle_cluster_update(
        &mut self,
        _connection: &mut Connection,
        new_member: (u64, SocketAddr),
    ) {
        self.members.insert(new_member.0, new_member.1);
        println!("[REPLICA] new cluster member added: {:?}", new_member);
        println!("[REPLICA] current members: {:?}", self.members.len());
    }

    async fn handle_cluster_view(&mut self, members: Vec<(u64, SocketAddr)>) {
        // Si llega el cluster_view es porque nosotros mandamos el join, así que está ok.
        self.members.clear();
        for (id, addr) in members {
            self.members.insert(id, addr);
        }

        // Ensure we are present with the correct address.
        self.members.insert(self.id, self.address);
        println!("[REPLICA] updated cluster view: {:?}", self.members);
    }
}

impl Replica {
    pub fn into_leader(self) -> Leader {
        use super::leader::Leader;


        Leader::from_existing(
            self.id,
            current_op_id,
            self.coords,
            self.address,
            self.members,
            self.bully,
            self.operations,
            self.is_offline,
            self.offline_queue,
        )
    }
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
            members,
            bully,
            operations,
            is_offline,
            offline_queue,
        }
    }
    async fn commit_operation(&mut self, db: &mut Database, op_id: u32) -> AppResult<()> {
        let Some(op) = self.operations.remove(&op_id) else {
            todo!();
        };

        // Igual que en Leader: fire-and-forget al actor world, ahora vía `Database`.
        db.send(DatabaseCmd::Execute {
            op_id,
            operation: op,
        });

        Ok(())
    }

    /// - boots the ConnectionManager (TCP),
    /// - boots the actor system wrapped in `Database` (Actix in a dedicated thread),
    /// - starts the Station abstraction (which internally spawns the simulator with `pumps` pumps on stdin),
    /// - seeds the cluster membership (self + leader),
    /// - sends an initial Join message to the leader,
    /// - then enters the main async event loop.
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
        members.insert(id, address);

        let leader_id = get_id_given_addr(leader_addr);
        members.insert(leader_id, leader_addr);

        let mut replica = Self {
            id,
            coords,
            address,
            leader_addr,
            members,
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

        // Usamos el loop genérico de `Node::run`, igual que el Leader:
        // - own `connection`, `db` y `station`.
        replica.run(connection, db, station).await
    }
}

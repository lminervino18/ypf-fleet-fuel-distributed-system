use super::database::{Database, DatabaseCmd}; // use the Database abstraction
use super::node::Node;
use super::pending_operatoin::PendingOperation;
use super::replica::Replica;
use crate::node::node::RoleChange;
use crate::{errors::AppResult, node::utils::get_id_given_addr};
use common::operation::Operation;
use common::operation_result::{ChargeResult, OperationResult};
use common::{Connection, Message, NodeToStationMsg, Station};
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    time::{Duration, Instant},
};

/// Leader node.
///
/// The Leader wires together:
/// - the TCP ConnectionManager (network),
/// - the local "database" (actor system wrapped in `Database`),
/// - the Station abstraction (which internally runs the stdin-driven pump simulator).
///
/// ONLINE behavior (default):
/// --------------------------
/// For a `Charge` coming from a pump the flow is:
///
/// 1. Station → Leader: `StationToNodeMsg::ChargeRequest`.
/// 2. Leader builds `Operation::Charge { .., from_offline_station: false }`
///    and sends a high-level Execute command to the actor system.
/// 3. ActorRouter/actors run verification + apply internally.
/// 4. Once finished, ActorRouter emits a single
///    `ActorEvent::OperationResult { op_id, operation, result, .. }`.
/// 5. Leader traduce ese `OperationResult` en:
///    - `NodeToStationMsg::ChargeResult` si viene de la estación,
///    - `Message::Response { result: OperationResult }` si viene de un cliente TCP.
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
///       - **not** sent to the actor system yet.
/// - Station → Leader: `StationToNodeMsg::ConnectNode`
///   sets `is_offline = false` and **replays** all queued charges to
///   the actor system with `from_offline_station = true`.
pub struct Leader {
    id: u64,
    current_op_id: u32,
    coords: (f64, f64),
    address: SocketAddr,
    cluster: HashMap<u64, SocketAddr>,
    operations: HashMap<u32, PendingOperation>,
    is_offline: bool,
    offline_queue: VecDeque<Operation>,

    // Estado de elección Bully (mismo concepto que en Replica)
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

    async fn get_status(&self) -> String{
        //give mi cluster and leader, only that
        format!("Cluster: {:?}, Leader: {:?}", self.cluster.len(), self.address)
    }

    fn log_offline_operation(&mut self, op: Operation) {
        self.offline_queue.push_back(op);
    }

    fn get_address(&self) -> SocketAddr {
        self.address
    }

    async fn anounce_coordinator(
        &mut self,
        connection: &mut Connection,
    ) -> AppResult<RoleChange> {
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

        // Ya soy líder; no hay cambio de rol.
        Ok(RoleChange::None)
    }

    async fn handle_connection_lost_with(
        &mut self,
        _connection: &mut Connection,
        address: SocketAddr,
    ) -> AppResult<RoleChange> {
        if let Some(_) = self.cluster.values().find(|&&addr| addr == address) {
            println!("[LEADER] Connection lost with {:?} that is a REPLICA", address);
        } else {
            println!(
                "[LEADER] Connection lost with {:?}, but it was a client",
                address
            );
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
        for node in self.cluster.values() {
            if connection
                .send(Message::Join { addr: self.address }, node)
                .await
                .is_err()
            {
                continue;
            }
        }

        Ok(())
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
        if pending.ack_count != (self.cluster.len() - 1) / 2 && self.cluster.len() - 1 != 1 {
            return;
        }

        // Mayoría alcanzada → ejecutar la operación en el mundo de actores.
        db.send(DatabaseCmd::Execute {
            op_id,
            operation: pending.op.clone(),
        });
        // La respuesta async vuelve por ActorEvent::OperationResult.
    }

    async fn handle_operation_result(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        op_id: u32,
        operation: Operation,
        result: OperationResult,
    ) -> AppResult<()> {
        let pending = self
            .operations
            .remove(&op_id)
            .expect("leader received the result of an unexisting operation");

        if pending.client_addr == self.address {
            if let Operation::Charge { .. } = operation {
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

        // Caso cliente externo: devolvemos el OperationResult completo.
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

    async fn handle_election(
        &mut self,
        connection: &mut Connection,
        candidate_id: u64,
        candidate_addr: SocketAddr,
    ) -> AppResult<RoleChange> {
        if self.id > candidate_id {
            println!(
                "[LEADER {}] Received Election from {}. Replying ElectionOk and starting own election",
                self.id,
                get_id_given_addr(candidate_addr)
            );
            let reply = Message::ElectionOk {
                responder_id: self.id,
            };
            if let Err(e) = connection.send(reply, &candidate_addr).await {
                println!(
                    "[LEADER {}] Failed to send ElectionOk to {:?}: {:?}",
                    self.id, candidate_addr, e
                );
            }

            // Igual que una réplica, corro mi propia elección (podría haber otro mayor).
            self.start_election(connection).await
        } else {
            println!(
                "[LEADER {}] Received Election from {} with >= id. Not replying.",
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
            "[LEADER {}] Received ElectionOk from {}. Waiting for Coordinator...",
            self.id, responder_id
        );
        // Sabemos que hay alguien más grande vivo; esperamos Coordinator.
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

        // Si alguien (con ID >= al mío) anuncia que es líder, me demoto a réplica.
        if leader_id >= self.id {
            println!(
                "[LEADER {}] Demoting to REPLICA, new leader is {} at {}",
                self.id, leader_id, leader_addr
            );
            return Ok(RoleChange::DemoteToReplica {
                new_leader_addr: leader_addr,
            });
        }

        // Si leader_id < self.id, ignoramos (no debería pasar en Bully correcto).
        println!(
            "[LEADER {}] Received Coordinator from id {} but I'm higher. Ignoring.",
            self.id, leader_id
        );
        Ok(RoleChange::None)
    }

    /// Igual lógica que en Replica, pero si no hay ID mayor y gano,
    /// simplemente reafirmo liderazgo (sin RoleChange).
    async fn start_election(
        &mut self,
        connection: &mut Connection,
    ) -> AppResult<RoleChange> {
        if self.election_in_progress {
            println!(
                "[LEADER {}] start_election called but election already in progress",
                self.id
            );
            return Ok(RoleChange::None);
        }

        println!(
            "[LEADER {}] Starting election, checking for higher-ID nodes...",
            self.id
        );

        let mut sent_any = false;

        for (&node_id, addr) in &self.cluster {
            if node_id > self.id {
                sent_any = true;
                println!(
                    "[LEADER {}] -> sending Election to {:?} (id={})",
                    self.id, addr, node_id
                );
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
            println!(
                "[LEADER {}] No higher-ID nodes found. Reasserting myself as COORDINATOR",
                self.id
            );
            self.election_in_progress = false;
            self.election_start = None;
            // Reafirmo liderazgo; no hay RoleChange.
            self.anounce_coordinator(connection).await
        }
    }

    /// Called when we receive a `Message::Join` through the generic
    /// Node dispatcher.
    async fn handle_join(
        &mut self,
        connection: &mut Connection,
        addr: SocketAddr,
    ) -> AppResult<()> {
        let node_id = get_id_given_addr(addr); // gracias ale :)
        self.cluster.insert(node_id, addr);
        let view_msg = Message::ClusterView {
            members: self
                .cluster
                .iter()
                .map(|(id, addr)| (*id, *addr))
                .collect(),
        };
        // le mandamos el clúster view al que entró
        connection.send(view_msg, &addr).await?;
        // le mandamos el update a las réplicas
        for replica_addr in self.cluster.values() {
            if *replica_addr == self.address || *replica_addr == addr {
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

        if node_id > self.id {
            println!(
                "[LEADER {}] New node with higher ID ({}) joined. Starting election.",
                self.id, node_id
            );
            self.start_election(connection).await?;
        }

        println!("[LEADER] New node joined: (ID={node_id})");
        println!("[LEADER] Current cluster members: {:?}", self.cluster.len());
        // NOTA: el nuevo nodo corre Bully desde Replica::start.
        Ok(())
    }

    async fn handle_cluster_update(
        &mut self,
        _connection: &mut Connection,
        _new_member: (u64, SocketAddr),
    ) {
        // only leaders send cluster updates
        todo!();
    }

    /// Called when we receive a `Message::ClusterView` through the
    /// generic Node dispatcher.
    async fn handle_cluster_view(
        &mut self,
        _connection: &mut Connection,
        _database: &mut Database,
        _members: Vec<(u64, SocketAddr)>,
    ) -> AppResult<()> {
        Ok(()) // leaders don't receive cluster view
    }

    /// Timeout de elección para Leader: si no hay ElectionOk en 2s
    /// y la elección seguía viva, reafirmo que sigo siendo líder.
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
            println!(
                "[LEADER {}] Election timeout exceeded. No higher node responded → reasserting myself as COORDINATOR",
                self.id
            );
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
        // descarto las operaciones pendientes; la replica no las necesita
        // en todo caso el cliente vuelve a preguntar
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
        // Old committed ops ya están en el actor system.

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

    /// Continue running as Leader after being promoted from Replica.
    /// Reuses existing state from the replica.
    pub async fn run_from_replica(
        mut leader: Leader,
        address: SocketAddr,
        coords: (f64, f64),
        max_conns: usize,
        pumps: usize,
    ) -> AppResult<()> {
        // Recreate resources that were consumed by replica.run()
        let db = super::database::Database::start().await?;
        let station = Station::start(pumps).await?;
        let mut connection = Connection::start(address, max_conns).await?;

        // Announce ourselves as the new coordinator to all members
        let coordinator_msg = Message::Coordinator {
            leader_id: leader.id,
            leader_addr: leader.address,
        };
        for (peer_id, peer_addr) in &leader.cluster {
            if *peer_id != leader.id {
                if let Err(e) = connection.send(coordinator_msg.clone(), peer_addr).await {
                    println!(
                        "[LEADER {}] Failed to send Coordinator (run_from_replica) to {:?}: {:?}",
                        leader.id, peer_addr, e
                    );
                }
            }
        }

        // Loop para manejar cambios de rol
        loop {
            let role_change = leader.run(connection, db, station).await?;

            match role_change {
                RoleChange::DemoteToReplica { new_leader_addr } => {
                    println!("[LEADER] Converting to Replica...");
                    let replica = leader.into_replica(new_leader_addr);
                    return Box::pin(Replica::run_from_leader(
                        replica, address, coords, max_conns, pumps,
                    ))
                    .await;
                }
                _ => {
                    return Ok(());
                }
            }
        }
    }

    /// Start a Leader node:
    ///
    /// - boots the ConnectionManager (TCP),
    /// - boots the actor system wrapped in `Database` (Actix in a dedicated thread),
    /// - starts the Station abstraction (which internally spawns the pump simulator),
    /// - seeds the cluster membership with self (other nodes will join dynamically),
    /// - then enters the main async event loop.
    pub async fn start(
        address: SocketAddr,
        coords: (f64, f64),
        max_conns: usize,
        pumps: usize,
    ) -> AppResult<()> {
        // Start the actor-based "database" subsystem (ActorRouter + Actix system).
        let db = super::database::Database::start().await?;

        // Start the shared Station abstraction (stdin-based simulator).
        let station = Station::start(pumps).await?;

        // Seed membership: leader only. Other members will be
        // registered via Join / ClusterView messages.
        let self_id = get_id_given_addr(address);
        let mut members: HashMap<u64, SocketAddr> = HashMap::new();
        members.insert(self_id, address);

        println!(
            "[LEADER {}] Starting leader node with address={}",
            self_id, address
        );

        // Start the TCP ConnectionManager for this node.
        let connection = Connection::start(address, max_conns).await?;

        let mut leader = Self {
            id: self_id,
            current_op_id: 0,
            coords,
            address,
            cluster: members,
            operations: HashMap::new(),
            is_offline: false,
            offline_queue: VecDeque::new(),
            election_in_progress: false,
            election_start: None,
        };

        // Loop para manejar cambios de rol
        loop {
            let role_change = leader.run(connection, db, station).await?;

            match role_change {
                RoleChange::DemoteToReplica { new_leader_addr } => {
                    println!("[LEADER] Converting to Replica...");
                    let replica = leader.into_replica(new_leader_addr);
                    return Box::pin(Replica::run_from_leader(
                        replica, address, coords, max_conns, pumps,
                    ))
                    .await;
                }
                _ => {
                    return Ok(());
                }
            }
        }
    }

    #[cfg(test)]
    pub fn test_get_id(&self) -> u64 {
        self.id
    }
    #[cfg(test)]
    pub fn test_get_members(&self) -> HashMap<u64, SocketAddr> {
        self.cluster.clone()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::{
        net::{IpAddr, Ipv4Addr},
        thread::{self, JoinHandle},
        time::Duration,
    };
    use tokio::task;

    fn spawn_leader_in_thread(leader_addr: SocketAddr) -> JoinHandle<()> {
        thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    task::spawn(async move {
                        tokio::time::timeout(
                            Duration::from_secs(1),
                            Leader::start(leader_addr, (0.0, 0.0), 10, 1),
                        )
                        .await
                        .unwrap()
                        .unwrap()
                    })
                    .await
                    .unwrap()
                });
        })
    }

    #[tokio::test]
    #[ignore = "se queda en timeout ya que no hay replica corriendo"]
    async fn test_leader_sends_log_msg_when_handling_a_request() {
        let leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12362);
        let replica_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12363);
        let _leader_handle = spawn_leader_in_thread(leader_addr);
        thread::sleep(Duration::from_secs(1)); // fuerzo context switch
        let mut replica = Connection::start(replica_addr, 1).await.unwrap();
        replica
            .send(Message::Join { addr: replica_addr }, &leader_addr)
            .await
            .unwrap();

        let received = replica.recv().await.unwrap();
        let expected = Message::ClusterView {
            members: vec![
                (get_id_given_addr(leader_addr), leader_addr),
                (get_id_given_addr(replica_addr), replica_addr),
            ],
        };
        // FIXME: este test a veces falla porq las addr vienen al revés, comparar sets
        assert_eq!(received, expected);
    }

    #[tokio::test]
    async fn test_leader_sends_log_to_two_replicas_when_receiving_a_request() {
        // init del test
        let leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12364);
        let replica1_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12365);
        let replica2_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12366);
        let client_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12367);
        let _leader_handle = spawn_leader_in_thread(leader_addr);
        thread::sleep(Duration::from_secs(1)); // fuerzo context switch
        let mut replica1 = Connection::start(replica1_addr, 1).await.unwrap();
        let mut replica2 = Connection::start(replica2_addr, 1).await.unwrap();
        // joineo las reps
        replica1
            .send(
                Message::Join {
                    addr: replica1_addr,
                },
                &leader_addr,
            )
            .await
            .unwrap();
        // vuelo los msjs de cluster view
        let _ = replica1.recv().await.unwrap();
        thread::sleep(Duration::from_secs(1));
        replica2
            .send(
                Message::Join {
                    addr: replica2_addr,
                },
                &leader_addr,
            )
            .await
            .unwrap();
        let _ = replica2.recv().await.unwrap();
        // ahora la salsa
        let op = Operation::Charge {
            account_id: 10,
            card_id: 1500,
            amount: 134_989.5,
            from_offline_station: false,
        };
        let mut client = Connection::start(client_addr, 1).await.unwrap();
        client
            .send(
                Message::Request {
                    req_id: 0,
                    op: op.clone(),
                    addr: client_addr,
                },
                &leader_addr,
            )
            .await
            .unwrap(); // request al líder
        let expected_log = Message::Log { op_id: 0, op };
        assert_eq!(replica1.recv().await.unwrap(), expected_log);
        assert_eq!(replica2.recv().await.unwrap(), expected_log);
    }
}

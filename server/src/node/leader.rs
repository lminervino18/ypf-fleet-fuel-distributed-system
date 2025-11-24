use super::database::{Database, DatabaseCmd}; // use the Database abstraction
use super::node::Node;
use super::pending_operatoin::PendingOperation;
use crate::{
    errors::{AppError, AppResult},
    node::utils::get_id_given_addr,
};
use common::operation::Operation;
use common::operation_result::{ChargeResult, OperationResult};
use common::{Connection, Message, NodeToStationMsg, Station};
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
};

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
    members: HashMap<u64, SocketAddr>,
    operations: HashMap<u32, PendingOperation>,
    is_offline: bool,
    offline_queue: VecDeque<Operation>,
    // NOTE: we no longer store Database here; it is owned by `run()`.
}

// ==========================================================
// Node trait implementation
// ==========================================================
impl Node for Leader {
    fn is_offline(&self) -> bool {
        self.is_offline
    }

    fn log_offline_operation(&mut self, op: Operation) {
        self.offline_queue.push_back(op);
    }

    fn get_address(&self) -> SocketAddr {
        self.address
    }

    async fn handle_request(
        &mut self,
        connection: &mut Connection,
        req_id: u32,
        op: Operation,
        client_addr: SocketAddr,
    ) -> AppResult<()> {
        // Store the operation locally.
        println!("[LEADER] Handling new request from {:?}: {:?}", client_addr, op);
        self.operations.insert(
            self.current_op_id,
            PendingOperation::new(op.clone(), client_addr, req_id),
        );

        // Build the log message to replicate.
        // Broadcast the log to all known members except self.
        for (node_id, addr) in &self.members {
            if *node_id == self.id {
                continue;
            }

            connection
                .send(
                    Message::Log {
                        op_id: self.current_op_id,
                        op: op.clone(),
                    },
                    addr,
                )
                .await?;
        }

        self.current_op_id += 1;

        Ok(())
    }

    async fn handle_response(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        req_id: u32,
        op_result: OperationResult,
    ) -> AppResult<()> {
        todo!()
    }

    async fn handle_role_query(
        &mut self,
        connection: &mut Connection,
    ) -> AppResult<()>{
        let role_msg = Message::RoleResponse {
            node_id: get_id_given_addr(self.address),
            role: common::NodeRole::Leader,
        };
        connection.send(role_msg, &self.address).await?;
        Ok(())
    }
    
    async fn handle_log(
        &mut self,
        _connection: &mut Connection,
        _db: &mut Database,
        _op_id: u32,
        _op: Operation,
    ) {
        // Leader should not receive Log messages.
        todo!(); // TODO: leader should not receive any Log messages.
    }

    // CHANGED SIGNATURE: now receives `db: &mut Database`
    async fn handle_ack(&mut self, _connection: &mut Connection, db: &mut Database, op_id: u32) {

        let Some(pending) = self.operations.get_mut(&op_id) else {
            return; // TODO: handle this case (unknown op_id).
        };

        println!("[LEADER] Received ACK for op_id {}", op_id);
        pending.ack_count += 1;
        if pending.ack_count != (self.members.len() - 1) / 2 {
            return;
        }

        println!(
            "[LEADER] Majority ACKs reached for op_id {} ({} / {})",
            op_id,
            pending.ack_count,
            self.members.len()
        );
        // Mayoría alcanzada → ejecutar la operación en el mundo de actores.
        //
        // Instead of talking directly to ActorRouter / RouterCmd, we now
        // send a high-level DatabaseCmd to the Database abstraction.
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
    ) {
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

                    let _ = station.send(msg).await;
                } else {
                    // Mismatch raro entre Operation y OperationResult.
                    // Podrías loggear algo acá si querés.
                }
            } else {
                // Si en el futuro la estación dispara otras operaciones
                // (por ejemplo algún Query), habría que manejarlas acá.
            }

            return;
        }

        // Caso cliente externo: devolvemos el OperationResult completo.
        let _ = connection
            .send(
                Message::Response {
                    req_id: pending.request_id,
                    op_result: result,
                },
                &pending.client_addr,
            )
            .await;
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
    async fn handle_join(&mut self, connection: &mut Connection, addr: SocketAddr) {
        let node_id = get_id_given_addr(addr); // gracias ale :)
        self.members.insert(node_id, addr);
        let view_msg = Message::ClusterView {
            members: self.members.iter().map(|(id, addr)| (*id, *addr)).collect(),
        };
        // le mandamos el clúster view al que entró
        connection.send(view_msg, &addr).await.unwrap(); // TODO: handle this errors!!
                                                         // y le mandamos sólo el nuevo al resto de las réplicas
        for replica_addr in self.members.values() {
            if *replica_addr == self.address || *replica_addr == addr {
                continue;
            }
            let _ = connection
                .send(
                    Message::ClusterUpdate {
                        new_member: (node_id, addr),
                    },
                    replica_addr,
                )
                .await;
        }
        println!("[LEADER] New node joined: {:?} (ID={})", addr, node_id);
        println!("[LEADER] Current members: {:?}", self.members.len());
        // TODO: una vez q le avisaste a todas las réplicas hay que llamar a una elección de líder
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
    async fn handle_cluster_view(&mut self, _members: Vec<(u64, SocketAddr)>) {
        // leader shouldn't receive cluster_view messages
        todo!();
    }
}

impl Leader {
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

        // Start the TCP ConnectionManager for this node.
        let connection = Connection::start(address, max_conns).await?;

        let mut leader = Self {
            id: self_id,
            current_op_id: 0,
            coords,
            address,
            members,
            operations: HashMap::new(),
            is_offline: false,
            offline_queue: VecDeque::new(),
        };

        // Now `run` takes ownership of `connection`, `db` and `station`,
        // exactly like with Connection and Station.
        leader.run(connection, db, station).await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn test_leader_sends_log_msg_when_handling_a_request() {
        let leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12362);
        let client_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12363);
        let replica_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12364);

        Leader::start(leader_addr, (0.0, 0.0), 1, 1).await.unwrap();

        let mut client = Connection::start(client_addr, 1).await.unwrap();
        let mut replica = Connection::start(replica_addr, 1).await.unwrap();

        replica
            .send(Message::Join { addr: replica_addr }, &leader_addr)
            .await
            .unwrap();

        let op = Operation::Charge {
            account_id: 13,
            card_id: 14,
            amount: 1500.5,
            from_offline_station: false,
        };

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
            .unwrap();

        let expected = Message::Log { op_id: 0, op };
        let received = replica.recv().await.unwrap();
        assert_eq!(received, expected);
    }
}

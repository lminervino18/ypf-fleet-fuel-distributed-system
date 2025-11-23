use super::actors::{actor_router::ActorRouter, ActorEvent, RouterCmd};
use super::node::Node;
use super::pending_operatoin::PendingOperation;
use crate::errors::VerifyError;
use crate::{
    errors::{AppError, AppResult},
    node::utils::get_id_given_addr,
};
use actix::{Actor, Addr};
use common::operation::Operation;
use common::{Connection, Message, NodeToStationMsg, Station};
use std::{
    collections::{HashMap, VecDeque},
    future::pending,
    net::SocketAddr,
};
use tokio::sync::{mpsc, oneshot};

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
/// - the local ActorRouter (Actix),
/// - the Station abstraction (which internally runs the stdin-driven pump simulator).
///
/// ONLINE behavior (default):
/// --------------------------
/// For a `Charge` coming from a pump the flow is:
///
/// 1. Station → Leader: `StationToNodeMsg::ChargeRequest`.
/// 2. Leader builds `ActorOperation::Charge { .., from_offline_station: false }`
///    and sends `RouterCmd::Execute { op_id, operation }` to ActorRouter.
/// 3. ActorRouter/actors run verification + apply internally.
/// 4. Once finished, ActorRouter emits a single
///    `ActorEvent::OperationResult { op_id, operation, success, error }`.
/// 5. Leader translates that into a `NodeToStationMsg::ChargeResult`
///    and sends it back to the station, removing the pending request.
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
///       - **not** sent to the ActorRouter yet.
/// - Station → Leader: `StationToNodeMsg::ConnectNode`
///   sets `is_offline = false` and **replays** all queued charges to
///   the ActorRouter with `from_offline_station = true`.
pub struct Leader {
    id: u64,
    current_op_id: u32,
    coords: (f64, f64),
    address: SocketAddr,
    members: HashMap<u64, SocketAddr>,
    operations: HashMap<u32, PendingOperation>,
    is_offline: bool,
    offline_queue: VecDeque<Operation>,
    station_requests: HashMap<u32, (usize, StationPendingCharge)>,
    router: Addr<ActorRouter>,
}

// ==========================================================
// Node trait implementation
// ==========================================================
impl Node for Leader {
    fn is_offline(&self) -> bool {
        self.is_offline
    }
    fn log_offline_opeartion(&mut self, op: Operation) {
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
        self.current_op_id += 1;
        self.operations.insert(
            self.current_op_id,
            PendingOperation::new(op.clone(), client_addr, req_id),
        );

        // Build the log message to replicate.
        let msg = Message::Log {
            op_id: self.current_op_id,
            op,
        };
        // Broadcast the log to all known members except self.
        for (node_id, addr) in &self.members {
            if *node_id == self.id {
                continue;
            }

            connection.send(msg.clone(), addr).await?;
        }

        Ok(())
    }

    async fn handle_log(&mut self, _connection: &mut Connection, _op_id: u32, _op: Operation) {
        // Leader should not receive Log messages.
        todo!(); // TODO: leader should not receive any Log messages.
    }

    async fn handle_ack(&mut self, _connection: &mut Connection, op_id: u32) {
        let Some(pending) = self.operations.get_mut(&op_id) else {
            todo!(); // TODO: handle this case (unknown op_id).
        };

        pending.ack_count += 1;
        if pending.ack_count <= (self.members.len() - 1) / 2 {
            return;
        }

        self.router.send(RouterCmd::Execute {
            op_id,
            operation: pending.op.clone(),
        });
        // router response is async so it's handled separately below
    }

    async fn handle_operation_result(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        op_id: u32,
        operation: Operation,
        success: bool,
        error: Option<VerifyError>,
    ) {
        let Some(pending) = self.operations.remove(&op_id) else {
            todo!()
        };

        if pending.client_addr == self.address {
            station
                .send(NodeToStationMsg::ChargeResult {
                    request_id: pending.request_id,
                    allowed: success,
                    error,
                })
                .await
                .unwrap();
            return;
        }

        connection
            .send(
                Message::Response {
                    req_id: pending.request_id,
                    result: error,
                },
                &pending.client_addr,
            )
            .await
            .unwrap();
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
            let _ = connection
                .send(
                    Message::ClusterUpdate {
                        new_member: (node_id, addr),
                    },
                    replica_addr,
                )
                .await;
        }
        // TODO: una vez q le avisaste a todas las réplicas hay que llamar a una elección de líder
    }

    async fn handle_cluster_update(
        &mut self,
        connection: &mut Connection,
        new_member: (u64, SocketAddr),
    ) {
        // only leaders send cluster updates
        todo!();
    }

    /// Called when we receive a `Message::ClusterView` through the
    /// generic Node dispatcher.
    async fn handle_cluster_view(&mut self, members: Vec<(u64, SocketAddr)>) {
        // leader shouldn't receive cluster_view messages
        todo!();
    }
}

impl Leader {
    /// Start a Leader node:
    ///
    /// - boots the ConnectionManager (TCP),
    /// - boots the ActorRouter in a dedicated Actix system thread,
    /// - starts the Station abstraction (which internally spawns the pump simulator),
    /// - seeds the cluster membership with self (other nodes will join dynamically),
    /// - then enters the main async event loop.
    pub async fn start(
        address: SocketAddr,
        coords: (f64, f64),
        max_conns: usize,
        pumps: usize,
    ) -> AppResult<()> {
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();
        // Spawn a thread for Actix that runs inside a Tokio runtime.
        std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(actor_tx).start();
                if router_tx.send(router.clone()).is_err() {
                    // println!("[ERROR] Failed to deliver ActorRouter addr to Leader");
                }

                pending::<()>().await; // Keep the Actix system running indefinitely.
            });
        });
        // Start the shared Station abstraction (stdin-based simulator).
        let station = Station::start(pumps).await?;
        // Seed membership: leader only. Other members will be
        // registered via Join / ClusterView messages.
        let self_id = get_id_given_addr(address);
        let mut members: HashMap<u64, SocketAddr> = HashMap::new();
        members.insert(self_id, address);
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
            station_requests: HashMap::new(),
            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive ActorRouter address: {e}"),
            })?,
        };

        leader.run(connection, actor_rx, station).await
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

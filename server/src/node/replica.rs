use super::{
    actors::{actor_router::ActorRouter, ActorEvent},
    election::bully::Bully,
    message::Message,
    network::Connection,
    node::Node,
    operation::Operation,
    utils::get_id_given_addr,
};
use crate::errors::{AppError, AppResult};
use actix::{Actor, Addr};
use common::{Station, StationToNodeMsg};
use std::{
    collections::{HashMap, VecDeque},
    future::pending,
    net::SocketAddr,
    sync::Arc,
};
use tokio::sync::{mpsc, oneshot, Mutex};

/// Replica node.
///
/// Same wiring as the Leader, but intended to be a follower in the
/// distributed system. For station-originated charges it uses the
/// exact same Execute (verify + apply inside actors) flow as the Leader
/// when ONLINE, and the same offline queueing behavior when OFFLINE.
///
/// Internal state for a charge coming from a station pump while the node is
/// ONLINE (kept locally in replica implementation).
#[derive(Debug, Clone)]
struct StationPendingCharge {
    account_id: u64,
    card_id: u64,
    amount: f64,
}

#[derive(Debug, Clone)]
struct OfflineQueuedCharge {
    request_id: u64,
    account_id: u64,
    card_id: u64,
    amount: f64,
}

pub struct Replica {
    id: u64,
    coords: (f64, f64),
    max_conns: usize,
    address: SocketAddr,
    /// Current leader address (updated on Coordinator messages).
    leader_addr: SocketAddr,
    /// Current cluster membership: node_id -> address (including self & leader).
    members: HashMap<u64, SocketAddr>,
    bully: Arc<Mutex<Bully>>,
    operations: HashMap<u32, Operation>,
    // connection: Connection,
    // actor_rx: mpsc::Receiver<ActorEvent>,
    is_offline: bool,
    offline_queue: VecDeque<OfflineQueuedCharge>,
    /// High-level station abstraction (internally runs the simulator task).
    // station: Station,
    /// Pending station-originated charges while ONLINE.
    station_requests: HashMap<u64, StationPendingCharge>,
    router: Addr<ActorRouter>,
}

impl Node for Replica {
    async fn handle_charge_request(
        &mut self,
        _station: &mut Station,
        _pump_id: usize,
        _account_id: u64,
        _card_id: u64,
        _amount: f32,
        _request_id: u64,
    ) {
        // Not wired yet: replicas do not handle station-originated charges directly.
        todo!();
    }

    async fn handle_operation_result(
        &mut self,
        _op_id: u32,
        _operation: Operation,
        _success: bool,
        _error: Option<crate::errors::VerifyError>,
    ) {
        // This will be wired if we ever have replicas executing operations locally.
        todo!();
    }

    async fn handle_request(
        &mut self,
        connection: &mut Connection,
        op_id: u32,
        op: Operation,
        addr: SocketAddr,
    ) -> AppResult<()> {
        // Redirect to leader node.
        connection
            .send(Message::Request { op_id, addr, op }, &self.leader_addr)
            .await
            .unwrap();
        todo!();
    }

    async fn handle_log(&mut self, connection: &mut Connection, op_id: u32, new_op: Operation) {
        // Store the replicated operation locally and acknowledge back to the leader.
        self.operations.insert(op_id, new_op);
        // self.commit_operation(new_op_id - 1).await; // TODO: this logic should be in actors module.
        connection
            .send(Message::Ack { op_id }, &self.leader_addr)
            .await
            .unwrap();
        todo!();
    }

    async fn handle_ack(&mut self, _connection: &mut Connection, _id: u32) {
        // Replicas should not receive any ACK messages.
        todo!();
    }

    async fn handle_actor_event(&mut self, event: ActorEvent) {
        // Same behavior as the default Node implementation:
        match event {
            ActorEvent::OperationResult {
                op_id,
                operation,
                success,
                error,
            } => {
                self.handle_operation_result(op_id, operation, success, error)
                    .await;
            }
            _ => {
                todo!();
            }
        }
    }

    async fn handle_station_msg(&mut self, station: &mut Station, msg: StationToNodeMsg) {
        // Same behavior as the default Node implementation:
        match msg {
            StationToNodeMsg::ChargeRequest {
                pump_id,
                account_id,
                card_id,
                amount,
                request_id,
            } => {
                self.handle_charge_request(
                    station, pump_id, account_id, card_id, amount, request_id,
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

        // TODO:
        // The replica now becomes leader, presumably by changing its role
        // with the NodeRole enum, but this would require further unification
        // between replica.rs and leader.rs.
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

    /// Replicas ignore Join themselves — new nodes should always Join via the current leader.
    async fn handle_join(
        &mut self,
        _connection: &mut Connection,
        _node_id: u64,
        _addr: SocketAddr,
    ) {
        // No-op for replicas.
    }

    /// Replicas update their local membership when receiving a ClusterView snapshot.
    async fn handle_cluster_view(&mut self, members: Vec<(u64, SocketAddr)>) {
        self.handle_cluster_view_message(members);
    }
}

impl Replica {
    /// - boots the ConnectionManager (TCP),
    /// - boots the ActorRouter in a dedicated Actix system thread,
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
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();

        // Spawns a thread for Actix that runs inside a Tokio runtime.
        std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(actor_tx).start();
                if router_tx.send(router.clone()).is_err() {
                    // println!("[ERROR] Failed to deliver ActorRouter addr to Replica");
                }

                pending::<()>().await; // Keep the Actix system running indefinitely.
            });
        });

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
            max_conns,
            address,
            leader_addr,
            members,
            bully: Arc::new(Mutex::new(Bully::new(id, address))),
            operations: HashMap::new(),
            is_offline: false,
            offline_queue: VecDeque::new(),
            station_requests: HashMap::new(),
            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive ActorRouter address: {e}"),
            })?,
        };

        // Immediately announce ourselves to the leader so it can
        // rebroadcast a fresh ClusterView (membership snapshot).
        let join_msg = Message::Join {
            node_id: replica.id,
            addr: replica.address,
        };
        let _ = connection.send(join_msg, &replica.leader_addr).await;

        replica.run(connection, actor_rx, station).await
    }

    /// Handle an incoming ClusterView snapshot:
    /// - replace our current membership with the provided one,
    /// - but ensure we always include ourselves with our known address.
    pub fn handle_cluster_view_message(&mut self, members: Vec<(u64, SocketAddr)>) {
        self.members.clear();

        for (id, addr) in members {
            self.members.insert(id, addr);
        }

        // Ensure we are present with the correct address.
        self.members.insert(self.id, self.address);
    }
}

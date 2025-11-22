use super::{
    actors::{actor_router::ActorRouter, ActorEvent},
    election::bully::Bully,
    message::Message,
    network::Connection,
    node::Node,
    operation::Operation,
    utils::get_id_given_addr,
};
use crate::{
    errors::{AppError, AppResult},
};
use common::{Station, StationToNodeMsg, NodeToStationMsg};
use actix::{Actor, Addr};
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
    leader_addr: SocketAddr,
    other_replicas: Vec<SocketAddr>,
    bully: Arc<Mutex<Bully>>,
    operations: HashMap<u32, Operation>,
    connection: Connection,
    actor_rx: mpsc::Receiver<ActorEvent>,
    is_offline: bool,
    offline_queue: VecDeque<OfflineQueuedCharge>,
    /// High-level station abstraction (internally runs the simulator task).
    station: Station,
    /// Pending station-originated charges while ONLINE.
    station_requests: HashMap<u64, StationPendingCharge>,
    router: Addr<ActorRouter>,
}

impl Node for Replica {
    async fn handle_charge_request(
        &mut self,
        pump_id: usize,
        account_id: u64,
        card_id: u64,
        amount: f32,
        request_id: u64,
    ) {
        todo!();
    }

    async fn recv_station_message(&mut self) -> Option<StationToNodeMsg> {
        self.station.recv().await
    }

    async fn handle_operation_result(
        &mut self,
        op_id: u32,
        operation: Operation,
        success: bool,
        error: Option<crate::errors::VerifyError>,
    ) {
        todo!();
    }

    async fn handle_request(
        &mut self,
        op_id: u32,
        op: Operation,
        addr: SocketAddr,
    ) -> AppResult<()> {
        // Redirect to leader node.
        self.connection
            .send(Message::Request { op_id, addr, op }, &self.leader_addr)
            .await
            .unwrap();
        todo!();
    }

    async fn handle_log(&mut self, op_id: u32, new_op: Operation) {
        self.operations.insert(op_id, new_op);
        // self.commit_operation(new_op_id - 1).await; // TODO: this logic should be in actors module.
        self.connection
            .send(Message::Ack { op_id }, &self.leader_addr)
            .await
            .unwrap();
        todo!();
    }

    async fn handle_ack(&mut self, _id: u32) {
        todo!(); // TODO: replicas should not receive any ACK messages.
    }

    async fn recv_node_msg(&mut self) -> AppResult<Message> {
        self.connection.recv().await
    }

    async fn recv_actor_event(&mut self) -> Option<ActorEvent> {
        todo!()
    }

    async fn handle_actor_event(&mut self, event: ActorEvent) {
        todo!()
    }

    async fn handle_station_msg(&mut self, msg: StationToNodeMsg) {
        todo!()
    }

    async fn handle_election(&mut self, candidate_id: u64, candidate_addr: SocketAddr) {
        // If we have higher id, reply ElectionOk and start our own election.
        let should_reply = {
            let b = self.bully.lock().await;
            b.should_reply_ok(candidate_id)
        };
        if should_reply {
            let reply = Message::ElectionOk {
                responder_id: self.id,
            };
            let _ = self.connection.send(reply, &candidate_addr).await;

            // Start own election immediately.
            self.start_election().await;
        }
    }

    async fn handle_election_ok(&mut self, responder_id: u64) {
        let mut b = self.bully.lock().await;
        b.on_election_ok(responder_id);
    }

    async fn handle_coordinator(&mut self, leader_id: u64, leader_addr: SocketAddr) {
        let mut b = self.bully.lock().await;
        b.on_coordinator(leader_id, leader_addr);

        // TODO:
        // The replica now becomes leader, presumably by changing its role
        // with the NodeRole enum, but this would require further unification
        // between replica.rs and leader.rs.
    }

    async fn start_election(&mut self) {
        // Build peer_ids map: include leader and other replicas.
        let mut peer_ids = HashMap::new();
        peer_ids.insert(get_id_given_addr(self.leader_addr), self.leader_addr);
        for addr in &self.other_replicas {
            peer_ids.insert(get_id_given_addr(*addr), *addr);
        }

        crate::node::election::bully::conduct_election(
            &self.bully,
            &mut self.connection,
            peer_ids,
            self.id,
            self.address,
        )
        .await;
    }
}

impl Replica {
    /// - boots the ConnectionManager (TCP),
    /// - boots the ActorRouter in a dedicated Actix system thread,
    /// - starts the Station abstraction (which internally spawns the simulator with `pumps` pumps on stdin),
    /// - then enters the main async event loop.
    pub async fn start(
        address: SocketAddr,
        leader_addr: SocketAddr,
        coords: (f64, f64),
        other_replicas: Vec<SocketAddr>,
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

        let id = get_id_given_addr(address);
        let mut replica = Self {
            id,
            coords,
            max_conns,
            address,
            leader_addr,
            other_replicas,
            bully: Arc::new(Mutex::new(Bully::new(id, address))),
            operations: HashMap::new(),
            connection: Connection::start(address, max_conns).await?,
            actor_rx,
            is_offline: false,
            offline_queue: VecDeque::new(),
            station,
            station_requests: HashMap::new(),
            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive ActorRouter address: {e}"),
            })?,
        };

        replica.run().await
    }
}

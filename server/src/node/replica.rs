use super::{message::Message, network::Connection, node::Node, operation::Operation};
use crate::{
    actors::actor_router::ActorRouter,
    actors::messages::ActorEvent,
    errors::{AppError, AppResult},
    node::election::bully::Bully,
    node::station::{NodeToStationMsg, StationToNodeMsg},
};
use actix::{Addr, Actor};
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
/// exact same Execute (verify + apply interno) flow as the Leader
/// when ONLINE, and the same offline queueing behavior when OFFLINE.
/// Internal state for a charge coming from a station pump while the node is
/// ONLINE. (kept locally in replica implementation)
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
    station_cmd_rx: mpsc::Receiver<StationToNodeMsg>,
    station_result_tx: mpsc::Sender<NodeToStationMsg>,
    station_requests: HashMap<u64, StationPendingCharge>,
    router: Addr<ActorRouter>,
}

impl Node for Replica {
    async fn handle_request(&mut self, op: Operation, addr: SocketAddr) {
        // redirect to leader node
        let _ = self.connection
            .send(Message::Request { op, addr }, &self.leader_addr).await;
    }

    async fn handle_log(&mut self, new_op: Operation) {
        let new_op_id = new_op.id;
        self.operations.insert(new_op_id, new_op);
        // self.commit_operation(new_op_id - 1).await; // TODO: this logic should be in actors mod
        let _ = self.connection
            .send(Message::Ack { id: new_op_id }, &self.leader_addr).await;
    }

    async fn handle_ack(&mut self, _id: u32) {
        todo!(); // TODO: replicas should not receive any ACK msgs
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
        let should_reply = { let b = self.bully.lock().await; b.should_reply_ok(candidate_id) };
        if should_reply {
            let reply = Message::ElectionOk {};
            // reply using direct connection
            let _ = self.connection.send(reply, &candidate_addr).await;

            // start own election in background; use the connection's outgoing sender (refactor?)
            let mut peers = Vec::with_capacity(self.other_replicas.len() + 1);
            peers.extend(self.other_replicas.iter().cloned());
            peers.push(self.leader_addr);

            let bully = self.bully.clone();
            let outgoing = self.connection.outgoing_sender();
            let id = self.id;
            let address = self.address;
            tokio::spawn(async move {
                crate::node::election::bully::conduct_election(
                    bully, outgoing, peers, id, address
                ).await;
            });
        }
    }

    async fn handle_election_ok(&mut self) {
        let mut b = self.bully.lock().await;
        b.on_election_ok();
    }

    async fn handle_coordinator(&mut self, leader_id: u64, leader_addr: SocketAddr) {
        let mut b = self.bully.lock().await;
        b.on_coordinator(leader_id, leader_addr);
    }

    async fn start_election(&mut self) {
        // Build peer list: include leader and other replicas
        let mut peers = Vec::with_capacity(self.other_replicas.len() + 1);
        peers.extend(self.other_replicas.iter().cloned());
        peers.push(self.leader_addr);
        let bully = self.bully.clone();
        let outgoing = self.connection.outgoing_sender();
        let id = self.id;
        let address = self.address;

        tokio::spawn(async move {
            crate::node::election::bully::conduct_election(
                bully, outgoing, peers, id, address
            ).await;
        });
    }

    
}

impl Replica {
    /// - boots the ConnectionManager (TCP),
    /// - boots the ActorRouter in a dedicated Actix system thread,
    /// - spawns the station simulator with `pumps` pumps on stdin,
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
        let (station_cmd_tx, station_cmd_rx) = mpsc::channel::<StationToNodeMsg>(128);
        let (station_result_tx, station_result_rx) = mpsc::channel::<NodeToStationMsg>(128);
        let station_cmd_tx_for_station = station_cmd_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::node::station::run_station_simulator(
                pumps,
                station_cmd_tx_for_station,
                station_result_rx,
            )
            .await
            {
                // println!("[Station] simulator error: {e:?}");
            }
        });

        std::thread::spawn(move || {
            // spawns a thread for Actix that runs inside a Tokio runtime 
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(actor_tx).start();
                if router_tx.send(router.clone()).is_err() {
                    // println!("[ERROR] Failed to deliver ActorRouter addr to Replica");
                }

                pending::<()>().await; // Keep the actix system running indefinitely
            });
        });

        let id = rand::random::<u64>();
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
            station_cmd_rx,
            station_result_tx,
            station_requests: HashMap::new(),
            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive ActorRouter address: {e}"),
            })?,
        };

        replica.run().await
    }
}
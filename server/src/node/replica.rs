use super::{message::Message, network::Connection, node::Node, operation::Operation};
use crate::{
    actors::actor_router::ActorRouter,
    actors::messages::ActorEvent,
    errors::{AppError, AppResult},
    node::station::{NodeToStationMsg, StationToNodeMsg},
};
use actix::{Addr, Actor};
use std::{
    collections::{HashMap, VecDeque},
    future::pending,
    net::SocketAddr,
};
use tokio::sync::{mpsc, oneshot};

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
    current_term: u64,
    voted_for: Option<u64>,
    election_in_progress: bool,
    votes_received: usize,
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
        self.connection
            .send(Message::Request { op, addr }, &self.leader_addr);
    }

    async fn handle_log(&mut self, new_op: Operation) {
        let new_op_id = new_op.id;
        self.operations.insert(new_op_id, new_op);
        // self.commit_operation(new_op_id - 1).await; // TODO: this logic should be in actors mod
        self.connection
            .send(Message::Ack { id: new_op_id }, &self.leader_addr);
    }

    async fn handle_ack(&mut self, _id: u32) {
        todo!(); // TODO: replicas should not receive any ACK msgs
    }

    async fn recv_node_msg(&mut self) -> AppResult<Message> {
        self.connection.recv().await
    }

    async fn recv_actor_event(&mut self) -> Option<ActorEvent> {
        self.actor_rx.recv().await
    }

    async fn handle_actor_event(&mut self, _event: ActorEvent) {
        // Replica-specific actor events handling will go here.
    }

    async fn handle_station_msg(&mut self, _msg: StationToNodeMsg) {
        // Replicas don't service station pumps; ignore or log.
    }

    async fn handle_vote_request(&mut self, term: u64, candidate_id: u64, candidate_addr: SocketAddr) {
        // Simple voting rule: if term is newer, reset vote state. If we haven't voted this term,
        // grant vote and record voter.
        if term > self.current_term {
            self.current_term = term;
            self.voted_for = None;
        }

        let mut grant = false;
        if self.voted_for.is_none() || self.voted_for == Some(candidate_id) {
            self.voted_for = Some(candidate_id);
            grant = true;
        }

        let reply = Message::Vote {
            term: self.current_term,
            voter_id: self.id,
            voter_addr: self.address,
            granted: grant,
        };

        self.connection.send(reply, &candidate_addr);
    }

    async fn handle_vote(&mut self, term: u64, _voter_id: u64, _voter_addr: SocketAddr, granted: bool) {
        if !self.election_in_progress {
            return;
        }

        if term != self.current_term {
            // stale vote
            return;
        }

        if granted {
            self.votes_received += 1;
            let total_peers = 1 + self.other_replicas.len(); // candidate + replicas (leader also counted separately)
            // majority: > total_peers/2
            if self.votes_received > total_peers / 2 {
                // Won election
                self.election_in_progress = false;
                // For now we only log detection of leadership. Transition is future work.
                // println!("[Replica:{}] WON election for term {}", self.id, self.current_term);
            }
        }
    }

    async fn start_election(&mut self) {
        if self.election_in_progress {
            return;
        }

        self.election_in_progress = true;
        self.current_term = self.current_term.saturating_add(1);
        self.voted_for = Some(self.id);
        self.votes_received = 1; // vote for self

        let msg = Message::RequestVote {
            term: self.current_term,
            candidate_id: self.id,
            candidate_addr: self.address,
        };

        // send to leader and other replicas
        let _ = self.connection.send(msg.clone(), &self.leader_addr);
        for peer in &self.other_replicas {
            let _ = self.connection.send(msg.clone(), peer);
        }
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

        let mut replica = Self {
            id: rand::random::<u64>(),
            coords,
            max_conns,
            address,
            leader_addr,
            other_replicas,
            current_term: 0,
            voted_for: None,
            election_in_progress: false,
            votes_received: 0,
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
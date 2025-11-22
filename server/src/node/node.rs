use super::{message::Message, operation::Operation, station::StationToNodeMsg};
use crate::{actors::ActorEvent, errors::AppResult};
use std::net::SocketAddr;
use tokio::select;
use tokio::time::{sleep, Duration};
use rand::Rng;

/// Role of a node in the YPF Ruta distributed system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeRole {
    Leader,
    Replica,
    Station,
}

pub trait Node {
    async fn handle_request(&mut self, op: Operation, client_addr: SocketAddr);
    async fn handle_log(&mut self, op: Operation);
    async fn handle_ack(&mut self, id: u32);
    async fn recv_node_msg(&mut self) -> AppResult<Message>;
    async fn recv_actor_event(&mut self) -> Option<ActorEvent>;
    async fn handle_actor_event(&mut self, event: ActorEvent);
    async fn handle_vote_request(&mut self, term: u64, candidate_id: u64, candidate_addr: SocketAddr);
    async fn handle_vote(&mut self, term: u64, voter_id: u64, voter_addr: SocketAddr, granted: bool);
    async fn handle_station_msg(&mut self, msg: StationToNodeMsg);
    async fn start_election(&mut self);

    async fn handle_node_msg(&mut self, msg: Message) {
        match msg {
            Message::Request { op, addr } => {
                self.handle_request(op, addr).await;
            }
            Message::Log { op } => {
                self.handle_log(op).await;
            }
            Message::Ack { id } => {
                self.handle_ack(id).await;
            }
            Message::RequestVote { term, candidate_id, candidate_addr } => {
                self.handle_vote_request(term, candidate_id, candidate_addr).await;
            }
            Message::Vote { term, voter_id, voter_addr, granted } => {
                self.handle_vote(term, voter_id, voter_addr, granted).await;
            }
        }
    }

    async fn run(&mut self) -> AppResult<()> {
        loop {
            // Randomized election timeout per iteration (150-300ms)
            let mut rng = rand::thread_rng();
            let timeout_ms = rng.gen_range(150..=300);
            let deadline = sleep(Duration::from_millis(timeout_ms));
            tokio::pin!(deadline);

            select! {
                _ = &mut deadline => {
                    // election timeout fired
                    self.start_election().await;
                }
                node_msg = self.recv_node_msg() =>{
                    match node_msg {
                        Ok(msg) => {
                            match msg {
                                Message::Request { op, addr } => {
                                    self.handle_request(op, addr).await;
                                }
                                Message::Log { op } => {
                                    self.handle_log(op).await;
                                }
                                Message::Ack { id } => {
                                    self.handle_ack(id).await;
                                }
                                Message::RequestVote { term, candidate_id, candidate_addr } => {
                                    self.handle_vote_request(term, candidate_id, candidate_addr).await;
                                }
                                Message::Vote { term, voter_id, voter_addr, granted } => {
                                    self.handle_vote(term, voter_id, voter_addr, granted).await;
                                }
                            }
                        }
                        _ => { return Ok(()); }
                    }
                }
            }
        }
    }
}

use super::operation::Operation;
use std::net::SocketAddr;

/// Messages sent between nodes.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /* Raft
     ***/
    /// Operation request sent by the client server (station).
    Request { op: Operation, addr: SocketAddr },
    /// Log message sent by coordinator to the replicas.
    Log { op: Operation },
    /// Acknowledgement reply sent by a replica after receiving a valid `Log` message from the
    /// coordinator.
    Ack { id: u32 },
    /* Leader election */
    /// RequestVote sent by a candidate to ask for votes.
    RequestVote {
        term: u64,
        candidate_id: u64,
        candidate_addr: SocketAddr,
    },
    /// Vote reply sent by a follower to a candidate.
    Vote {
        term: u64,
        voter_id: u64,
        voter_addr: SocketAddr,
        granted: bool,
    },
    /* Leader election
     ***/
    // ...
}

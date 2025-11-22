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
    /* Bully election */
    /// Election message sent by a candidate to notify peers.
    Election {
        candidate_id: u64,
        candidate_addr: SocketAddr,
    },
    /// OK reply sent by a higher-id process to a candidate.
    ElectionOk {
        responder_id: u64,
        responder_addr: SocketAddr,
    },
    /// Coordinator announcement sent by the new leader to all processes.
    Coordinator {
        leader_id: u64,
        leader_addr: SocketAddr,
    },
    /* Leader election
     ***/
    // ...
}

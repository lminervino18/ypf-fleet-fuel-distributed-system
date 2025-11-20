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
    /* Leader election
     ***/
    // ...
}

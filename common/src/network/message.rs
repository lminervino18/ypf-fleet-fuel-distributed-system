use crate::operation::Operation;
use std::net::SocketAddr;

/// Messages sent between nodes.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /* Raft
     ***/
    /// Operation request sent by the client server (station).
    Request {
        op_id: u32,
        op: Operation,
        addr: SocketAddr,
    },

    /// Log message sent by coordinator to the replicas.
    Log { op_id: u32, op: Operation },

    /// Acknowledgement reply sent by a replica after receiving a valid `Log` message
    /// from the coordinator.
    Ack { op_id: u32 },

    /* Leader election (bully) */
    /// Election message sent by a candidate to notify peers.
    Election {
        candidate_id: u64,
        candidate_addr: SocketAddr,
    },

    /// OK reply sent by a higher-id process to a candidate.
    ElectionOk {
        /// Useful for debugging/logging who replied.
        responder_id: u64,
    },

    /// Coordinator announcement sent by the new leader to all processes.
    Coordinator {
        leader_id: u64,
        leader_addr: SocketAddr,
    },

    /* Cluster membership / discovery */
    /// A node asks to join the cluster.
    ///
    /// Typically sent by a replica to the (assumed) leader when it starts up,
    /// so the leader can register it in the membership view and reply with
    /// the current cluster state.
    Join { node_id: u64, addr: SocketAddr },

    /// Snapshot of the current cluster membership.
    ///
    /// Used by the leader to inform nodes about all known members (IDs + addresses),
    /// so each node can build a consistent view of the cluster and use it for
    /// leader election, replication, etc.
    ClusterView { members: Vec<(u64, SocketAddr)> },
}

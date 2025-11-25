use crate::NodeRole;
use crate::operation::{Operation,DatabaseSnapshot };
use crate::operation_result::{OperationResult};
use std::net::SocketAddr;

/// Messages sent between nodes.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /* Client request and server response messages
     ***/
    /// Operation request sent by the clients (station/admins).
    Request {
        // La request ya lleva la Operation adentro
        req_id: u32,
        op: Operation,
        addr: SocketAddr,
    },

    /// Operation response sent by the leader to the client.
    ///
    /// El resultado ahora es un `OperationResult`, que matchea con la
    /// `Operation` original. Ejemplos:
    /// - si la op era `Charge`, será `OperationResult::Charge(ChargeResult)`
    /// - si la op era `AccountQuery`, será `OperationResult::AccountQuery(...)`
    Response {
        req_id: u32,
        op_result: OperationResult,
    },

    /* Raft
     ***/
    /// Log message sent by coordinator to the replicas.
    Log {
        op_id: u32,
        op: Operation,
    },

    /// Acknowledgement reply sent by a replica after receiving a valid `Log` message
    /// from the coordinator.
    Ack {
        op_id: u32,
    },

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
    Join {
        addr: SocketAddr,
    },

    /// Snapshot of the current cluster membership + database.
    ClusterView {
        /// Lista de miembros conocidos: (node_id, addr)
        members: Vec<(u64, SocketAddr)>,
        /// Dirección del líder actual.
        leader_addr: SocketAddr,
        /// Snapshot lógico de toda la base de datos.
        database: DatabaseSnapshot,
    },

    /// Notificación incremental de un nuevo miembro.
    ClusterUpdate {
        new_member: (u64, SocketAddr),
    },

    // debug msgs for typescript simulation frontend
    RoleQuery {
        addr: SocketAddr,
    },
    RoleResponse {
        node_id: u64,
        role: NodeRole,
    },
}

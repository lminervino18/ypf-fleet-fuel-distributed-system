//! Protocol-level message types exchanged between nodes and clients.
//!
//! This module defines the `Message` enum used across the network layer.
//! Messages cover client requests/responses, replication (log/ack), leader
//! election (bully), cluster membership, and a few debug messages.
//!
//! Each variant is serialized/deserialized by the corresponding module in
//! `network::serials`. Keep changes here in sync with the wire format.

use crate::NodeRole;
use crate::operation::{DatabaseSnapshot, Operation};
use crate::operation_result::OperationResult;
use std::net::SocketAddr;

/// Messages exchanged between nodes (and between clients and nodes).
///
/// Variants include:
/// - client requests/responses (Request/Response),
/// - replication primitives (Log/Ack),
/// - leader election messages (Election/ElectionOk/Coordinator),
/// - cluster membership management (Join/ClusterView/ClusterUpdate),
/// - debug messages used by the frontend (RoleQuery/RoleResponse).
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /* Client request and server response messages */
    /// Operation request sent by clients (stations or admin tools).
    Request {
        /// Correlation id assigned by the requester.
        req_id: u32,
        /// Operation to execute.
        op: Operation,
        /// Address of the requesting endpoint.
        addr: SocketAddr,
    },

    /// Operation response sent by the leader to the client.
    ///
    /// The `op_result` corresponds to the original `Operation`:
    /// - for a `Charge` operation, `OperationResult::Charge(ChargeResult)` is used;
    /// - for an `AccountQuery`, `OperationResult::AccountQuery(...)` is used; etc.
    Response {
        req_id: u32,
        op_result: OperationResult,
    },

    /* Replication (log replication) */
    /// Log entry forwarded by the leader to replicas.
    Log {
        op_id: u32,
        op: Operation,
    },

    /// Acknowledgement sent by a replica after applying/storing a `Log`.
    Ack {
        op_id: u32,
    },

    /* Leader election (Bully algorithm) */
    /// Election announcement sent by a candidate to peers.
    Election {
        candidate_id: u64,
        candidate_addr: SocketAddr,
    },

    /// Positive reply from a higher-id process to a candidate.
    ///
    /// Includes the responder's id for debugging/logging.
    ElectionOk {
        responder_id: u64,
    },

    /// Coordinator (leader) announcement sent by the newly elected leader.
    Coordinator {
        leader_id: u64,
        leader_addr: SocketAddr,
    },

    /* Cluster membership / discovery */
    /// Request to join the cluster.
    Join {
        addr: SocketAddr,
    },

    /// Full cluster view snapshot including membership and database state.
    ClusterView {
        /// Known members as pairs (node_id, address).
        members: Vec<(u64, SocketAddr)>,
        /// Current leader's address.
        leader_addr: SocketAddr,
        /// Logical snapshot of the database.
        database: DatabaseSnapshot,
    },

    /// Incremental notification about a new cluster member.
    ClusterUpdate {
        new_member: (u64, SocketAddr),
    },

    /* Debug messages for external frontend / simulation */
    /// Ask a node to report its role (debugging / UI).
    RoleQuery {
        addr: SocketAddr,
    },

    /// Reply indicating a node's role.
    RoleResponse {
        node_id: u64,
        role: NodeRole,
    },
}

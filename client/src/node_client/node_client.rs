//! Thin client node.
//!
//! This module implements a lightweight forwarding node that sits between a
//! local `Station` (pump simulator) and a set of known cluster nodes. The
//! behaviour is intentionally simple and does not participate in consensus or
//! actor-based logic.
//!
//! Features:
//! - Expose a local `Station` (pumps on stdin).
//! - When ONLINE:
//!     - Forward each `ChargeRequest` to one known distributed node,
//!       trying nodes in order as fallbacks.
//!     - Return the real cluster result to the `Station`.
//!     - If no node accepts the request, fall back to OFFLINE behaviour.
//! - When OFFLINE:
//!     - Answer OK locally and enqueue charges to replay later.
//! - On CONNECT:
//!     - Flush the offline queue to the cluster with `from_offline_station = true`.
//!     - Try nodes in order and keep entries that still cannot be sent.
//! - Do not participate in elections or actor-based processing.

use common::errors::AppResult;
use common::operation::Operation;
use common::operation_result::{ChargeResult, OperationResult};
use common::{Connection, Message, NodeToStationMsg, Station, StationToNodeMsg};

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use tokio::select;

/// Charge stored while the client is OFFLINE.
///
/// These operations will be replayed to the cluster once the client
/// switches back to ONLINE mode.
#[derive(Debug, Clone)]
struct QueuedCharge {
    account_id: u64,
    card_id: u64,
    amount: f32,
}

/// Compute a stable u64 identifier for a `SocketAddr`.
///
/// The result is derived from the IP and port via a default hasher. The
/// returned value is in the range 1..=max where `max = 100000`.
pub fn get_id_given_addr(addr: SocketAddr) -> u64 {
    let mut hasher = DefaultHasher::new();
    addr.ip().hash(&mut hasher);
    addr.port().hash(&mut hasher);
    let hash = hasher.finish();
    let max = 100000;
    (hash % max) + 1
}

/// Forwarding client node.
///
/// It sits between a local Station (pumps) and the distributed cluster:
///
/// - On each `StationToNodeMsg::ChargeRequest`:
///     * if ONLINE: builds an `Operation::Charge` (`from_offline_station = false`)
///       and sends a `Message::Request` to one known node (trying them in order).
///       Later, when the cluster responds, it forwards a `ChargeResult` to the Station.
///       If it cannot send to ANY known node, it falls back to OFFLINE behavior:
///       answer OK locally and enqueue for later replay.
///     * if OFFLINE: immediately sends a local `ChargeResult` OK to the Station
///       and enqueues the charge to be replayed later.
/// - On CONNECT: replays all queued offline charges with `from_offline_station = true`
///   and ignores their responses (these are primarily to update cluster state).
///
/// The NodeClient intentionally does not implement cluster protocols such as
/// elections or actor interactions; it only forwards requests and manages a
/// local offline queue.
pub struct NodeClient {
    /// Local address used by this node (only to populate `addr` in Request).
    bind_addr: SocketAddr,

    /// Cluster nodes that this client knows about and can forward to.
    ///
    /// Nodes are tried in-order as fallbacks.
    known_nodes: Vec<SocketAddr>,

    /// Simple round-robin index over `known_nodes` (kept for potential future use).
    next_node_idx: usize,

    /// Whether this client is ONLINE (forwarding enabled) or OFFLINE.
    online: bool,

    /// Queue of charges accumulated while OFFLINE.
    offline_queue: Vec<QueuedCharge>,

    /// Requests that were sent while ONLINE and whose responses
    /// still need to be forwarded to the Station.
    active_online_requests: HashSet<u32>,

    /// Synthetic request id for replaying OFFLINE charges to the cluster.
    ///
    /// These ids are internal to the client and are not forwarded back to the Station.
    next_replay_req_id: u32,
}

impl NodeClient {
    /// Create a new `NodeClient` with the given local address and known nodes.
    ///
    /// The client starts in ONLINE mode by default.
    pub fn new(bind_addr: SocketAddr, known_nodes: Vec<SocketAddr>) -> Self {
        Self {
            bind_addr,
            known_nodes,
            next_node_idx: 0,
            online: true,
            offline_queue: Vec::new(),
            active_online_requests: HashSet::new(),
            next_replay_req_id: 1_000_000, // keep replay ids clearly separated
        }
    }

    /// (Optional) pick a single remote node using round-robin.
    ///
    /// Currently unused: the implementation tries nodes in `known_nodes` order.
    #[allow(dead_code)]
    fn pick_next_node(&mut self) -> SocketAddr {
        let idx = self.next_node_idx % self.known_nodes.len();
        let addr = self.known_nodes[idx];
        self.next_node_idx = (self.next_node_idx + 1) % self.known_nodes.len();
        addr
    }

    /// Main event loop.
    ///
    /// Multiplex `Station` messages and network messages using `tokio::select!`.
    /// This client never touches the actor world.
    pub async fn run(mut self, mut connection: Connection, mut station: Station) -> AppResult<()> {
        loop {
            select! {
                // ======================
                // Station → NodeClient
                // ======================
                maybe_msg = station.recv() => {
                    match maybe_msg {
                        Some(msg) => {
                            self.handle_station_msg(&mut connection, &mut station, msg).await?;
                        }
                        None => {
                            // Station closed its side: nothing left to do.
                            eprintln!("[node_client] station closed; shutting down");
                            break;
                        }
                    }
                }

                // ======================
                // Node → NodeClient
                // ======================
                msg_res = connection.recv() => {
                    match msg_res {
                        Ok(msg) => {
                            self.handle_node_msg(&mut connection, &mut station, msg).await?;
                        }
                        Err(e) => {
                            // Log network errors; Connection manages sockets internally.
                            eprintln!("[node_client] connection recv error: {e:?}");
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle messages coming from the local `Station`.
    ///
    /// Behaviour:
    /// - `ChargeRequest` → if ONLINE, forward with fallback between known nodes;
    ///   if OFFLINE, answer locally and enqueue.
    /// - `DisconnectNode` → switch to OFFLINE mode.
    /// - `ConnectNode` → switch to ONLINE mode and flush offline queue.
    async fn handle_station_msg(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        msg: StationToNodeMsg,
    ) -> AppResult<()> {
        match msg {
            StationToNodeMsg::ChargeRequest {
                account_id,
                card_id,
                amount,
                request_id,
            } => {
                self.forward_or_enqueue_charge(
                    connection, station, account_id, card_id, amount, request_id,
                )
                .await?;
            }

            StationToNodeMsg::DisconnectNode => {
                self.online = false;
                let _ = station
                    .send(NodeToStationMsg::Debug(
                        "[node_client] switched to OFFLINE mode; new charges will be accepted locally and replayed later."
                            .to_string(),
                    ))
                    .await;
            }

            StationToNodeMsg::ConnectNode => {
                self.online = true;
                let _ = station
                    .send(NodeToStationMsg::Debug(
                        "[node_client] switched to ONLINE mode; replaying queued offline charges (if any)."
                            .to_string(),
                    ))
                    .await;

                // Once ONLINE, flush any queued offline charges to the cluster.
                self.flush_offline_queue(connection).await?;
            }
        }

        Ok(())
    }

    /// Decide what to do with a new charge.
    ///
    /// - If there are no known nodes, treat the request as OFFLINE and enqueue.
    /// - If OFFLINE, enqueue and reply OK locally.
    /// - If ONLINE, forward to known nodes with fallback.
    async fn forward_or_enqueue_charge(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        account_id: u64,
        card_id: u64,
        amount: f32,
        request_id: u32,
    ) -> AppResult<()> {
        // If we don't know any cluster node, behave as OFFLINE.
        if self.known_nodes.is_empty() {
            let _ = station
                .send(NodeToStationMsg::Debug(
                    "[node_client] no known nodes; treating charge as offline and enqueuing for later replay."
                        .to_string(),
                ))
                .await;

            self.enqueue_offline_charge(station, account_id, card_id, amount, request_id)
                .await?;
            return Ok(());
        }

        // Explicit OFFLINE mode: accept & enqueue.
        if !self.online {
            self.enqueue_offline_charge(station, account_id, card_id, amount, request_id)
                .await?;
            return Ok(());
        }

        // ONLINE: try to forward to some known node.
        self.forward_online_charge(connection, station, account_id, card_id, amount, request_id)
            .await
    }

    /// OFFLINE handling:
    ///
    /// - Send a local `ChargeResult::Ok` so the pump can continue.
    /// - Enqueue the charge for later replay with `from_offline_station = true`.
    async fn enqueue_offline_charge(
        &mut self,
        station: &mut Station,
        account_id: u64,
        card_id: u64,
        amount: f32,
        request_id: u32,
    ) -> AppResult<()> {
        self.offline_queue.push(QueuedCharge {
            account_id,
            card_id,
            amount,
        });

        // Inform the Station that the charge is accepted, so the pump is unblocked.
        station
            .send(NodeToStationMsg::ChargeResult {
                request_id,
                allowed: true,
                error: None,
            })
            .await?;

        Ok(())
    }

    /// ONLINE forwarding:
    ///
    /// - Build an `Operation::Charge` with `from_offline_station = false`.
    /// - Try to send a `Message::Request` to known nodes in order.
    /// - If a send succeeds, track `request_id` so the response is forwarded to the Station.
    /// - If all sends fail, fall back to OFFLINE behaviour (local OK + enqueue).
    async fn forward_online_charge(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        account_id: u64,
        card_id: u64,
        amount: f32,
        request_id: u32,
    ) -> AppResult<()> {
        let op = Operation::Charge {
            account_id,
            card_id,
            amount,
            from_offline_station: false,
        };

        let mut sent = false;

        // Try each known node in order until one send succeeds.
        for target in &self.known_nodes {
            let msg = Message::Request {
                req_id: request_id,
                op: op.clone(),
                addr: self.bind_addr,
            };

            match connection.send(msg, target).await {
                Ok(()) => {
                    sent = true;
                    break;
                }
                Err(_) => {
                    // Try next node in the list.
                }
            }
        }

        if sent {
            // Track the request so we know to forward its response back to the Station.
            self.active_online_requests.insert(request_id);
            Ok(())
        } else {
            // All nodes failed: fall back to OFFLINE behavior for this charge.
            eprintln!(
                "[node_client] all known nodes failed for request_id={request_id}; falling back to OFFLINE behavior (local OK + enqueue)."
            );

            self.enqueue_offline_charge(station, account_id, card_id, amount, request_id)
                .await
        }
    }

    /// Flush all queued offline charges to the cluster.
    ///
    /// - Try all known nodes for each charge (in order).
    /// - Use synthetic request ids (starting from `next_replay_req_id`).
    /// - Set `from_offline_station = true`.
    /// - Do not send any result back to the Station.
    /// - Re-enqueue charges that cannot be sent to any node.
    async fn flush_offline_queue(&mut self, connection: &mut Connection) -> AppResult<()> {
        if self.offline_queue.is_empty() {
            return Ok(());
        }

        if self.known_nodes.is_empty() {
            eprintln!(
                "[node_client] cannot flush offline queue: no known nodes ({} charges still queued).",
                self.offline_queue.len()
            );
            return Ok(());
        }

        let mut still_pending: Vec<QueuedCharge> = Vec::new();

        for queued in self.offline_queue.drain(..) {
            let op = Operation::Charge {
                account_id: queued.account_id,
                card_id: queued.card_id,
                amount: queued.amount,
                from_offline_station: true,
            };

            let mut sent = false;

            for target in &self.known_nodes {
                let req_id = self.next_replay_req_id;
                self.next_replay_req_id = self.next_replay_req_id.wrapping_add(1);

                let msg = Message::Request {
                    req_id,
                    op: op.clone(),
                    addr: self.bind_addr,
                };

                match connection.send(msg, target).await {
                    Ok(()) => {
                        sent = true;
                        break;
                    }
                    Err(e) => {
                        eprintln!(
                            "[node_client] failed to replay offline charge to {target}: {e:?}"
                        );
                        // Try next node.
                    }
                }
            }

            if !sent {
                // Could not send this charge to any node; keep it in the queue.
                still_pending.push(queued);
            }
        }

        // Restore charges that we couldn't flush.
        if !still_pending.is_empty() {
            self.offline_queue = still_pending;
        }

        Ok(())
    }

    /// Handle any `Message` coming from remote nodes.
    ///
    /// Expected messages include:
    /// - `Message::Response { req_id, op_result }`.
    ///   Responses for replayed offline charges are ignored at the Station level.
    async fn handle_node_msg(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        msg: Message,
    ) -> AppResult<()> {
        match msg {
            Message::Response { req_id, op_result } => {
                // Only forward responses that correspond to ONLINE requests.
                if !self.active_online_requests.remove(&req_id) {
                    eprintln!(
                        "[node_client] response for non-tracked req_id {req_id} (probably offline replay); ignoring at Station level."
                    );
                    return Ok(());
                }

                self.handle_response_from_node(station, req_id, op_result)
                    .await?;
            }
            Message::RoleQuery { addr } => {
                let role_msg = Message::RoleResponse {
                    node_id: get_id_given_addr(self.bind_addr),
                    role: common::NodeRole::Station,
                };
                connection.send(role_msg, &addr).await?;
            }
            other => {
                // This forwarding client is not part of the cluster protocol.
                // Ignore other message types and log for debugging.
                eprintln!("[node_client] unexpected message from node: {other:?}");
            }
        }

        Ok(())
    }

    /// Translate the `OperationResult` from the cluster into a
    /// `NodeToStationMsg` and send it back to the Station.
    ///
    /// Called only for ONLINE requests (tracked in `active_online_requests`).
    async fn handle_response_from_node(
        &mut self,
        station: &mut Station,
        req_id: u32,
        op_result: OperationResult,
    ) -> AppResult<()> {
        match op_result {
            OperationResult::Charge(cr) => {
                let (allowed, error) = match cr {
                    ChargeResult::Ok => (true, None),
                    ChargeResult::Failed(e) => (false, Some(e)),
                };

                station
                    .send(NodeToStationMsg::ChargeResult {
                        request_id: req_id,
                        allowed,
                        error,
                    })
                    .await?;
            }

            other => {
                // For now, only Charge responses are relevant to pumps.
                let _ = station
                    .send(NodeToStationMsg::Debug(format!(
                        "[node_client] ignoring non-charge response for req_id {req_id}: {other:?}"
                    )))
                    .await;
            }
        }

        Ok(())
    }
}

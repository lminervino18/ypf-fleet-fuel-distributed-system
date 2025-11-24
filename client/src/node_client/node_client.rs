// client/src/node_client/node_client.rs

//! Thin client node:
//! - exposes a local Station (pumps on stdin),
//! - when ONLINE: forwards each ChargeRequest to one known distributed node
//!   and returns the real cluster result to the Station,
//!   trying nodes in order and falling back to OFFLINE behavior if none work.
//! - when OFFLINE: answers OK locally and enqueues charges to replay later,
//! - on CONNECT: flushes the offline queue to the cluster with
//!   `from_offline_station = true`, trying nodes in order and keeping
//!   entries that still cannot be sent,
//! - does NOT participate in elections or actor-based logic.

use common::errors::AppResult;
use common::operation::Operation;
use common::operation_result::{ChargeResult, OperationResult};
use common::{Connection, Message, NodeToStationMsg, Station, StationToNodeMsg};

use std::collections::HashSet;
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

/// Forwarding client node.
///
/// It sits between a local Station (pumps) and the distributed cluster:
/// - On each `StationToNodeMsg::ChargeRequest`:
///     * if ONLINE: builds an `Operation::Charge` (`from_offline_station = false`)
///       and sends a `Message::Request` to one known node (trying them in order).
///       Later, when the cluster responds, it forwards a `ChargeResult` to the Station.
///       If it cannot send to ANY known node, it falls back to OFFLINE behavior:
///       answer OK locally and enqueue for later replay.
///     * if OFFLINE: immediately sends a local `ChargeResult` OK to the Station
///       and enqueues the charge to be replayed later.
/// - On CONNECT: replays all queued offline charges with `from_offline_station = true`
///   and ignores their responses (they are mainly for the cluster state).
pub struct NodeClient {
    /// Local address used by this node (only to populate `addr` in Request).
    bind_addr: SocketAddr,

    /// Cluster nodes that this client knows about and can forward to.
    ///
    /// We try them in order as fallbacks.
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
    /// We never send these ids back to the Station; they are only used
    /// between this client and the cluster.
    next_replay_req_id: u32,
}

impl NodeClient {
    /// Create a new NodeClient with the given local address and known nodes.
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
    /// Currently unused: we always try nodes in `known_nodes` order.
    #[allow(dead_code)]
    fn pick_next_node(&mut self) -> SocketAddr {
        let idx = self.next_node_idx % self.known_nodes.len();
        let addr = self.known_nodes[idx];
        self.next_node_idx = (self.next_node_idx + 1) % self.known_nodes.len();
        addr
    }

    /// Main event loop:
    /// - multiplexes Station messages and node messages with `tokio::select!`,
    /// - never touches the actor world.
    pub async fn run(
        mut self,
        mut connection: Connection,
        mut station: Station,
    ) -> AppResult<()> {
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
                            println!("[node_client] Station closed; shutting down.");
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
                            // For now, just log network errors. The Connection
                            // type is responsible for managing sockets.
                            eprintln!("[node_client] connection recv error: {e:?}");
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle high-level Station messages:
    /// - ChargeRequest → if ONLINE, forward with fallback between known nodes;
    ///                   if OFFLINE, answer locally and enqueue.
    /// - DisconnectNode → switch to OFFLINE mode.
    /// - ConnectNode → switch back to ONLINE mode and flush offline queue.
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
                    connection,
                    station,
                    account_id,
                    card_id,
                    amount,
                    request_id,
                )
                .await?;
            }

            StationToNodeMsg::DisconnectNode => {
                self.online = false;
                println!("[node_client] Switched to OFFLINE mode.");
                let _ = station
                    .send(NodeToStationMsg::Debug(
                        "[node_client] Node switched to OFFLINE mode; new charges will be accepted locally and replayed later."
                            .to_string(),
                    ))
                    .await;
            }

            StationToNodeMsg::ConnectNode => {
                self.online = true;
                println!("[node_client] Switched to ONLINE mode.");
                let _ = station
                    .send(NodeToStationMsg::Debug(
                        "[node_client] Node switched to ONLINE mode; replaying queued offline charges (if any)."
                            .to_string(),
                    ))
                    .await;

                // Once ONLINE, flush any queued offline charges to the cluster.
                self.flush_offline_queue(connection).await?;
            }
        }

        Ok(())
    }

    /// Decide what to do with a new charge:
    /// - If we have no known nodes, treat it as OFFLINE and enqueue.
    /// - If OFFLINE, enqueue and answer locally.
    /// - If ONLINE, forward to one of the known nodes with fallback.
    async fn forward_or_enqueue_charge(
        &mut self,
        connection: &mut Connection,
        station: &mut Station,
        account_id: u64,
        card_id: u64,
        amount: f32,
        request_id: u32,
    ) -> AppResult<()> {
        // If we don't know any cluster node, we effectively behave as OFFLINE.
        if self.known_nodes.is_empty() {
            let _ = station
                .send(NodeToStationMsg::Debug(
                    "[node_client] No known nodes; treating charge as offline and enqueuing for later replay."
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
        self.forward_online_charge(
            connection,
            station,
            account_id,
            card_id,
            amount,
            request_id,
        )
        .await
    }

    /// OFFLINE path:
    /// - Send a local `ChargeResult` OK so the pump can continue,
    /// - Enqueue the charge to replay later with `from_offline_station = true`.
    async fn enqueue_offline_charge(
        &mut self,
        station: &mut Station,
        account_id: u64,
        card_id: u64,
        amount: f32,
        request_id: u32,
    ) -> AppResult<()> {
        println!(
            "[node_client][OFFLINE] Enqueuing offline charge: account={account_id}, card={card_id}, amount={amount}, request_id={request_id}"
        );

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

    /// ONLINE path:
    /// - Build an `Operation::Charge` with `from_offline_station = false`,
    /// - Try to send a `Message::Request` to one of the known nodes in order,
    /// - If at least one send succeeds, track `request_id` in `active_online_requests`
    ///   so we know we must return that response to the Station.
    /// - If ALL sends fail, fall back to OFFLINE behavior:
    ///   answer OK locally and enqueue the charge for replay.
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

        println!(
            "[node_client][ONLINE] Forwarding Charge(account={}, card={}, amount={}) with request_id={} to known nodes={:?}",
            account_id,
            card_id,
            amount,
            request_id,
            self.known_nodes,
        );

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
                    println!(
                        "[node_client] Successfully sent request_id={request_id} to {target}"
                    );
                    sent = true;
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "[node_client] Failed to send request_id={request_id} to {target}: {e:?}"
                    );
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
                "[node_client] All known nodes failed for request_id={request_id}; falling back to OFFLINE behavior (local OK + enqueue)."
            );

            self.enqueue_offline_charge(station, account_id, card_id, amount, request_id)
                .await
        }
    }

    /// Flush all queued offline charges to the cluster.
    ///
    /// - Tries all known nodes for each charge (in order),
    /// - Uses synthetic `req_id`s (starting from `next_replay_req_id`),
    /// - Sets `from_offline_station = true`,
    /// - Does NOT send any result back to the Station (the Station ya respondió OK),
    /// - If it cannot send a given charge to ANY node, it re-enqueues that charge.
    async fn flush_offline_queue(&mut self, connection: &mut Connection) -> AppResult<()> {
        if self.offline_queue.is_empty() {
            return Ok(());
        }

        if self.known_nodes.is_empty() {
            println!(
                "[node_client] Cannot flush offline queue: no known nodes ({} charges still queued).",
                self.offline_queue.len()
            );
            return Ok(());
        }

        let total = self.offline_queue.len();

        println!(
            "[node_client] Flushing {} offline charges to known nodes {:?} with from_offline_station=true",
            total, self.known_nodes
        );

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
                        println!(
                            "[node_client] Replayed offline charge (req_id={} account={} card={} amount={}) to {}",
                            req_id, queued.account_id, queued.card_id, queued.amount, target
                        );
                        sent = true;
                        break;
                    }
                    Err(e) => {
                        eprintln!(
                            "[node_client] Failed to replay offline charge to {target}: {e:?}"
                        );
                        // Try next node.
                    }
                }
            }

            if !sent {
                // Could not send this charge to ANY node; keep it in the queue.
                eprintln!(
                    "[node_client] Could not replay offline charge (account={} card={} amount={}); keeping it queued.",
                    queued.account_id, queued.card_id, queued.amount
                );
                still_pending.push(queued);
            }
        }

        // Restore charges that we couldn't flush.
        if !still_pending.is_empty() {
            println!(
                "[node_client] {} offline charges remain queued after flush attempt.",
                still_pending.len()
            );
            self.offline_queue = still_pending;
        }

        Ok(())
    }

    /// Handle any `Message` coming from remote nodes.
    ///
    /// For this client we only expect:
    /// - `Message::Response { req_id, op_result }`.
    ///   Responses for replayed offline charges will *not* be forwarded back
    ///   to the Station (we never registered those ids in `active_online_requests`).
    async fn handle_node_msg(
        &mut self,
        _connection: &mut Connection,
        station: &mut Station,
        msg: Message,
    ) -> AppResult<()> {
        match msg {
            Message::Response { req_id, op_result } => {
                // Only forward responses that correspond to ONLINE requests.
                if !self.active_online_requests.remove(&req_id) {
                    println!(
                        "[node_client] Response for non-tracked req_id {req_id} (probably offline replay); ignoring at Station level."
                    );
                    return Ok(());
                }

                self.handle_response_from_node(station, req_id, op_result)
                    .await?;
            }
            other => {
                // This forwarding client is not part of the cluster protocol,
                // so all other message types are unexpected and ignored.
                eprintln!("[node_client] Unexpected message from node: {other:?}");
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
                // For now, we only use Charge operations coming from pumps.
                let _ = station
                    .send(NodeToStationMsg::Debug(format!(
                        "[node_client] Ignoring non-charge response for req_id {req_id}: {other:?}"
                    )))
                    .await;
            }
        }

        Ok(())
    }
}

use super::{
    actors::ActorEvent,
    message::Message,
    operation::Operation,
};
use common::{Station, StationToNodeMsg, NodeToStationMsg};
use crate::errors::{AppResult, VerifyError};
use std::net::SocketAddr;
use tokio::select;
use tokio::time::{sleep, Duration};

/// Role of a node in the YPF Ruta distributed system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeRole {
    Leader,
    Replica,
    Station,
}

pub trait Node {
    async fn handle_request(
        &mut self,
        op_id: u32,
        op: Operation,
        client_addr: SocketAddr,
    ) -> AppResult<()>;

    async fn handle_log(&mut self, op_id: u32, op: Operation);

    async fn handle_ack(&mut self, id: u32);

    async fn handle_operation_result(
        &mut self,
        op_id: u32,
        operation: Operation,
        success: bool,
        error: Option<VerifyError>,
    );

    /// Default dispatcher for Actor events.
    ///
    /// Currently only forwards OperationResult into `handle_operation_result`.
    async fn handle_actor_event(&mut self, event: ActorEvent) {
        match event {
            ActorEvent::OperationResult {
                op_id,
                operation,
                success,
                error,
            } => {
                self.handle_operation_result(op_id, operation, success, error).await;
            }
            _ => {
                // Other actor events not wired yet.
                todo!();
            }
        }
    }

    /* OperationResult {
        op_id: u64,
        operation: Operation,
        success: bool,
        /// Domain/business error, if any (limits, invalid updates, etc.).
        /// For offline-replayed charges this will always be `None`.
        error: Option<VerifyError>,
    },*/

    async fn handle_charge_request(
        &mut self,
        pump_id: usize,
        account_id: u64,
        card_id: u64,
        amount: f32,
        request_id: u64,
    );

    /// Default OFFLINE transition handler.
    ///
    /// Concrete nodes (Leader / Replica) override this to:
    /// - flip their `is_offline` flag,
    /// - emit debug messages to the Station,
    /// - adjust cluster behavior.
    async fn handle_disconnect_node(&mut self) {
        /* Example (Leader):
        if self.is_offline {
            let _ = self
                .station.send(NodeToStationMsg::Debug(
                    "[Leader] Node is already in OFFLINE mode; pump operations are auto-approved."
                        .to_string(),
                ))
                .await;
        } else {
            self.is_offline = true;
            let _ = self
                .station.send(NodeToStationMsg::Debug(
                    "[Leader] Node switched to OFFLINE mode. Cluster traffic will be ignored and pump operations will be queued and auto-approved."
                        .to_string(),
                ))
                .await;
        }
        */
    }

    /// Default ONLINE transition handler.
    ///
    /// Concrete nodes (Leader / Replica) override this to:
    /// - flip their `is_offline` flag,
    /// - replay queued offline operations into the actor layer,
    /// - notify the Station via debug messages.
    async fn handle_connect_node(&mut self) {
        /* Example (Leader):
        if !self.is_offline {
            let _ = self
                .station.send(NodeToStationMsg::Debug(
                    "[Leader] Node is already in ONLINE mode; pump operations go through normal verification."
                        .to_string(),
                ))
                .await;
        } else {
            self.is_offline = false;
            let queued = self.offline_queue.len();
            while let Some(OfflineQueuedCharge {
                request_id,
                account_id,
                card_id,
                amount,
            }) = self.offline_queue.pop_front()
            {
                let op = Operation::Charge {
                    account_id,
                    card_id,
                    amount,
                    from_offline_station: true,
                };

                self.router.do_send(RouterCmd::Execute {
                    op_id: request_id,
                    operation: op,
                });
            }

            let _ = self
                .station.send(NodeToStationMsg::Debug(
                    format!(
                        "[Leader] Node switched back to ONLINE mode. Replayed {} queued offline operations into the actor system (they were already confirmed to the station).",
                        queued
                    ),
                ))
                .await;
        }
        */
    }

    /// Default handler for high-level Station messages.
    ///
    /// Dispatches:
    /// - `ChargeRequest` into `handle_charge_request`,
    /// - `DisconnectNode` into `handle_disconnect_node`,
    /// - `ConnectNode` into `handle_connect_node`.
    async fn handle_station_msg(&mut self, msg: StationToNodeMsg) {
        match msg {
            StationToNodeMsg::ChargeRequest {
                pump_id,
                account_id,
                card_id,
                amount,
                request_id,
            } => {
                self.handle_charge_request(pump_id, account_id, card_id, amount, request_id)
                    .await;
            }

            StationToNodeMsg::DisconnectNode => {
                self.handle_disconnect_node().await;
            }

            StationToNodeMsg::ConnectNode => {
                self.handle_connect_node().await;
            }
        }
    }

    // === I/O hooks each concrete node must provide ===
    async fn recv_node_msg(&mut self) -> AppResult<Message>;
    async fn recv_actor_event(&mut self) -> Option<ActorEvent>;
    async fn recv_station_message(&mut self) -> Option<StationToNodeMsg>;

    // === Bully / leader election hooks ===
    async fn handle_election(&mut self, candidate_id: u64, candidate_addr: SocketAddr);
    async fn handle_election_ok(&mut self, responder_id: u64);
    async fn handle_coordinator(&mut self, leader_id: u64, leader_addr: SocketAddr);
    async fn start_election(&mut self);

    // === Cluster membership hooks (Join / ClusterView) ===

    /// Handle an incoming Join message from a node that wants to enter the cluster.
    ///
    /// Typical Leader behavior:
    /// - add (node_id, addr) into the membership map,
    /// - broadcast a fresh ClusterView snapshot to all members (including the newcomer).
    ///
    /// Typical Replica behavior:
    /// - usually does not receive Join (only Leader does), so this may be a no-op or `todo!()`.
    async fn handle_join(&mut self, node_id: u64, addr: SocketAddr);

    /// Handle an incoming ClusterView snapshot.
    ///
    /// Typical behavior (Leader/Replica):
    /// - replace local membership view with the provided vector,
    /// - ensure we always keep our own (id, address) entry consistent.
    async fn handle_cluster_view(&mut self, members: Vec<(u64, SocketAddr)>);

    /// Default handler for any node-to-node Message.
    async fn handle_node_msg(&mut self, msg: Message) {
        match msg {
            Message::Request { op_id, op, addr } => {
                self.handle_request(op_id, op, addr).await;
            }
            Message::Log { op_id, op } => {
                self.handle_log(op_id, op).await;
            }
            Message::Ack { op_id } => {
                self.handle_ack(op_id).await;
            }
            Message::Election {
                candidate_id,
                candidate_addr,
            } => {
                self.handle_election(candidate_id, candidate_addr).await;
            }
            Message::ElectionOk { responder_id } => {
                self.handle_election_ok(responder_id).await;
            }
            Message::Coordinator {
                leader_id,
                leader_addr,
            } => {
                self.handle_coordinator(leader_id, leader_addr).await;
            }
            Message::Join { node_id, addr } => {
                self.handle_join(node_id, addr).await;
            }
            Message::ClusterView { members } => {
                self.handle_cluster_view(members).await;
            }
        }
    }

    /// Main async event loop for any node role (Leader / Replica / Station-backed node).
    ///
    /// It multiplexes:
    /// - node-to-node messages (Raft / Bully / cluster membership),
    /// - Station messages (pump simulator),
    /// - Actor events (operation results),
    /// and periodically checks for liveness to decide when to trigger a Bully election.
    async fn run(&mut self) -> AppResult<()> {
        use tokio::time::{interval, Instant};

        let mut check = interval(Duration::from_millis(100));
        let liveness_threshold = Duration::from_millis(300);

        // Timer of last received node-to-node message.
        let mut last_seen = Instant::now();
        todo!();
        loop {
            select! {
                // periodic tick to check liveness
                _ = check.tick() => {
                    let elapsed = last_seen.elapsed();
                    if elapsed >= liveness_threshold {
                        // If it's been a while since we saw messages from peers,
                        // start a Bully election.
                        //
                        // For actual leaders, `start_election` can be a no-op;
                        // for replicas, it should orchestrate `conduct_election`.
                        self.start_election().await;

                        // Update last_seen to avoid continuous restarts.
                        last_seen = Instant::now();
                    }
                }

                // === Node-to-node messages (Raft / Bully / Cluster membership) ===
                node_msg = self.recv_node_msg() => {
                    match node_msg {
                        Ok(msg) => {
                            // Any valid node message counts as liveness.
                            last_seen = Instant::now();
                            self.handle_node_msg(msg).await;
                        }
                        Err(_e) => {
                            // Network error receiving a node message.
                            // Concrete implementation may want to log this.
                            // For now, we just keep looping.
                            //
                            // TODO: consider treating repeated errors as a liveness issue too.
                        }
                    }
                }

                // === Station (pump simulator) messages ===
                pump_msg = self.recv_station_message() => {
                    match pump_msg {
                        Some(msg) => {
                            self.handle_station_msg(msg).await;
                        }
                        None => {
                            // Station side closed its channel.
                            // Depending on design, this might be fine (no more pumps)
                            // or should terminate the node. For now, we TODO.
                            todo!();
                        }
                    }
                }

                // === Actor events (OperationResult, etc.) ===
                actor_evt = self.recv_actor_event() => {
                    match actor_evt {
                        Some(evt) => {
                            self.handle_actor_event(evt).await;
                        }
                        None => {
                            // Actor system stopped; for now, we treat it as a fatal condition.
                            todo!();
                        }
                    }
                }
            }
        }
    }
}

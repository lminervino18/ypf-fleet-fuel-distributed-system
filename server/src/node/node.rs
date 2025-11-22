use super::{
    actors::ActorEvent, message::Message, operation::Operation, station::StationToNodeMsg,
};
use crate::errors::{AppResult, VerifyError};
use rand::Rng;
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

    async fn handle_actor_event(&mut self, event: ActorEvent) {
        match event {
            ActorEvent::OperationResult {
                op_id,
                operation,
                success,
                error,
            } => {
                self.handle_operation_result(op_id, operation, success, error);
            }
            _ => {
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

    async fn handle_disconnect_node(&mut self) {
        /*         if self.is_offline {
            let _ = self
                .station_result_tx
                .send(NodeToStationMsg::Debug(
                    "[Leader] Node is already in OFFLINE mode; pump operations are auto-approved."
                        .to_string(),
                ))
                .await;
        } else {
            self.is_offline = true;
            let _ = self
                        .station_result_tx
                        .send(NodeToStationMsg::Debug(
                            "[Leader] Node switched to OFFLINE mode. Cluster traffic will be ignored and pump operations will be queued and auto-approved."
                                .to_string(),
                        ))
                        .await;
        } */
    }

    async fn handle_connect_node(&mut self) {
        /*         if !self.is_offline {
            let _ = self
                        .station_result_tx
                        .send(NodeToStationMsg::Debug(
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
                let op = ActorOperation::Charge {
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
                        .station_result_tx
                        .send(NodeToStationMsg::Debug(
                            format!(
                                "[Leader] Node switched back to ONLINE mode. Replayed {} queued offline operations into the actor system (they were already confirmed to the station).",
                                queued
                            ),
                        ))
                        .await;
        } */
    }

    async fn handle_station_msg(&mut self, msg: StationToNodeMsg) {
        match msg {
            StationToNodeMsg::ChargeRequest {
                pump_id,
                account_id,
                card_id,
                amount,
                request_id,
            } => {
                self.handle_charge_request(pump_id, account_id, card_id, amount, request_id);
            }

            StationToNodeMsg::DisconnectNode => {
                self.handle_disconnect_node();
            }

            StationToNodeMsg::ConnectNode => {
                self.handle_connect_node();
            }
        }
    }
    async fn recv_node_msg(&mut self) -> AppResult<Message>;
    async fn recv_actor_event(&mut self) -> Option<ActorEvent>;
    async fn recv_station_message(&mut self) -> Option<StationToNodeMsg>;
    async fn handle_election(&mut self, candidate_id: u64, candidate_addr: SocketAddr);
    async fn handle_election_ok(&mut self, responder_id: u64);
    async fn handle_coordinator(&mut self, leader_id: u64, leader_addr: SocketAddr);
    async fn start_election(&mut self);

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
        }
    }

    async fn run(&mut self) -> AppResult<()> {
        use tokio::time::{interval, Instant};
        let mut check = interval(Duration::from_millis(100));
        let liveness_threshold = Duration::from_millis(300);
        // timer of last received message
        let mut last_seen = Instant::now();
        todo!();

        loop {
            select! {
                // periodic tick to check liveness
                _ = check.tick() => {
                    let elapsed = last_seen.elapsed();
                    if elapsed >= liveness_threshold {
                        // If it's been a while since we saw messages, start election
                        // (start_election should be non-blocking or spawn its own task)
                        self.start_election().await;
                        // update last_seen to avoid continuous restarts
                        last_seen = Instant::now();
                    }
                }
                node_msg = self.recv_node_msg() =>{
                    match node_msg {
                        Ok(msg) => {
                            self.handle_node_msg(msg).await;
                        }
                        _ => { todo!(); }
                    }
                }
                pump_msg = self.recv_station_message() =>{
                    match pump_msg {
                        Some(msg) => {
                            self.handle_station_msg(msg).await;
                        }
                        None => { todo!(); }
                    }
                }
                actor_evt = self.recv_actor_event() => {
                    match actor_evt {
                        Some(evt) => {
                            self.handle_actor_event(evt).await;
                        }
                        None => { todo!(); }
                    }
                }
                // station_msg = self.recv_actor_event() =>{
                //     // TODO
                // }
            }
        }
    }
}

use super::{message::Message, operation::Operation, station::StationToNodeMsg};
use crate::{actors::ActorEvent, errors::AppResult};
use std::net::SocketAddr;
use tokio::select;

/// Role of a node in the YPF Ruta distributed system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeRole {
    Leader,
    Replica,
    Station,
}

pub trait Node {
    async fn handle_request(&mut self, op: Operation, client_addr: SocketAddr);
    async fn handle_log(&mut self, op: Operation);
    async fn handle_ack(&mut self, id: u32);
    async fn recv_node_msg(&mut self) -> AppResult<Message>;
    async fn recv_actor_event(&mut self) -> Option<ActorEvent>;
    async fn handle_actor_event(&mut self, event: ActorEvent);
    async fn handle_station_msg(&mut self, msg: StationToNodeMsg);

    async fn handle_node_msg(&mut self, msg: Message) {
        match msg {
            Message::Request { op, addr } => {
                self.handle_request(op, addr).await;
            }
            Message::Log { op } => {
                self.handle_log(op).await;
            }
            Message::Ack { id } => {
                self.handle_ack(id).await;
            }
        }
    }

    async fn run(&mut self) -> AppResult<()> {
        loop {
            select! {
                node_msg = self.recv_node_msg() =>{
                    match node_msg {
                        Ok(msg) => {
                            match msg {
                                Message::Request { op, addr } => {
                                    self.handle_request(op, addr);
                                }
                                Message::Log { op } => {
                                    self.handle_log(op);
                                }
                                Message::Ack { id } => {
                                    self.handle_ack(id);
                                }
                            }
                        }
                        _ => { return Ok(()); }
                    }
                }
                // station_msg = self.recv_actor_event() =>{
                //     // TODO
                // }

            }
        }
    }
}

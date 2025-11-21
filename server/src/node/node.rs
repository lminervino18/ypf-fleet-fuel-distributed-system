use super::{message::Message, operation::Operation};
use crate::errors::AppResult;
use std::net::SocketAddr;

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
        while let Ok(msg) = self.recv_node_msg().await {
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

        Ok(())
    }
}

use super::{connection_manager::InboundEvent, node_message::NodeMessage, operation::Operation};
use crate::errors::AppResult;
use std::future::pending;

/// Role of a node in the YPF Ruta distributed system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeRole {
    Leader,
    Replica,
    Station,
}

/// Distributed node participating in the YPF Ruta system.
///
/// A Node encapsulates:
/// - networking (TCP) via the ConnectionManager,
/// - a local ActorRouter (Actix) for application logic,
/// - role-specific behaviour (leader, replica, station).
///
/// The trait delegates the main execution to `run_loop()`,
/// which is implemented by each concrete node type.
pub trait Node {
    async fn handle_request(&mut self, op: Operation);
    async fn handle_log(&mut self, op: Operation);
    async fn handle_ack(&mut self, id: u32);
    async fn recv_node_msg(&mut self) -> Option<InboundEvent>;

    async fn handle_node_msg(&mut self, msg: NodeMessage) {
        match msg {
            NodeMessage::Request { op } => {
                self.handle_request(op).await;
            }
            NodeMessage::Log { op } => {
                self.handle_log(op);
            }
            NodeMessage::Ack { id } => {
                self.handle_ack(id);
            }
        }
    }

    async fn run(&mut self) -> AppResult<()> {
        while let Some(evt) = self.recv_node_msg().await {
            match evt {
                InboundEvent::Received { peer, payload } => {
                    self.handle_node_msg(payload.try_into()?).await;
                }
                InboundEvent::ConnClosed { peer } => {
                    println!("[INFO] Connection closed by {}", peer);
                }
            }
        }

        pending::<()>().await;
        Ok(())
    }
}

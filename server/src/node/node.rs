use super::{connection_manager::InboundEvent, node_message::NodeMessage, operation::Operation};
use crate::errors::AppResult;
use std::future::pending;

/// Role of a node in the YPF Ruta distributed system.
///
/// - Leader: coordinates replicas and accepts connections from stations/replicas.
/// - Replica: connects to a leader and acts as a passive replica.
/// - Station: edge node that represents a physical station and forwards requests.
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
/// The Node is responsible for wiring background tasks that bridge the async
/// Tokio runtime (network IO) and the Actix actor system (application logic).
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

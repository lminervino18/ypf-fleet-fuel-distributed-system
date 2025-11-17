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
    async fn handle_accept(&mut self, op: Operation);
    async fn handle_learn(&mut self, op: Operation);
    async fn handle_commit(&mut self, op: Operation);
    async fn handle_finished(&mut self, op: Operation);
    async fn recv_node_msg(&mut self) -> Option<InboundEvent>;

    async fn handle_node_msg(&mut self, msg: NodeMessage) {
        match msg {
            NodeMessage::Accept(op) => {
                self.handle_accept(op).await;
            }
            NodeMessage::Learn(op) => {
                self.handle_learn(op);
            }
            NodeMessage::Commit(op) => {
                self.handle_commit(op);
            }
            NodeMessage::Finished(op) => {
                self.handle_finished(op);
            }
            _ => {}
        }
    }

    async fn run(&mut self) -> AppResult<()> {
        while let Some(evt) = self.recv_node_msg().await {
            match evt {
                InboundEvent::Received { peer, payload } => {
                    self.handle_node_msg(payload.try_into().unwrap()).await;
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

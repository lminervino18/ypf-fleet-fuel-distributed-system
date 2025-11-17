use super::{connection_manager::InboundEvent, node_message::NodeMessage, operation::Operation};
use crate::actors::types::ActorEvent;
use crate::errors::AppResult;

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
    // ==========
    // Consensus operations (network driven)
    // ==========
    async fn handle_accept(&mut self, op: Operation);
    async fn handle_learn(&mut self, op: Operation);
    async fn handle_commit(&mut self, op: Operation);
    async fn handle_finished(&mut self, op: Operation);

    /// Receive the next inbound event from the network layer (ConnectionManager).
    async fn recv_node_msg(&mut self) -> Option<InboundEvent>;

    // ==========
    // Actor-driven events (ActorRouter → Node)
    // ==========

    /// Receive the next event from the ActorRouter (default: no actor events).
    async fn recv_actor_event(&mut self) -> Option<ActorEvent> {
        None
    }

    /// Handle an ActorEvent coming from the actor system.
    ///
    /// Default: do nothing. Leader/Replica override this when needed.
    async fn handle_actor_event(&mut self, _event: ActorEvent) {}

    // ==========
    // Helpers
    // ==========

    async fn handle_node_msg(&mut self, msg: NodeMessage) {
        match msg {
            NodeMessage::Accept(op) => self.handle_accept(op).await,
            NodeMessage::Learn(op) => self.handle_learn(op).await,
            NodeMessage::Commit(op) => self.handle_commit(op).await,
            NodeMessage::Finished(op) => self.handle_finished(op).await,
            _ => { /* ignore for now */ }
        }
    }

    // ==========
    // Event loop — implemented by Leader/Replica/Station
    // ==========
    async fn run_loop(&mut self) -> AppResult<()>;

    /// Wrapper so callers call `node.run()` but each concrete type implements run_loop().
    async fn run(&mut self) -> AppResult<()> {
        self.run_loop().await
    }
}

use actix::prelude::*;
use log::{info, warn};

use crate::errors::{AppError, AppResult};
use super::actor_router::ActorRouter;
use super::types::ActorMsg;

/// Subscriber actor:
/// - Represents a local replica for a given `card_id` on this node.
/// - There can be multiple `Subscriber` actors (one per node) for the same card.
/// - In distributed terms, it handles replication, updates, and event notifications.
pub struct Subscriber {
    pub card_id: u64,
    pub actor_router: Addr<ActorRouter>,
}

impl Subscriber {
    /// Create a new local Subscriber.
    pub fn new(card_id: u64, actor_router: Addr<ActorRouter>) -> Self {
        Self { card_id, actor_router }
    }

    /// Process incoming domain messages.
    /// Returns `AppResult<()>` for structured error handling.
    fn process_message(&mut self, msg: ActorMsg) -> AppResult<()> {
        match msg {
            ActorMsg::Placeholder(content) => {
                info!(
                    "[Subscriber card={}] received placeholder message: {}",
                    self.card_id, content
                );

                // Validate message content
                if content.trim().is_empty() {
                    return Err(AppError::InvalidData {
                        details: format!("Empty or malformed message for Subscriber {}", self.card_id),
                    });
                }

                // Here you could handle specific commands, like:
                // - ActorMsg::SyncState { ... }
                // - ActorMsg::ApplyDelta { ... }
                // - ActorMsg::Acknowledge { ... }
                info!("[Subscriber card={}] processed message OK", self.card_id);
            }
        }

        Ok(())
    }
}

impl Actor for Subscriber {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("[Subscriber card={}] started", self.card_id);
    }
}

/// Handles messages routed from the ActorRouter.
/// Keeps the actor alive even on recoverable errors.
impl Handler<ActorMsg> for Subscriber {
    type Result = ();

    fn handle(&mut self, msg: ActorMsg, _ctx: &mut Context<Self>) -> Self::Result {
        match self.process_message(msg) {
            Ok(_) => { /* success */ }
            Err(e) => {
                warn!(
                    "[Subscriber card={}][WARN] Failed to process message: {:?}",
                    self.card_id, e
                );
            }
        }
    }
}

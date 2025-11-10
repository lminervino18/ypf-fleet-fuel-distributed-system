use actix::prelude::*;
use log::{info, warn, error};

use crate::errors::{AppError, AppResult};
use super::actor_router::ActorRouter;
use super::types::ActorMsg;

/// LeaderCard actor: represents a "cluster leader" for a given card_id.
/// Each LeaderCard actor manages coordination and high-level logic for that card.
pub struct LeaderCard {
    pub card_id: u64,
    pub actor_router: Addr<ActorRouter>,
}

impl LeaderCard {
    /// Create a new leader actor for this card.
    pub fn new(card_id: u64, actor_router: Addr<ActorRouter>) -> Self {
        Self { card_id, actor_router }
    }

    /// Core business logic for message handling.
    /// Returns `AppResult<()>` so structured errors can be logged and analyzed.
    fn process_message(&mut self, msg: ActorMsg) -> AppResult<()> {
        match msg {
            ActorMsg::Placeholder(content) => {
                info!("[LeaderCard {}] received placeholder message: {}", self.card_id, content);

                if content.trim().is_empty() {
                    return Err(AppError::InvalidData {
                        details: format!("Empty message for LeaderCard {}", self.card_id),
                    });
                }

                // Here you could later add validation for commands like
                // ActorMsg::UpdateCardState, ActorMsg::SyncCluster, etc.
                info!("[LeaderCard {}] processed message successfully", self.card_id);
            }
        }

        Ok(())
    }
}

impl Actor for LeaderCard {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("[LeaderCard {}] started", self.card_id);
    }
}

/// Message handler for domain-level messages coming from the router.
/// Uses structured error handling (non-fatal for recoverable issues).
impl Handler<ActorMsg> for LeaderCard {
    type Result = ();

    fn handle(&mut self, msg: ActorMsg, _ctx: &mut Context<Self>) -> Self::Result {
        match self.process_message(msg) {
            Ok(_) => { /* success */ }
            Err(e) => {
                warn!(
                    "[LeaderCard {}][WARN] Failed to process message: {:?}",
                    self.card_id, e
                );
            }
        }
    }
}

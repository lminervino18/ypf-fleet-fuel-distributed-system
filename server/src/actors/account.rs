use actix::prelude::*;
use log::{info, warn};

use crate::errors::{AppError, AppResult};
use super::actor_router::ActorRouter;
use super::types::ActorMsg;

/// Account actor:
/// - Represents a single logical account in the distributed system.
/// - Handles messages related to account activity or state updates.
/// - One `Account` actor exists per account_id (cluster-wide unique).
pub struct Account {
    pub account_id: u64,
    pub actor_router: Addr<ActorRouter>,
}

impl Account {
    /// Creates a new `Account` actor instance.
    pub fn new(account_id: u64, actor_router: Addr<ActorRouter>) -> Self {
        Self { account_id, actor_router }
    }

    /// Handles business logic for account-related messages.
    /// Returns an `AppResult<()>` for structured error propagation.
    fn process_message(&mut self, msg: ActorMsg) -> AppResult<()> {
        match msg {
            ActorMsg::Placeholder(data) => {
                info!(
                    "[Account {}] received placeholder message: {}",
                    self.account_id, data
                );

                // Validate message contents
                if data.trim().is_empty() {
                    return Err(AppError::InvalidData {
                        details: format!("Empty or malformed message for Account {}", self.account_id),
                    });
                }

                // Example of domain behavior — expand later:
                // - Update balance
                // - Trigger notification
                // - Persist transaction, etc.
                info!("[Account {}] processed message successfully", self.account_id);
            }
        }

        Ok(())
    }
}

impl Actor for Account {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("[Account {}] started", self.account_id);
    }
}

/// Message handler for domain-level messages routed by the ActorRouter.
/// Uses structured `AppResult` errors and logs gracefully on failure.
impl Handler<ActorMsg> for Account {
    type Result = ();

    fn handle(&mut self, msg: ActorMsg, _ctx: &mut Context<Self>) -> Self::Result {
        match self.process_message(msg) {
            Ok(_) => { /* success */ }
            Err(e) => {
                warn!(
                    "[Account {}][WARN] Failed to process message: {:?}",
                    self.account_id, e
                );
            }
        }
    }
}

//! Actor responsible for the lifecycle and messages of a single card.
//!
//! Each CardActor owns the state and logic for one card (physical or virtual)
//! belonging to an account. It receives authorization, debit and update messages
//! and coordinates with the router or parent account actor as needed.

use actix::prelude::*;
use crate::errors::{AppError, AppResult};
use super::types::ActorMsg;

/// Represents a card (physical or logical) belonging to an account.
///
/// The actor handles per-card responsibilities such as processing
/// authorization requests, applying local updates, and tracking TTL/version
/// state (not implemented here).
pub struct CardActor {
    pub card_id: u64,
    pub account_id: u64,
    pub router: Addr<super::actor_router::ActorRouter>,
}

impl CardActor {
    /// Construct a new CardActor bound to `card_id` and `account_id`.
    pub fn new(card_id: u64, account_id: u64, router: Addr<super::actor_router::ActorRouter>) -> Self {
        Self {
            card_id,
            account_id,
            router,
        }
    }
}

impl Actor for CardActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        println!(
            "[Card {}@Account {}] Started",
            self.card_id, self.account_id
        );
    }
}

impl Handler<ActorMsg> for CardActor {
    type Result = ();

    fn handle(&mut self, msg: ActorMsg, _ctx: &mut Context<Self>) {
        match msg {
            ActorMsg::Placeholder(text) => {
                println!(
                    "[Card {}@Account {}] Received message: {}",
                    self.card_id, self.account_id, text
                );
            }
            _ => {
                println!(
                    "[Card {}@Account {}] Unknown message received",
                    self.card_id, self.account_id
                );
            }
        }
    }
}

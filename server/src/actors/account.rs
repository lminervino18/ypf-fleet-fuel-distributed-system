//! Actor that manages a local account and its card actors.
//!
//! An AccountActor maintains child CardActor instances (one per active card)
//! and routes messages to them. It encapsulates per-account state and provides
//! a point to aggregate or validate operations across cards.

use actix::prelude::*;
use std::collections::HashMap;

use crate::errors::{AppError, AppResult};
use super::card::CardActor;
use super::types::ActorMsg;

/// Represents a local YPF Ruta account.
///
/// Each account can have multiple active card actors. The AccountActor is
/// responsible for creating and routing to per-card actors on demand.
pub struct AccountActor {
    pub account_id: u64,
    pub cards: HashMap<u64, Addr<CardActor>>,
    pub router: Addr<super::actor_router::ActorRouter>,
}

impl AccountActor {
    /// Create a new AccountActor bound to `account_id`.
    pub fn new(account_id: u64, router: Addr<super::actor_router::ActorRouter>) -> Self {
        Self {
            account_id,
            cards: HashMap::new(),
            router,
        }
    }

    /// Get an existing CardActor for `card_id` or create one if missing.
    fn get_or_create_card(&mut self, card_id: u64, ctx: &mut Context<Self>) -> Addr<CardActor> {
        if let Some(addr) = self.cards.get(&card_id) {
            return addr.clone();
        }
        println!("[Account {}] Creating new card {}", self.account_id, card_id);
        let addr = CardActor::new(card_id, self.account_id, self.router.clone()).start();
        self.cards.insert(card_id, addr.clone());
        addr
    }
}

impl Actor for AccountActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        println!("[Account {}] Started", self.account_id);
    }
}

impl Handler<ActorMsg> for AccountActor {
    type Result = ();

    fn handle(&mut self, msg: ActorMsg, ctx: &mut Context<Self>) {
        match msg {
            ActorMsg::CardMessage { card_id, msg } => {
                let card = self.get_or_create_card(card_id, ctx);
                card.do_send(*msg); // Box<ActorMsg> is dereferenced before sending
            }
            ActorMsg::Placeholder(text) => {
                println!("[Account {}] Generic message: {}", self.account_id, text);
            }
        }
    }
}


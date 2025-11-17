//! Actor router module.
//!
//! Flows:
//! - Node → ActorRouter : `RouterCmd`
//! - ActorRouter → CardActor : `CardMsg` (for CheckLimit, ApplyCharge, card limit updates)
//! - ActorRouter → AccountActor : `AccountMsg` (for account limit updates)
//! - CardActor → AccountActor : `AccountMsg::CheckAccountLimit` (when card passes)
//! - AccountActor / CardActor → ActorRouter : `RouterInternalMsg`
//! - ActorRouter → Node : `ActorEvent` (via mpsc channel)

use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::account::AccountActor;
use super::card::CardActor;
use super::types::{
    AccountMsg, ActorEvent, CardMsg, RouterCmd, RouterInternalMsg,
};

/// Router actor that owns and routes to AccountActor and CardActor instances.
pub struct ActorRouter {
    /// account_id -> AccountActor address
    pub accounts: HashMap<u64, Addr<AccountActor>>,
    /// card_id -> CardActor address
    pub cards: HashMap<u64, Addr<CardActor>>,

    /// Channel ActorRouter → Node
    pub event_tx: mpsc::Sender<ActorEvent>,
}

impl ActorRouter {
    pub fn new(event_tx: mpsc::Sender<ActorEvent>) -> Self {
        Self {
            accounts: HashMap::new(),
            cards: HashMap::new(),
            event_tx,
        }
    }

    /// Helper to emit an event to the Node.
    fn emit(&self, ev: ActorEvent) {
        let _ = self.event_tx.try_send(ev);
    }

    /// Return or create an AccountActor.
    fn get_or_create_account(
        &mut self,
        account_id: u64,
        ctx: &mut Context<Self>,
    ) -> Addr<AccountActor> {
        if let Some(addr) = self.accounts.get(&account_id) {
            return addr.clone();
        }

        println!("[Router] Creating local account {}", account_id);

        let addr = AccountActor::new(account_id, ctx.address()).start();
        self.accounts.insert(account_id, addr.clone());
        addr
    }

    /// Return or create a CardActor.
    ///
    /// A card always belongs to an account, so we ensure the account exists first.
    fn get_or_create_card(
        &mut self,
        account_id: u64,
        card_id: u64,
        ctx: &mut Context<Self>,
    ) -> Addr<CardActor> {
        if let Some(addr) = self.cards.get(&card_id) {
            return addr.clone();
        }

        let account_addr = self.get_or_create_account(account_id, ctx);

        println!(
            "[Router] Creating card {} for account {}",
            card_id, account_id
        );

        let addr =
            CardActor::new(card_id, account_id, ctx.address(), account_addr).start();

        self.cards.insert(card_id, addr.clone());
        addr
    }
}

impl Actor for ActorRouter {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        println!(
            "[Router] Started with {} accounts and {} cards",
            self.accounts.len(),
            self.cards.len()
        );
    }
}

/// Handle commands sent from Node → ActorRouter
impl Handler<RouterCmd> for ActorRouter {
    type Result = ();

    fn handle(&mut self, msg: RouterCmd, ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            RouterCmd::AuthorizeCharge {
                account_id,
                card_id,
                amount,
                request_id,
            } => {
                // IMPORTANT: optimization path
                // We go directly to the CardActor, not the account.
                let card = self.get_or_create_card(account_id, card_id, ctx);

                card.do_send(CardMsg::CheckLimit {
                    amount,
                    request_id,
                    account_id,
                    card_id,
                });
            }

            RouterCmd::ApplyCharge {
                account_id,
                card_id,
                amount,
                timestamp,
                op_id,
                request_id: _,
            } => {
                // Start from the card: card updates its own usage,
                // then the account updates account-wide usage and emits ChargeApplied.
                let card = self.get_or_create_card(account_id, card_id, ctx);
                card.do_send(CardMsg::ApplyCharge { amount });

                let account = self.get_or_create_account(account_id, ctx);
                account.do_send(AccountMsg::ApplyCharge {
                    card_id,
                    amount,
                    request_id: 0, // correlation can be done via op_id/timestamp if needed
                    timestamp,
                    op_id,
                });
            }

            RouterCmd::UpdateCardLimit {
                account_id,
                card_id,
                new_limit,
                request_id,
            } => {
                let card = self.get_or_create_card(account_id, card_id, ctx);

                card.do_send(CardMsg::UpdateLimit {
                    new_limit,
                    request_id,
                });
            }

            RouterCmd::UpdateAccountLimit {
                account_id,
                new_limit,
                request_id,
            } => {
                let account = self.get_or_create_account(account_id, ctx);

                account.do_send(AccountMsg::UpdateAccountLimit {
                    new_limit,
                    request_id,
                });
            }

            RouterCmd::ListAccounts => {
                println!("[Router] Accounts: {:?}", self.accounts.keys());
            }
        }
    }
}

/// Handle messages from AccountActor / CardActor → ActorRouter
impl Handler<RouterInternalMsg> for ActorRouter {
    type Result = ();

    fn handle(&mut self, msg: RouterInternalMsg, _ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            RouterInternalMsg::LimitCheckResult {
                request_id,
                allowed,
                error,
            } => {
                self.emit(ActorEvent::LimitCheckResult {
                    request_id,
                    allowed,
                    error,
                });
            }

            RouterInternalMsg::ChargeApplied { operation } => {
                self.emit(ActorEvent::ChargeApplied { operation });
            }

            RouterInternalMsg::LimitUpdated {
                scope,
                account_id,
                card_id,
                new_limit,
            } => {
                self.emit(ActorEvent::LimitUpdated {
                    scope,
                    account_id,
                    card_id,
                    new_limit,
                });
            }

            RouterInternalMsg::LimitUpdateFailed {
                scope,
                account_id,
                card_id,
                request_id,
                error,
            } => {
                self.emit(ActorEvent::LimitUpdateFailed {
                    scope,
                    account_id,
                    card_id,
                    request_id,
                    error,
                });
            }

            RouterInternalMsg::Debug(msg) => {
                self.emit(ActorEvent::Debug(msg));
            }
        }
    }
}

use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::actors::messages::{
    AccountMsg,
    ActorEvent,
    CardMsg,
    RouterCmd,
    RouterInternalMsg,
};
use crate::domain::Operation;
use crate::errors::ApplyError;

use super::account::AccountActor;
use super::card::CardActor;


/// Internal state used to aggregate `ApplyResult` coming from multiple actors
/// (e.g. Card + Account for a `Charge`). Tracks expected acknowledgements and
/// aggregates success/error for multi-actor operations.
#[derive(Debug)]
struct PendingApply {
    expected_acks: u8,
    received_acks: u8,
    success: bool,
    error: Option<ApplyError>,
}

/// Router actor that owns and routes to AccountActor and CardActor instances.
///
/// Responsibilities:
/// - Maintain and create AccountActor and CardActor instances as needed.
/// - Route Verify/Apply requests to the correct actors.
/// - Aggregate results for multi-actor operations (e.g. charge).
/// - Emit ActorEvent messages to the Node via event_tx.
pub struct ActorRouter {
    /// Maps account_id to AccountActor address.
    pub accounts: HashMap<u64, Addr<AccountActor>>,

    /// Maps (account_id, card_id) to CardActor address.
    pub cards: HashMap<(u64, u64), Addr<CardActor>>,

    /// Maps op_id to Operation for correlation.
    pub operations: HashMap<u64, Operation>,

    /// Tracks pending Apply operations and their expected acknowledgements.
    pending_apply: HashMap<u64, PendingApply>,

    /// Channel for sending ActorEvent messages to the Node.
    pub event_tx: mpsc::Sender<ActorEvent>,
}

impl ActorRouter {
    /// Create a new ActorRouter with the given event channel.
    pub fn new(event_tx: mpsc::Sender<ActorEvent>) -> Self {
        Self {
            accounts: HashMap::new(),
            cards: HashMap::new(),
            operations: HashMap::new(),
            pending_apply: HashMap::new(),
            event_tx,
        }
    }

    /// Emit an ActorEvent to the Node.
    fn emit(&self, ev: ActorEvent) {
        let _ = self.event_tx.try_send(ev);
    }

    /// Return or create an AccountActor for the given account_id.
    fn get_or_create_account(
        &mut self,
        account_id: u64,
        ctx: &mut Context<Self>,
    ) -> Addr<AccountActor> {
        if let Some(a) = self.accounts.get(&account_id) {
            return a.clone();
        }

        let addr = AccountActor::new(account_id, ctx.address()).start();
        self.accounts.insert(account_id, addr.clone());
        addr
    }

    /// Return or create a CardActor for the given account_id and card_id.
    ///
    /// Card identity is the pair (account_id, card_id).
    fn get_or_create_card(
        &mut self,
        account_id: u64,
        card_id: u64,
        ctx: &mut Context<Self>,
    ) -> Addr<CardActor> {
        let key = (account_id, card_id);

        if let Some(c) = self.cards.get(&key) {
            return c.clone();
        }

        let account_addr = self.get_or_create_account(account_id, ctx);
        let addr = CardActor::new(card_id, account_id, ctx.address(), account_addr).start();

        self.cards.insert(key, addr.clone());
        addr
    }

    /// Register a pending apply operation.
    /// Use 1 ack for limit operations, 2 for charge operations.
    fn register_pending_apply(&mut self, op_id: u64, expected_acks: u8) {
        self.pending_apply.insert(
            op_id,
            PendingApply {
                expected_acks,
                received_acks: 0,
                success: true,
                error: None,
            },
        );
    }
}

impl Actor for ActorRouter {
    type Context = Context<Self>;
}

// ----------------------------
// Node → Router
// ----------------------------
impl Handler<RouterCmd> for ActorRouter {
    type Result = ();

    fn handle(&mut self, msg: RouterCmd, ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            // ---------------------------------
            // VERIFY
            // ---------------------------------
            RouterCmd::Verify { op_id, operation } => {
                self.operations.insert(op_id, operation.clone());

                match operation {
                    Operation::Charge {
                        account_id,
                        card_id,
                        amount,
                    } => {
                        let card = self.get_or_create_card(account_id, card_id, ctx);
                        card.do_send(CardMsg::VerifyCharge {
                            amount,
                            op_id,
                            account_id,
                            card_id,
                        });
                    }

                    Operation::LimitAccount {
                        account_id,
                        new_limit,
                    } => {
                        let acc = self.get_or_create_account(account_id, ctx);
                        acc.do_send(AccountMsg::VerifyLimitChange { new_limit, op_id });
                    }

                    Operation::LimitCard {
                        account_id,
                        card_id,
                        new_limit,
                    } => {
                        let card = self.get_or_create_card(account_id, card_id, ctx);
                        card.do_send(CardMsg::VerifyLimitChange { new_limit, op_id });
                    }
                }
            }

            // ---------------------------------
            // APPLY
            // ---------------------------------
            RouterCmd::Apply { op_id, operation } => {
                self.operations.insert(op_id, operation.clone());

                match operation {
                    Operation::Charge {
                        account_id,
                        card_id,
                        amount,
                    } => {
                        // Charge: Apply to Card + Account (2 acks expected).
                        self.register_pending_apply(op_id, 2);

                        let card = self.get_or_create_card(account_id, card_id, ctx);
                        card.do_send(CardMsg::ApplyCharge { amount, op_id });

                        let acc = self.get_or_create_account(account_id, ctx);
                        acc.do_send(AccountMsg::ApplyCharge {
                            card_id,
                            amount,
                            op_id,
                        });
                    }

                    Operation::LimitAccount {
                        account_id,
                        new_limit,
                    } => {
                        self.register_pending_apply(op_id, 1);

                        let acc = self.get_or_create_account(account_id, ctx);
                        acc.do_send(AccountMsg::ApplyLimitChange { new_limit, op_id });
                    }

                    Operation::LimitCard {
                        account_id,
                        card_id,
                        new_limit,
                    } => {
                        self.register_pending_apply(op_id, 1);

                        let card = self.get_or_create_card(account_id, card_id, ctx);
                        card.do_send(CardMsg::ApplyLimitChange { new_limit, op_id });
                    }
                }
            }

            RouterCmd::GetLog => {
                let msg = format!(
                    "[Router] Accounts={}, Cards={}",
                    self.accounts.len(),
                    self.cards.len()
                );
                self.emit(ActorEvent::Debug(msg));
            }
        }
    }
}

// ----------------------------
// CardActor / AccountActor → Router
// ----------------------------
impl Handler<RouterInternalMsg> for ActorRouter {
    type Result = ();

    fn handle(&mut self, msg: RouterInternalMsg, _ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            RouterInternalMsg::VerifyResult {
                op_id,
                allowed,
                error,
            } => {
                if let Some(op) = self.operations.get(&op_id) {
                    self.emit(ActorEvent::VerifyResult {
                        op_id,
                        operation: op.clone(),
                        allowed,
                        error,
                    });
                } else {
                    self.emit(ActorEvent::Debug(format!(
                        "[Router] VerifyResult for unknown op_id={}",
                        op_id
                    )));
                }
            }

            RouterInternalMsg::ApplyResult {
                op_id,
                success,
                error,
            } => {
                let pending = match self.pending_apply.get_mut(&op_id) {
                    Some(p) => p,
                    None => {
                        self.emit(ActorEvent::Debug(format!(
                            "[Router] ApplyResult for non-pending op_id={}",
                            op_id
                        )));
                        return;
                    }
                };

                pending.received_acks += 1;
                if !success {
                    pending.success = false;
                    if pending.error.is_none() {
                        pending.error = error.clone();
                    }
                }

                if pending.received_acks >= pending.expected_acks {
                    let pending = self.pending_apply.remove(&op_id).unwrap();

                    if let Some(op) = self.operations.get(&op_id) {
                        self.emit(ActorEvent::ApplyResult {
                            op_id,
                            operation: op.clone(),
                            success: pending.success,
                            error: pending.error,
                        });
                    }
                }
            }

            RouterInternalMsg::Debug(msg) => {
                self.emit(ActorEvent::Debug(msg));
            }
        }
    }
}

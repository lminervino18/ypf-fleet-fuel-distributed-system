//! CardActor: manages per-card limit and per-card consumption.
//!
//! This actor is responsible for enforcing *card-level* limits and
//! tracking consumption per card. It also provides a small per-card
//! queue so that concurrent operations on the same card are serialized
//! in a robust way.
//!
//! Important design choice:
//! - If the card is *busy* (there is an in-flight operation waiting
//!   for an AccountActor reply), **new requests are only enqueued**.
//!   They are not evaluated immediately, because the final consumption
//!   of the card depends on how the previous operations complete.
//!
//! Flows for `Charge`:
//! - Router → CardActor: `CardMsg::ExecuteCharge`
//! - CardActor:
//!     * if idle:
//!         - checks local card-limit against the **current** consumed
//!           amount (no assumptions about queued operations),
//!         - if the check fails → immediately notifies the router,
//!         - if the check passes → starts the charge as the current
//!           task and sends `AccountMsg::ApplyChargeFromCard`.
//!     * if busy:
//!         - simply enqueues the charge (no limit check yet).
//! - AccountActor verifies + applies the charge at account level and
//!   replies with `AccountChargeReply`.
//! - CardActor, upon `AccountChargeReply`:
//!     * on success:
//!         - applies the charge locally (card consumption),
//!     * on failure:
//!         - does **not** change its own consumption,
//!     * in both cases:
//!         - notifies the router with `RouterInternalMsg::OperationCompleted`,
//!         - pops and starts the next queued task, if any.
//!
//! Flows for `LimitCard`:
//! - Router → CardActor: `CardMsg::ExecuteLimitChange`
//! - CardActor:
//!     * if idle:
//!         - checks that the new limit is not below the **current**
//!           consumed amount,
//!         - if allowed, updates the limit and reports success,
//!         - otherwise reports a limit-update error;
//!     * if busy:
//!         - enqueues the limit-change operation and will process it
//!           only after all prior operations (charges or limits) have
//!           completed, so it sees the final, up-to-date consumption.
//!
//! With this design, intra-node concurrency is robust and easy to reason about:
//! - per card: at most one operation (charge or limit change) is in-flight;
//!   the rest are queued and processed in order,
//! - per account: all operations are serialized by the AccountActor
//!   mailbox, so account-wide limits cannot be overspent.

use actix::prelude::*;
use std::collections::VecDeque;

use super::account::AccountActor;
use super::actor_router::ActorRouter;
use super::messages::{
    AccountChargeReply,
    AccountMsg,
    CardMsg,
    RouterInternalMsg,
};
use crate::errors::{LimitCheckError, LimitUpdateError, VerifyError};

/// Internal representation of a card-level charge.
#[derive(Debug, Clone, Copy)]
struct CardChargeOp {
    op_id: u64,
    account_id: u64,
    amount: f64,
}

/// Internal tasks that a card can execute.
///
/// We treat charges and limit changes as items in a single per-card
/// queue, so they are processed strictly one at a time, in order.
#[derive(Debug, Clone, Copy)]
enum CardTask {
    Charge(CardChargeOp),
    LimitChange { op_id: u64, new_limit: Option<f64> },
}

/// Actor responsible for a single card's state and limit enforcement.
pub struct CardActor {
    pub card_id: u64,
    pub account_id: u64,

    /// Per-card limit. None means "no limit".
    limit: Option<f64>,

    /// Total amount consumed by this card (already applied).
    consumed: f64,

    /// Optional in-flight task currently being processed.
    current_task: Option<CardTask>,

    /// Queue of tasks (charges or limit changes) waiting to be processed.
    queue: VecDeque<CardTask>,

    /// Back-reference to the router for reporting results.
    router: Addr<ActorRouter>,

    /// Back-reference to the account actor for account-limit checks and updates.
    account: Addr<AccountActor>,
}

impl CardActor {
    pub fn new(
        card_id: u64,
        account_id: u64,
        router: Addr<ActorRouter>,
        account: Addr<AccountActor>,
    ) -> Self {
        Self {
            card_id,
            account_id,
            limit: Some(50.0),
            consumed: 0.0,
            current_task: None,
            queue: VecDeque::new(),
            router,
            account,
        }
    }

    /// Send an internal message to the router.
    fn send_internal(&self, msg: RouterInternalMsg) {
        self.router.do_send(msg);
    }

    /// Check the card-level limit for a *single* charge, using the
    /// current applied consumption only.
    ///
    /// We do not try to predict the effect of queued operations or
    /// assume they will succeed; each task is validated when it
    /// actually becomes the current task.
    fn check_charge_limit(&self, amount: f64) -> Result<(), LimitCheckError> {
        if let Some(limit) = self.limit {
            if self.consumed + amount > limit {
                return Err(LimitCheckError::CardLimitExceeded);
            }
        }
        Ok(())
    }

    /// Check whether we can set a new card limit, given the current
    /// applied consumption.
    ///
    /// Note: we only consider `self.consumed` here. Previous operations
    /// must have already completed (or will complete before this
    /// task is evaluated), so they either contributed to `consumed`
    /// or were rejected.
    fn can_set_new_limit(
        &self,
        new_limit: Option<f64>,
    ) -> Result<(), LimitUpdateError> {
        match new_limit {
            None => Ok(()), // "no limit" is always allowed
            Some(lim) if lim >= self.consumed => Ok(()),
            Some(_) => Err(LimitUpdateError::BelowCurrentUsage),
        }
    }

    /// Start processing the current task, if any.
    ///
    /// - For charges:
    ///     * validate the card limit with current `consumed`,
    ///     * if OK, send `AccountMsg::ApplyChargeFromCard`,
    ///     * if not OK, finish immediately with error.
    ///
    /// - For limit changes:
    ///     * validate the new limit with current `consumed`,
    ///     * apply or reject and finish immediately.
    fn start_current_task(&mut self, ctx: &mut Context<Self>) {
        let task = match self.current_task {
            Some(t) => t,
            None => return,
        };

        match task {
            CardTask::Charge(op) => {
                // Check card-level limit right before hitting the account.
                let res = self
                    .check_charge_limit(op.amount)
                    .map_err(VerifyError::ChargeLimit);

                if let Err(err) = res {
                    // Card-level limit rejected the charge; do not touch
                    // the account or any state, just report failure and
                    // move on to the next task.
                    self.finish_current_task(op.op_id, false, Some(err), ctx);
                    return;
                }

                let reply_to = ctx.address().recipient::<AccountChargeReply>();

                self.account.do_send(AccountMsg::ApplyChargeFromCard {
                    op_id: op.op_id,
                    amount: op.amount,
                    card_id: self.card_id,
                    reply_to,
                });
            }

            CardTask::LimitChange { op_id, new_limit } => {
                let res = self
                    .can_set_new_limit(new_limit)
                    .map_err(VerifyError::LimitUpdate);

                match res {
                    Ok(()) => {
                        // Safe to apply the new limit immediately.
                        self.limit = new_limit;

                        self.finish_current_task(op_id, true, None, ctx);
                    }
                    Err(err) => {
                        self.finish_current_task(op_id, false, Some(err), ctx);
                    }
                }
            }
        }
    }

    /// When a task (charge or limit-change) has finished (either success
    /// or failure), notify the router and move to the next queued task.
    fn finish_current_task(
        &mut self,
        op_id: u64,
        success: bool,
        error: Option<VerifyError>,
        ctx: &mut Context<Self>,
    ) {
        self.send_internal(RouterInternalMsg::OperationCompleted {
            op_id,
            success,
            error,
        });

        if let Some(next) = self.queue.pop_front() {
            self.current_task = Some(next);
            self.start_current_task(ctx);
        } else {
            self.current_task = None;
        }
    }
}

impl Actor for CardActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        // Uncomment for debugging:
        // println!("[Card {}/Account {}] Started", self.card_id, self.account_id);
    }
}

// ---------------------------------
// Router → CardActor
// ---------------------------------
impl Handler<CardMsg> for CardActor {
    type Result = ();

    fn handle(&mut self, msg: CardMsg, ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            CardMsg::ExecuteCharge {
                op_id,
                account_id,
                card_id,
                amount,
            } => {
                // Basic sanity: the router should not send charges for
                // a different account/card than this actor owns.
                if account_id != self.account_id || card_id != self.card_id {
                    self.send_internal(RouterInternalMsg::Debug(format!(
                        "[Card {}/{}] Received ExecuteCharge for mismatched account/card: account_id={}, card_id={}",
                        self.card_id,
                        self.account_id,
                        account_id,
                        card_id,
                    )));
                    self.send_internal(RouterInternalMsg::OperationCompleted {
                        op_id,
                        success: false,
                        error: Some(VerifyError::ChargeLimit(
                            LimitCheckError::CardLimitExceeded,
                        )),
                    });
                    return;
                }

                let task = CardTask::Charge(CardChargeOp {
                    op_id,
                    account_id,
                    amount,
                });

                if self.current_task.is_none() {
                    // Card is idle → start immediately.
                    self.current_task = Some(task);
                    self.start_current_task(ctx);
                } else {
                    // Card is busy → enqueue and wait until all previous
                    // operations are completed.
                    self.queue.push_back(task);
                }
            }

            CardMsg::ExecuteLimitChange { op_id, new_limit } => {
                let task = CardTask::LimitChange { op_id, new_limit };

                if self.current_task.is_none() {
                    // Card is idle → evaluate the limit change now.
                    self.current_task = Some(task);
                    self.start_current_task(ctx);
                } else {
                    // Card is busy with charges or another limit change.
                    // We enqueue this request so it will be evaluated only
                    // when all prior operations have completed and the
                    // card's consumption is fully up-to-date.
                    self.queue.push_back(task);
                }
            }

            CardMsg::Debug(text) => {
                self.send_internal(RouterInternalMsg::Debug(format!(
                    "[Card {}/{}] {}",
                    self.card_id, self.account_id, text
                )));
            }
        }
    }
}

// ---------------------------------
// AccountActor → CardActor
// ---------------------------------
impl Handler<AccountChargeReply> for CardActor {
    type Result = ();

    fn handle(&mut self, msg: AccountChargeReply, ctx: &mut Context<Self>) -> Self::Result {
        let AccountChargeReply {
            op_id,
            success,
            error,
        } = msg;

        match self.current_task {
            Some(CardTask::Charge(op)) if op.op_id == op_id => {
                if success {
                    // Account accepted and applied the charge; we can now
                    // safely apply it at card level.
                    self.consumed += op.amount;
                }

                self.finish_current_task(op_id, success, error, ctx);
            }

            _ => {
                // Unknown or out-of-sync op_id; do not mutate state.
                self.send_internal(RouterInternalMsg::Debug(format!(
                    "[Card {}/{}] Received AccountChargeReply for unknown op_id={}",
                    self.card_id, self.account_id, op_id
                )));
            }
        }
    }
}

// CardActor: manages per-card limit and per-card consumption.

use actix::prelude::*;
use std::collections::VecDeque;

use super::account::AccountActor;
use super::actor_router::ActorRouter;
use super::messages::{AccountChargeReply, AccountMsg, CardMsg, RouterInternalMsg};
use crate::errors::{LimitCheckError, LimitUpdateError, VerifyError};

/// Internal representation of a card-level charge.
#[derive(Debug, Clone, Copy)]
struct CardChargeOp {
    op_id: u32,
    account_id: u64,
    amount: f32,
    /// Whether this charge originates from a previously OFFLINE station.
    from_offline_station: bool,
}

/// Internal tasks that a card can execute.
#[derive(Debug, Clone, Copy)]
enum CardTask {
    Charge(CardChargeOp),
    LimitChange { op_id: u32, new_limit: Option<f32> },
}

/// Actor responsible for a single card's state and limit enforcement.
pub struct CardActor {
    pub card_id: u64,
    pub account_id: u64,

    /// Per-card limit. None means "no limit".
    limit: Option<f32>,

    /// Total amount consumed by this card (already applied).
    consumed: f32,

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

    /// Check the card-level limit for a *single* charge.
    fn check_charge_limit(&self, amount: f32) -> Result<(), LimitCheckError> {
        if let Some(limit) = self.limit {
            if self.consumed + amount > limit {
                return Err(LimitCheckError::CardLimitExceeded);
            }
        }
        Ok(())
    }

    /// Check whether we can set a new card limit.
    fn can_set_new_limit(&self, new_limit: Option<f32>) -> Result<(), LimitUpdateError> {
        match new_limit {
            None => Ok(()),
            Some(lim) if lim >= self.consumed => Ok(()),
            Some(_) => Err(LimitUpdateError::BelowCurrentUsage),
        }
    }

    /// Start processing the current task, if any.
    fn start_current_task(&mut self, ctx: &mut Context<Self>) {
        let task = match self.current_task {
            Some(t) => t,
            None => return,
        };

        match task {
            CardTask::Charge(op) => {
                if !op.from_offline_station {
                    let res = self
                        .check_charge_limit(op.amount)
                        .map_err(VerifyError::ChargeLimit);

                    if let Err(err) = res {
                        self.finish_current_task(op.op_id, false, Some(err), ctx);
                        return;
                    }
                }

                let reply_to = ctx.address().recipient::<AccountChargeReply>();

                self.account.do_send(AccountMsg::ApplyChargeFromCard {
                    op_id: op.op_id,
                    amount: op.amount,
                    card_id: self.card_id,
                    from_offline_station: op.from_offline_station,
                    reply_to,
                });
            }

            CardTask::LimitChange { op_id, new_limit } => {
                let res = self
                    .can_set_new_limit(new_limit)
                    .map_err(VerifyError::LimitUpdate);

                match res {
                    Ok(()) => {
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

    /// When a task has finished, notify the router and move to the next.
    fn finish_current_task(
        &mut self,
        op_id: u32,
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
                from_offline_station,
            } => {
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
                        error: Some(VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded)),
                    });
                    return;
                }

                let task = CardTask::Charge(CardChargeOp {
                    op_id,
                    account_id,
                    amount,
                    from_offline_station,
                });

                if self.current_task.is_none() {
                    self.current_task = Some(task);
                    self.start_current_task(ctx);
                } else {
                    self.queue.push_back(task);
                }
            }

            CardMsg::ExecuteLimitChange { op_id, new_limit } => {
                let task = CardTask::LimitChange { op_id, new_limit };

                if self.current_task.is_none() {
                    self.current_task = Some(task);
                    self.start_current_task(ctx);
                } else {
                    self.queue.push_back(task);
                }
            }

            CardMsg::QueryCardState { op_id, account_id } => {
                // solo respondemos si la cuenta matchea
                if account_id != self.account_id {
                    self.send_internal(RouterInternalMsg::Debug(format!(
                        "[Card {}/{}] QueryCardState con account_id distinto: {}",
                        self.card_id, self.account_id, account_id
                    )));
                    return;
                }

                // Respondemos a la Account con nuestro consumo actual
                self.account.do_send(AccountMsg::CardQueryReply {
                    op_id,
                    card_id: self.card_id,
                    consumed: self.consumed,
                });
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
                    self.consumed += op.amount;
                }

                self.finish_current_task(op_id, success, error, ctx);
            }

            _ => {
                self.send_internal(RouterInternalMsg::Debug(format!(
                    "[Card {}/{}] Received AccountChargeReply for unknown op_id={}",
                    self.card_id, self.account_id, op_id
                )));
            }
        }
    }
}

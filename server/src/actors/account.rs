//! AccountActor: manages one account and account-wide state.
//!
//! State:
//! - account_limit: optional (None = no limit)
//! - account_consumed: total charged across all cards
//!
//! Flows:
//! - CheckAccountLimit:
//!     Called from CardActor, *only if* the card passed its own limit.
//! - ApplyCharge:
//!     Called after CardActor has updated its card-level consumption.
//!     Updates account_consumed and emits ChargeApplied(Operation::Charge).
//! - UpdateAccountLimit:
//!     Only allow new_limit >= account_consumed.

use actix::prelude::*;

use super::actor_router::ActorRouter;
use super::types::{
    AccountMsg, LimitCheckError, LimitScope, LimitUpdateError, Operation, RouterInternalMsg,
};

pub struct AccountActor {
    pub account_id: u64,

    /// Optional account-wide limit. `None` means "no limit".
    account_limit: Option<f64>,

    /// Total consumption across all cards in this account.
    account_consumed: f64,

    /// Back-reference to the router, to send RouterInternalMsg events.
    router: Addr<ActorRouter>,
}

impl AccountActor {
    pub fn new(account_id: u64, router: Addr<ActorRouter>) -> Self {
        Self {
            account_id,
            account_limit: None,
            account_consumed: 0.0,
            router,
        }
    }

    fn send_internal(&self, msg: RouterInternalMsg) {
        self.router.do_send(msg);
    }

    fn check_account_limit(&self, amount: f64) -> Result<(), LimitCheckError> {
        if let Some(limit) = self.account_limit {
            if self.account_consumed + amount > limit {
                return Err(LimitCheckError::AccountLimitExceeded);
            }
        }
        Ok(())
    }

    fn can_raise_limit(
        new_limit: Option<f64>,
        already_consumed: f64,
    ) -> Result<(), LimitUpdateError> {
        match new_limit {
            None => Ok(()), // no limit is always allowed
            Some(lim) if lim >= already_consumed => Ok(()),
            Some(_) => Err(LimitUpdateError::BelowCurrentUsage),
        }
    }
}

impl Actor for AccountActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        println!("[Account {}] Started", self.account_id);
    }
}

impl Handler<AccountMsg> for AccountActor {
    type Result = ();

    fn handle(&mut self, msg: AccountMsg, _ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            AccountMsg::CheckAccountLimit { amount, request_id } => {
                // Called only when the card has already passed card-level checks.
                if let Err(err) = self.check_account_limit(amount) {
                    self.send_internal(RouterInternalMsg::LimitCheckResult {
                        request_id,
                        allowed: false,
                        error: Some(err),
                    });
                    return;
                }

                self.send_internal(RouterInternalMsg::LimitCheckResult {
                    request_id,
                    allowed: true,
                    error: None,
                });
            }

            AccountMsg::ApplyCharge {
                card_id,
                amount,
                request_id: _,
                timestamp,
                op_id,
            } => {
                // Card already updated its own card-level consumption.
                // Here we only manage account-wide consumption.
                self.account_consumed += amount;

                let op = Operation::Charge {
                    account_id: self.account_id,
                    card_id,
                    amount,
                    timestamp,
                    op_id,
                };

                self.send_internal(RouterInternalMsg::ChargeApplied { operation: op });
            }

            AccountMsg::UpdateAccountLimit {
                new_limit,
                request_id,
            } => {
                match Self::can_raise_limit(new_limit, self.account_consumed) {
                    Ok(()) => {
                        self.account_limit = new_limit;

                        self.send_internal(RouterInternalMsg::LimitUpdated {
                            scope: LimitScope::Account,
                            account_id: self.account_id,
                            card_id: None,
                            new_limit,
                        });
                    }
                    Err(err) => {
                        self.send_internal(RouterInternalMsg::LimitUpdateFailed {
                            scope: LimitScope::Account,
                            account_id: self.account_id,
                            card_id: None,
                            request_id,
                            error: err,
                        });
                    }
                }
            }
        }
    }
}

//! CardActor: per-card limit + per-card consumption.
//!
//! Goals:
//! - Short-circuit failed authorizations at card level so the account
//!   actor is not hammered by obviously invalid requests.
//! - Handle card-level limit updates.
//!
//! Flows:
//! - CheckLimit:
//!     1) If card limit exists and (consumed + amount) > limit,
//!        send LimitCheckResult(allowed=false, CardLimitExceeded) to Router.
//!        -> DOES NOT call AccountActor.
//!     2) If OK, call AccountActor.CheckAccountLimit(amount, request_id).
//!
//! - ApplyCharge:
//!     Increment local card_consumed (no limit check).
//!     Then AccountActor.ApplyCharge increments account_consumed and emits ChargeApplied.
//!
//! - UpdateLimit:
//!     Only allow new_limit >= consumed. Otherwise report failure.

use actix::prelude::*;

use super::account::AccountActor;
use super::actor_router::ActorRouter;
use super::types::{
    AccountMsg, CardMsg, LimitCheckError, LimitScope, LimitUpdateError, RouterInternalMsg,
};

pub struct CardActor {
    pub card_id: u64,
    pub account_id: u64,

    /// Per-card limit. None means "no limit".
    limit: Option<f64>,

    /// Total amount consumed by this card.
    consumed: f64,

    /// Back-reference to the router for reporting results.
    router: Addr<ActorRouter>,

    /// Back-reference to the account actor for account-limit checks.
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
            limit: None,
            consumed: 0.0,
            router,
            account,
        }
    }

    fn send_internal(&self, msg: RouterInternalMsg) {
        self.router.do_send(msg);
    }

    fn check_card_limit(&self, amount: f64) -> Result<(), LimitCheckError> {
        if let Some(limit) = self.limit {
            if self.consumed + amount > limit {
                return Err(LimitCheckError::CardLimitExceeded);
            }
        }
        Ok(())
    }

    fn can_raise_limit(
        new_limit: Option<f64>,
        already_consumed: f64,
    ) -> Result<(), LimitUpdateError> {
        match new_limit {
            None => Ok(()),
            Some(lim) if lim >= already_consumed => Ok(()),
            Some(_) => Err(LimitUpdateError::BelowCurrentUsage),
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

impl Handler<CardMsg> for CardActor {
    type Result = ();

    fn handle(&mut self, msg: CardMsg, _ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            CardMsg::CheckLimit {
                amount,
                request_id,
                account_id: _,
                card_id: _,
            } => {
                // 1) Card-level limit check
                if let Err(err) = self.check_card_limit(amount) {
                    // Short-circuit: do not hit the account.
                    self.send_internal(RouterInternalMsg::LimitCheckResult {
                        request_id,
                        allowed: false,
                        error: Some(err),
                    });
                    return;
                }

                // 2) If card is OK, ask the account for account-wide limit check.
                self.account
                    .do_send(AccountMsg::CheckAccountLimit { amount, request_id });
            }

            CardMsg::ApplyCharge { amount } => {
                // No limit validation here; it's just local usage tracking.
                self.consumed += amount;

                println!(
                    "[Card {}@Account {}] Applied charge: {} (total consumed: {})",
                    self.card_id, self.account_id, amount, self.consumed
                );
            }

            CardMsg::UpdateLimit {
                new_limit,
                request_id,
            } => {
                match Self::can_raise_limit(new_limit, self.consumed) {
                    Ok(()) => {
                        self.limit = new_limit;

                        self.send_internal(RouterInternalMsg::LimitUpdated {
                            scope: LimitScope::Card,
                            account_id: self.account_id,
                            card_id: Some(self.card_id),
                            new_limit,
                        });
                    }
                    Err(err) => {
                        self.send_internal(RouterInternalMsg::LimitUpdateFailed {
                            scope: LimitScope::Card,
                            account_id: self.account_id,
                            card_id: Some(self.card_id),
                            request_id,
                            error: err,
                        });
                    }
                }
            }

            CardMsg::Placeholder(text) => {
                println!(
                    "[Card {}@Account {}] Placeholder message: {}",
                    self.card_id, self.account_id, text
                );
            }
        }
    }
}

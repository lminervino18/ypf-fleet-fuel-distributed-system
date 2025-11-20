//! CardActor: manages per-card limit and per-card consumption.
//!
//! Now with reservations (pending_charges) to prevent "double spending"
//! between Verify and Apply when there is concurrency on the same card.

use actix::prelude::*;
use std::collections::HashMap;

use super::account::AccountActor;
use super::actor_router::ActorRouter;
use super::messages::{AccountMsg, CardMsg, RouterInternalMsg};
use crate::errors::{LimitCheckError, LimitUpdateError, VerifyError};

/// Actor responsible for a single card's state and limit enforcement.
pub struct CardActor {
    pub card_id: u64,
    pub account_id: u64,

    /// Per-card limit. None means "no limit".
    limit: Option<f64>,

    /// Total amount consumed by this card (already applied).
    consumed: f64,

    /// Holds charges that have been verified but not yet applied.
    /// Key = op_id, value = amount.
    pending_charges: HashMap<u64, f64>,

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
            limit: Some(50.0),
            consumed: 0.0,
            pending_charges: HashMap::new(),
            router,
            account,
        }
    }

    /// Send an internal message to the router.
    fn send_internal(&self, msg: RouterInternalMsg) {
        self.router.do_send(msg);
    }

    /// Effective consumption = applied + pending.
    fn effective_consumed(&self) -> f64 {
        let pending_sum: f64 = self.pending_charges.values().copied().sum();
        self.consumed + pending_sum
    }

    /// Check card-level limit for a given amount, considering pending reservations.
    fn check_card_limit_with_pending(&self, amount: f64) -> Result<(), LimitCheckError> {
        if let Some(limit) = self.limit {
            if self.effective_consumed() + amount > limit {
                return Err(LimitCheckError::CardLimitExceeded);
            }
        }
        Ok(())
    }

    /// Check if we can raise/set the card limit to `new_limit`.
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
        // Uncomment for debugging:
        // println!("[Card {}/Account {}] Started", self.card_id, self.account_id);
    }
}

impl Handler<CardMsg> for CardActor {
    type Result = ();

    fn handle(&mut self, msg: CardMsg, _ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            CardMsg::VerifyCharge {
                amount,
                op_id,
                account_id: _,
                card_id: _,
            } => {
                // 1) Card-level limit check with reservations
                let res = self
                    .check_card_limit_with_pending(amount)
                    .map_err(VerifyError::ChargeLimit);

                match res {
                    Err(err) => {
                        // Short-circuit: do not contact AccountActor if card fails.
                        self.send_internal(RouterInternalMsg::VerifyResult {
                            op_id,
                            allowed: false,
                            error: Some(err),
                        });
                    }
                    Ok(()) => {
                        // Reserve this amount at card level.
                        self.pending_charges.insert(op_id, amount);

                        // 2) If card is OK, request verification at account level.
                        self.account
                            .do_send(AccountMsg::VerifyCharge { amount, op_id });
                    }
                }
            }

            CardMsg::VerifyLimitChange { new_limit, op_id } => {
                let effective = self.effective_consumed();
                let res =
                    Self::can_raise_limit(new_limit, effective).map_err(VerifyError::LimitUpdate);

                match res {
                    Ok(()) => {
                        self.send_internal(RouterInternalMsg::VerifyResult {
                            op_id,
                            allowed: true,
                            error: None,
                        });
                    }
                    Err(err) => {
                        self.send_internal(RouterInternalMsg::VerifyResult {
                            op_id,
                            allowed: false,
                            error: Some(err),
                        });
                    }
                }
            }

            CardMsg::ApplyCharge { amount, op_id } => {
                // Take the reservation if it exists; if not, use amount.
                let reserved = self.pending_charges.remove(&op_id).unwrap_or(amount);
                self.consumed += reserved;

                // Uncomment for debugging:
                // println!(
                //     "[Card {}/Account {}] Applied charge: {} (total consumed: {})",
                //     self.card_id, self.account_id, reserved, self.consumed
                // );

                self.send_internal(RouterInternalMsg::ApplyResult {
                    op_id,
                    success: true,
                    error: None,
                });
            }

            CardMsg::ApplyLimitChange { new_limit, op_id } => {
                self.limit = new_limit;

                // Uncomment for debugging:
                // println!(
                //     "[Card {}/Account {}] New card limit set: {:?}",
                //     self.card_id, self.account_id, self.limit
                // );

                self.send_internal(RouterInternalMsg::ApplyResult {
                    op_id,
                    success: true,
                    error: None,
                });
            }

            CardMsg::Debug(text) => {
                self.send_internal(RouterInternalMsg::Debug(text));
            }
        }
    }
}

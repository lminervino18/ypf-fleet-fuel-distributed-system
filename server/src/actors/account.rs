//! AccountActor: manages one account and account-wide state.
//!
//! State:
//! - `account_limit`: optional (None = no limit).
//! - `account_consumed`: total charged across all cards of this account.
//! - `pending_charges`: holds amounts already verified but not yet applied.
//!
//! Flows for `Verify`:
//! - `VerifyCharge`:
//!     Called from CardActor, *only if* the card passed its own limit.
//!     The actor checks the account-wide limit for the requested amount,
//!     also considering pending reservations.
//!
//! - `VerifyLimitChange`:
//!     Checks that the new limit is not below the already consumed amount
//!     (including pending amounts).
//!
//! Flows for `Apply`:
//! - `ApplyCharge`:
//!     Takes the reservation associated with `op_id` (if any), updates
//!     `account_consumed` and reports success.
//!
//! - `ApplyLimitChange`:
//!     Sets the new account limit without additional checks (verification
//!     has already been done during `Verify`).

use actix::prelude::*;
use std::collections::HashMap;

use super::actor_router::ActorRouter;
use super::messages::{AccountMsg, RouterInternalMsg};
use crate::errors::{LimitCheckError, LimitUpdateError, VerifyError};

/// Actor that manages a single account and its state.
pub struct AccountActor {
    pub account_id: u64,

    /// Optional account-wide limit. `None` means "no limit".
    account_limit: Option<f64>,

    /// Total consumption across all cards in this account (already applied).
    account_consumed: f64,

    /// Holds charges that have been verified but not yet applied.
    /// Key = op_id, Value = amount.
    pending_charges: HashMap<u64, f64>,

    /// Back-reference to the router, to send RouterInternalMsg events.
    router: Addr<ActorRouter>,
}

impl AccountActor {
    pub fn new(account_id: u64, router: Addr<ActorRouter>) -> Self {
        Self {
            account_id,
            account_limit: Some(100.0),
            account_consumed: 0.0,
            pending_charges: HashMap::new(),
            router,
        }
    }

    /// Send an internal message to the router.
    fn send_internal(&self, msg: RouterInternalMsg) {
        self.router.do_send(msg);
    }

    /// Effective consumption = applied + pending.
    fn effective_consumed(&self) -> f64 {
        let pending_sum: f64 = self.pending_charges.values().copied().sum();
        self.account_consumed + pending_sum
    }

    /// Check account-wide limit for a given amount, considering pending reservations.
    fn check_account_limit_with_pending(&self, amount: f64) -> Result<(), LimitCheckError> {
        if let Some(limit) = self.account_limit {
            if self.effective_consumed() + amount > limit {
                return Err(LimitCheckError::AccountLimitExceeded);
            }
        }
        Ok(())
    }

    /// Check if we can raise/set the account limit to `new_limit`.
    /// Uses effective consumption (applied + pending).
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
        // Uncomment for debugging:
        // println!("[Account {}] Started", self.account_id);
    }
}

impl Handler<AccountMsg> for AccountActor {
    type Result = ();

    fn handle(&mut self, msg: AccountMsg, _ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            AccountMsg::VerifyCharge { amount, op_id } => {
                // Called only when the card has already passed card-level checks.
                let res = self
                    .check_account_limit_with_pending(amount)
                    .map_err(VerifyError::ChargeLimit);

                match res {
                    Ok(()) => {
                        // Reserve this amount for this op_id.
                        self.pending_charges.insert(op_id, amount);

                        self.send_internal(RouterInternalMsg::VerifyResult {
                            op_id,
                            allowed: true,
                            error: None,
                        });
                    }
                    Err(err) => {
                        // Clean up any previous phantom reservation.
                        self.pending_charges.remove(&op_id);

                        self.send_internal(RouterInternalMsg::VerifyResult {
                            op_id,
                            allowed: false,
                            error: Some(err),
                        });
                    }
                }
            }

            AccountMsg::VerifyLimitChange { new_limit, op_id } => {
                // Use effective consumption to avoid lowering below already
                // applied + pending.
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

            AccountMsg::ApplyCharge {
                card_id: _,
                amount,
                op_id,
            } => {
                // Take the reservation if it exists; if not (rare case) use `amount`.
                let reserved = self.pending_charges.remove(&op_id).unwrap_or(amount);

                self.account_consumed += reserved;

                // Uncomment for debugging:
                // println!(
                //     "[Account {}] Applied charge: {} (total consumed: {})",
                //     self.account_id, reserved, self.account_consumed
                // );

                self.send_internal(RouterInternalMsg::ApplyResult {
                    op_id,
                    success: true,
                    error: None,
                });
            }

            AccountMsg::ApplyLimitChange { new_limit, op_id } => {
                // Apply the new limit without re-checking invariants.
                self.account_limit = new_limit;

                // Uncomment for debugging:
                // println!(
                //     "[Account {}] New account limit set: {:?}",
                //     self.account_id, self.account_limit
                // );

                self.send_internal(RouterInternalMsg::ApplyResult {
                    op_id,
                    success: true,
                    error: None,
                });
            }
        }
    }
}

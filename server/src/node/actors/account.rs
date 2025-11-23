//! AccountActor: manages one account and account-wide state.
//!
//! State:
//! - `account_limit`: optional (None = no limit).
//! - `account_consumed`: total charged across all cards of this account.
//!
//! Flows:
//! - Charges (from cards):
//!   CardActor → AccountActor:
//!   `AccountMsg::ApplyChargeFromCard`
//!   The account:
//!     * if `from_offline_station == false`:
//!         - checks account-wide limit,
//!         - applies the charge if allowed,
//!     * if `from_offline_station == true`:
//!         - **skips all limit checks** and unconditionally applies
//!           the charge (these correspond to previously-confirmed
//!           offline operations that must be reconciled),
//!     * replies to the card via `AccountChargeReply`.
//!
//! - Account limit changes (from router):
//!   Router → AccountActor:
//!   `AccountMsg::ApplyAccountLimit`
//!   The account:
//!     * checks that the new limit is not below already consumed,
//!     * applies the new limit if allowed,
//!     * notifies the router via `RouterInternalMsg::OperationCompleted`.

use actix::prelude::*;

use super::actor_router::ActorRouter;
use super::messages::{AccountChargeReply, AccountMsg, RouterInternalMsg};
use crate::errors::{LimitCheckError, LimitUpdateError, VerifyError};

/// Actor that manages a single account and its state.
pub struct AccountActor {
    pub account_id: u64,

    /// Optional account-wide limit. `None` means "no limit".
    account_limit: Option<f32>,

    /// Total consumption across all cards in this account (already applied).
    account_consumed: f32,

    /// Back-reference to the router, to send `RouterInternalMsg` events.
    router: Addr<ActorRouter>,
}

impl AccountActor {
    pub fn new(account_id: u64, router: Addr<ActorRouter>) -> Self {
        Self {
            account_id,
            account_limit: Some(100.0),
            account_consumed: 0.0,
            router,
        }
    }

    /// Send an internal message to the router.
    fn send_internal(&self, msg: RouterInternalMsg) {
        self.router.do_send(msg);
    }

    /// Helper to check account-wide limit for a given amount.
    fn check_account_limit(&self, amount: f32) -> Result<(), LimitCheckError> {
        if let Some(limit) = self.account_limit {
            if self.account_consumed + amount > limit {
                return Err(LimitCheckError::AccountLimitExceeded);
            }
        }
        Ok(())
    }

    /// Helper to validate a new account limit against current consumption.
    fn can_raise_limit(
        new_limit: Option<f32>,
        already_consumed: f32,
    ) -> Result<(), LimitUpdateError> {
        match new_limit {
            None => Ok(()), // "no limit" is always allowed
            Some(lim) if lim >= already_consumed => Ok(()),
            Some(_) => Err(LimitUpdateError::BelowCurrentUsage),
        }
    }
}

impl Actor for AccountActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        // println!("[Account {}] Started", self.account_id);
    }
}

impl Handler<AccountMsg> for AccountActor {
    type Result = ();

    fn handle(&mut self, msg: AccountMsg, _ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            AccountMsg::ApplyChargeFromCard {
                op_id,
                amount,
                card_id: _,
                from_offline_station,
                reply_to,
            } => {
                if from_offline_station {
                    // OFFLINE REPLAY:
                    // ---------------
                    // This charge was already confirmed to the station while
                    // the node was OFFLINE. We must:
                    // - skip *all* limit checks,
                    // - unconditionally apply it at account level,
                    // - always report success.
                    self.account_consumed += amount;

                    reply_to.do_send(AccountChargeReply {
                        op_id,
                        success: true,
                        error: None,
                    });

                    return;
                }

                // ONLINE CHARGE:
                // -------------
                // Verify account-wide limit for this charge.
                let res = self
                    .check_account_limit(amount)
                    .map_err(VerifyError::ChargeLimit);

                match res {
                    Ok(()) => {
                        // Apply charge at account level.
                        self.account_consumed += amount;

                        reply_to.do_send(AccountChargeReply {
                            op_id,
                            success: true,
                            error: None,
                        });
                    }
                    Err(err) => {
                        // Do not update account_consumed; just notify the card.
                        reply_to.do_send(AccountChargeReply {
                            op_id,
                            success: false,
                            error: Some(err),
                        });
                    }
                }
            }

            AccountMsg::ApplyAccountLimit { op_id, new_limit } => {
                let res = Self::can_raise_limit(new_limit, self.account_consumed)
                    .map_err(VerifyError::LimitUpdate);

                match res {
                    Ok(()) => {
                        // Apply the new account limit.
                        self.account_limit = new_limit;

                        self.send_internal(RouterInternalMsg::OperationCompleted {
                            op_id,
                            success: true,
                            error: None,
                        });
                    }
                    Err(err) => {
                        self.send_internal(RouterInternalMsg::OperationCompleted {
                            op_id,
                            success: false,
                            error: Some(err),
                        });
                    }
                }
            }

            AccountMsg::ExecuteQueryAccount { op_id } => {
                let msg = format!(
                    "[Account {}] Query: limit={:?}, consumed={}",
                    self.account_id, self.account_limit, self.account_consumed
                );
                println!("{msg}");

                self.send_internal(RouterInternalMsg::OperationCompleted {
                    op_id,
                    success: true,
                    error: None,
                });
            }

            AccountMsg::ExecuteQueryCards { op_id } => {
                let msg = format!(
                    "[Account {}] QueryCards",
                    self.account_id
                );
                println!("{msg}");

                self.send_internal(RouterInternalMsg::OperationCompleted {
                    op_id,
                    success: true,
                    error: None,
                });
            }

            AccountMsg::ExecuteBilling { op_id, period } => {
                let msg = match period {
                    Some(p) => format!(
                        "[Account {}] Billing for period {}",
                        self.account_id, p
                    ),
                    None => format!(
                        "[Account {}] Billing for all periods",
                        self.account_id
                    ),
                };
                println!("{msg}");

                self.send_internal(RouterInternalMsg::OperationCompleted {
                    op_id,
                    success: true,
                    error: None,
                });
            }
        }
    }
}

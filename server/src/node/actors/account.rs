//! AccountActor: manages one account and account-wide state.
use actix::prelude::*;
use std::collections::HashMap;

use super::actor_router::ActorRouter;
use super::messages::{AccountChargeReply, AccountMsg, RouterInternalMsg};
use crate::errors::{LimitCheckError, LimitUpdateError, VerifyError};

/// Estado interno de un query de cuenta en curso.
#[derive(Debug)]
struct AccountPendingQuery {
    op_id: u32,
    remaining: usize,
    per_card_spent: HashMap<u64, f32>,
}

/// Actor that manages a single account and its state.
pub struct AccountActor {
    pub account_id: u64,

    /// Optional account-wide limit. `None` means "no limit".
    account_limit: Option<f32>,

    /// Total consumption across all cards in this account (already applied).
    account_consumed: f32,

    /// Back-reference to the router, to send `RouterInternalMsg` events.
    router: Addr<ActorRouter>,

    /// Query de cuenta en curso (si lo hay).
    pending_query: Option<AccountPendingQuery>,
}

impl AccountActor {
    pub fn new(account_id: u64, router: Addr<ActorRouter>) -> Self {
        Self {
            account_id,
            account_limit: Some(100.0),
            account_consumed: 0.0,
            router,
            pending_query: None,
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
                    // OFFLINE REPLAY: saltar chequeos y aplicar siempre
                    self.account_consumed += amount;

                    reply_to.do_send(AccountChargeReply {
                        op_id,
                        success: true,
                        error: None,
                    });

                    return;
                }

                // ONLINE CHARGE: verificar límite de cuenta
                let res = self
                    .check_account_limit(amount)
                    .map_err(VerifyError::ChargeLimit);

                match res {
                    Ok(()) => {
                        self.account_consumed += amount;

                        reply_to.do_send(AccountChargeReply {
                            op_id,
                            success: true,
                            error: None,
                        });
                    }
                    Err(err) => {
                        let _ = reply_to.do_send(AccountChargeReply {
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

            AccountMsg::StartAccountQuery { op_id, num_cards } => {
                // Si no hay tarjetas, respondemos directamente con lo que sabemos.
                if num_cards == 0 {
                    self.send_internal(RouterInternalMsg::AccountQueryCompleted {
                        op_id,
                        account_id: self.account_id,
                        total_spent: self.account_consumed,
                        per_card_spent: HashMap::new(),
                    });
                    return;
                }

                // Iniciar estado interno del query
                self.pending_query = Some(AccountPendingQuery {
                    op_id,
                    remaining: num_cards,
                    per_card_spent: HashMap::new(),
                });
            }

            AccountMsg::CardQueryReply {
                op_id,
                card_id,
                consumed,
            } => {
                // Solo nos importa si hay un query en curso con ese op_id
                let pending = match self.pending_query.as_mut() {
                    Some(p) if p.op_id == op_id => p,
                    _ => {
                        // Query inesperado; ignorar o loggear
                        self.send_internal(RouterInternalMsg::Debug(format!(
                            "[Account {}] CardQueryReply inesperado: op_id={}",
                            self.account_id, op_id
                        )));
                        return;
                    }
                };

                pending.per_card_spent.insert(card_id, consumed);
                if pending.remaining > 0 {
                    pending.remaining -= 1;
                }

                // ¿ya respondieron todas las tarjetas?
                if pending.remaining == 0 {
                    let per_card_spent = std::mem::take(&mut pending.per_card_spent);
                    let total_spent = per_card_spent.values().copied().sum::<f32>();

                    let op_id = pending.op_id;
                    let account_id = self.account_id;

                    // limpiar estado
                    self.pending_query = None;

                    self.send_internal(RouterInternalMsg::AccountQueryCompleted {
                        op_id,
                        account_id,
                        total_spent,
                        per_card_spent,
                    });
                }
            }
        }
    }
}

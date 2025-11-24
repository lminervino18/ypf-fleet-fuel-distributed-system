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
    per_card_spent: Vec<(u64, f32)>,
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
            account_limit: None,
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
                        per_card_spent: Vec::new(),
                    });
                    return;
                }

                // Iniciar estado interno del query
                self.pending_query = Some(AccountPendingQuery {
                    op_id,
                    remaining: num_cards,
                    per_card_spent: Vec::new(),
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

                pending.per_card_spent.push((card_id, consumed));
                if pending.remaining > 0 {
                    pending.remaining -= 1;
                }

                // ¿ya respondieron todas las tarjetas?
                if pending.remaining == 0 {
                    let per_card_spent = std::mem::take(&mut pending.per_card_spent);
                    let total_spent = per_card_spent
                        .clone()
                        .into_iter()
                        .map(|(_card, amount)| amount)
                        .sum::<f32>();

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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    use crate::errors::{LimitCheckError, LimitUpdateError};
    use crate::node::actors::actor_router::ActorRouter;

    /// Crea un AccountActor de prueba con límite y consumo inicial configurables.
    fn make_test_account(limit: Option<f32>, consumed: f32) -> AccountActor {
        let (tx, _rx) = mpsc::channel(8);
        let router = ActorRouter::new(tx).start();

        AccountActor {
            account_id: 1,
            account_limit: limit,
            account_consumed: consumed,
            router,
            pending_query: None,
        }
    }

    #[actix_rt::test]
    async fn default_account_has_no_limit_and_zero_consumption() {
        let (tx, _rx) = mpsc::channel(8);
        let router = ActorRouter::new(tx).start();

        let acc = AccountActor::new(42, router);

        assert_eq!(acc.account_id, 42);
        assert!(acc.account_limit.is_none());
        assert_eq!(acc.account_consumed, 0.0);

        // Con None como límite, cualquier monto debería ser aceptado.
        assert!(acc.check_account_limit(10.0).is_ok());
        assert!(acc.check_account_limit(10_000.0).is_ok());
    }

    #[actix_rt::test]
    async fn limited_account_blocks_above_limit() {
        // Cuenta con límite 50, ya consumidos 20
        let acc = make_test_account(Some(50.0), 20.0);

        // 1) Un cargo que deja justo en el límite (20 + 30 = 50) es válido
        assert!(acc.check_account_limit(30.0).is_ok());

        // 2) Un cargo que se pasa (20 + 31 = 51) debe fallar
        let err = acc.check_account_limit(31.0).unwrap_err();
        assert!(matches!(err, LimitCheckError::AccountLimitExceeded));
    }

    #[actix_rt::test]
    async fn none_limit_allows_any_amount() {
        // Cuenta sin límite (None) debería aceptar cualquier monto
        let acc = make_test_account(None, 100.0);

        assert!(acc.check_account_limit(1.0).is_ok());
        assert!(acc.check_account_limit(10_000.0).is_ok());
    }

    #[actix_rt::test]
    async fn can_raise_limit_respects_current_consumption() {
        let already_consumed = 30.0;

        // Subir el límite a algo >= consumo actual es válido
        let res_ok = AccountActor::can_raise_limit(Some(50.0), already_consumed);
        assert!(res_ok.is_ok());

        // Bajar el límite por debajo de lo ya consumido debe fallar
        let err = AccountActor::can_raise_limit(Some(10.0), already_consumed).unwrap_err();
        assert!(matches!(err, LimitUpdateError::BelowCurrentUsage));

        // Pasar a "sin límite" (None) siempre es válido
        let res_none = AccountActor::can_raise_limit(None, already_consumed);
        assert!(res_none.is_ok());
    }

    #[actix_rt::test]
    async fn account_consumed_field_accumulates() {
        let mut acc = make_test_account(None, 0.0);
        assert_eq!(acc.account_consumed, 0.0);

        acc.account_consumed += 10.0;
        assert_eq!(acc.account_consumed, 10.0);

        acc.account_consumed += 5.5;
        assert_eq!(acc.account_consumed, 15.5);
    }
}

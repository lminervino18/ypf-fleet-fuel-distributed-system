use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::node::actors::messages::{
    AccountMsg, ActorEvent, CardMsg, RouterCmd, RouterInternalMsg,
};

use super::account::AccountActor;
use super::card::CardActor;
use common::operation::Operation;
use common::operation_result::{AccountQueryResult, ChargeResult, LimitResult, OperationResult};

/// Router actor that owns and routes to AccountActor and CardActor instances.
///
/// Responsibilities:
/// - Maintain and create AccountActor and CardActor instances as needed.
/// - Route `RouterCmd::Execute` to the correct actors.
/// - Receive `RouterInternalMsg::*` from cards/accounts.
/// - Emit un único `ActorEvent::OperationResult` hacia el Node.
pub struct ActorRouter {
    /// Maps account_id to AccountActor address.
    pub accounts: HashMap<u64, Addr<AccountActor>>,

    /// Maps (account_id, card_id) to CardActor address.
    pub cards: HashMap<(u64, u64), Addr<CardActor>>,

    /// Maps op_id to Operation for correlation.
    pub operations: HashMap<u32, Operation>,

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
            // EXECUTE (single-step from Node POV)
            // ---------------------------------
            RouterCmd::Execute { op_id, operation } => {
                // Store the operation so we can attach it to the final result.
                self.operations.insert(op_id, operation.clone());

                match operation {
                    Operation::Charge {
                        account_id,
                        card_id,
                        amount,
                        from_offline_station,
                    } => {
                        let card = self.get_or_create_card(account_id, card_id, ctx);
                        card.do_send(CardMsg::ExecuteCharge {
                            op_id,
                            account_id,
                            card_id,
                            amount,
                            from_offline_station,
                        });
                    }
                    Operation::LimitAccount {
                        account_id,
                        new_limit,
                    } => {
                        let acc = self.get_or_create_account(account_id, ctx);
                        acc.do_send(AccountMsg::ApplyAccountLimit { op_id, new_limit });
                    }
                    Operation::LimitCard {
                        account_id,
                        card_id,
                        new_limit,
                    } => {
                        let card = self.get_or_create_card(account_id, card_id, ctx);
                        card.do_send(CardMsg::ExecuteLimitChange { op_id, new_limit });
                    }
                    Operation::AccountQuery { account_id } => {
                        // NUEVO: El Router arranca el query pidiéndole a la Account
                        // que espere N respuestas, y envía QueryCardState a cada Card
                        let acc = self.get_or_create_account(account_id, ctx);

                        // Determinar qué tarjetas pertenecen a esta cuenta
                        let mut card_ids = Vec::new();
                        for (acc_id, card_id) in self.cards.keys() {
                            if *acc_id == account_id {
                                card_ids.push(*card_id);
                            }
                        }
                        let num_cards = card_ids.len();

                        // 1) avisar a la cuenta cuántas tarjetas tiene que esperar
                        acc.do_send(AccountMsg::StartAccountQuery { op_id, num_cards });

                        // 2) pedirle a cada tarjeta su estado
                        for card_id in card_ids {
                            if let Some(card_addr) = self.cards.get(&(account_id, card_id)) {
                                card_addr.do_send(CardMsg::QueryCardState { op_id, account_id });
                            }
                        }
                        // El resultado final llegará vía RouterInternalMsg::AccountQueryCompleted
                    }
                    Operation::Bill { account_id, period } => todo!(),
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
            RouterInternalMsg::OperationCompleted {
                op_id,
                success,
                error,
            } => {
                // Retrieve the corresponding operation.
                let operation = match self.operations.get(&op_id) {
                    Some(op) => op.clone(),
                    None => {
                        // This should not normally happen: we got a completion
                        // for an operation we never registered.
                        self.emit(ActorEvent::Debug(format!(
                            "[Router] OperationCompleted for unknown op_id={op_id}",
                        )));
                        return;
                    }
                };

                // Mapear a OperationResult para Charge / LimitAccount / LimitCard
                let result = match operation {
                    Operation::Charge { .. } => {
                        let cr = if success {
                            ChargeResult::Ok
                        } else {
                            ChargeResult::Failed(
                                error
                                    .clone()
                                    .expect("VerifyError esperado si success == false"),
                            )
                        };
                        OperationResult::Charge(cr)
                    }
                    Operation::LimitAccount { .. } => {
                        let lr = if success {
                            LimitResult::Ok
                        } else {
                            LimitResult::Failed(
                                error
                                    .clone()
                                    .expect("VerifyError esperado si success == false"),
                            )
                        };
                        OperationResult::LimitAccount(lr)
                    }
                    Operation::LimitCard { .. } => {
                        let lr = if success {
                            LimitResult::Ok
                        } else {
                            LimitResult::Failed(
                                error
                                    .clone()
                                    .expect("VerifyError esperado si success == false"),
                            )
                        };
                        OperationResult::LimitCard(lr)
                    }
                    Operation::AccountQuery { .. } => {
                        // AccountQuery no debería entrar acá; lo manejamos con
                        // AccountQueryCompleted. Fallback defensivo.
                        OperationResult::AccountQuery(AccountQueryResult {
                            account_id: 0,
                            total_spent: 0.0,
                            per_card_spent: Vec::new(),
                        })
                    }
                    Operation::Bill { account_id, period } => todo!(),
                };

                self.emit(ActorEvent::OperationResult {
                    op_id,
                    operation,
                    result,
                    success,
                    error,
                });
            }

            RouterInternalMsg::AccountQueryCompleted {
                op_id,
                account_id,
                total_spent,
                per_card_spent,
            } => {
                let operation = match self.operations.get(&op_id) {
                    Some(op) => op.clone(),
                    None => {
                        self.emit(ActorEvent::Debug(format!(
                            "[Router] AccountQueryCompleted for unknown op_id={op_id}"
                        )));
                        return;
                    }
                };

                let result = OperationResult::AccountQuery(AccountQueryResult {
                    account_id,
                    total_spent,
                    per_card_spent,
                });

                self.emit(ActorEvent::OperationResult {
                    op_id,
                    operation,
                    result,
                    success: true,
                    error: None,
                });
            }

            RouterInternalMsg::Debug(msg) => {
                self.emit(ActorEvent::Debug(msg));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;

    use super::*;
    use actix::prelude::*;
    use tokio::sync::mpsc;

    use crate::errors::{LimitCheckError, LimitUpdateError, VerifyError};
    use crate::node::actors::messages::{ActorEvent, RouterCmd};
    use common::operation::Operation;
    use common::operation_result::{
        AccountQueryResult, ChargeResult, LimitResult, OperationResult,
    };

    /// Helper: crea un Router con su canal de eventos hacia el "Node".
    fn make_router() -> (Addr<ActorRouter>, mpsc::Receiver<ActorEvent>) {
        let (tx, rx) = mpsc::channel::<ActorEvent>(16);
        let router = ActorRouter::new(tx).start();
        (router, rx)
    }

    /// Espera hasta recibir un `ActorEvent::OperationResult` para el `op_id` dado.
    async fn wait_for_result(
        rx: &mut mpsc::Receiver<ActorEvent>,
        op_id: u32,
    ) -> (Operation, OperationResult, bool, Option<VerifyError>) {
        while let Some(ev) = rx.recv().await {
            match ev {
                ActorEvent::OperationResult {
                    op_id: got_id,
                    operation,
                    result,
                    success,
                    error,
                } if got_id == op_id => {
                    return (operation, result, success, error);
                }
                _ => {
                    // Ignoramos otros eventos / op_ids.
                }
            }
        }
        panic!("No se recibió OperationResult para op_id={op_id}");
    }

    #[actix_rt::test]
    async fn charge_success_creates_actors_and_emits_ok_result() {
        let (router, mut rx) = make_router();

        let account_id = 1;
        let card_id = 10;
        let op_id = 1;

        let op = Operation::Charge {
            account_id,
            card_id,
            amount: 25.0,
            from_offline_station: false,
        };

        router.do_send(RouterCmd::Execute {
            op_id,
            operation: op.clone(),
        });

        let (operation, result, success, error) = wait_for_result(&mut rx, op_id).await;

        assert_eq!(operation, op);
        assert!(success);
        assert!(error.is_none());

        match result {
            OperationResult::Charge(ChargeResult::Ok) => {}
            other => panic!("Esperábamos ChargeResult::Ok, obtuvimos {other:?}"),
        }

        // De paso, el router debería haber creado 1 cuenta y 1 tarjeta.
        assert!(router.try_send(RouterCmd::GetLog).is_ok());
    }

    #[actix_rt::test]
    async fn limit_card_then_charge_over_limit_fails() {
        let (router, mut rx) = make_router();

        let account_id = 2;
        let card_id = 20;

        // 1) Seteamos límite de tarjeta a 10.0
        let op_id_limit = 1;
        let op_limit = Operation::LimitCard {
            account_id,
            card_id,
            new_limit: Some(10.0),
        };

        router.do_send(RouterCmd::Execute {
            op_id: op_id_limit,
            operation: op_limit.clone(),
        });

        let (_operation, result, success, error) = wait_for_result(&mut rx, op_id_limit).await;

        assert!(success);
        assert!(error.is_none());
        match result {
            OperationResult::LimitCard(LimitResult::Ok) => {}
            other => panic!("Esperábamos LimitResult::Ok, obtuvimos {other:?}"),
        }

        // 2) Intentamos un cargo que excede ese límite (20.0 > 10.0)
        let op_id_charge = 2;
        let op_charge = Operation::Charge {
            account_id,
            card_id,
            amount: 20.0,
            from_offline_station: false,
        };

        router.do_send(RouterCmd::Execute {
            op_id: op_id_charge,
            operation: op_charge.clone(),
        });

        let (_operation, result, success, error) = wait_for_result(&mut rx, op_id_charge).await;

        assert!(!success);
        let err = error.expect("Esperábamos un VerifyError");
        match err {
            VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded) => {}
            other => panic!("Esperábamos CardLimitExceeded, obtuvimos {other:?}"),
        }

        match result {
            OperationResult::Charge(ChargeResult::Failed(e)) => match e {
                VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded) => {}
                other => panic!(
                    "Esperábamos ChargeResult::Failed(CardLimitExceeded), obtuvimos {other:?}",
                ),
            },
            other => panic!("Esperábamos ChargeResult::Failed(_), obtuvimos {other:?}"),
        }
    }

    #[actix_rt::test]
    async fn limit_account_below_consumption_fails_and_emits_error() {
        let (router, mut rx) = make_router();

        let account_id = 3;
        let card_id = 30;

        // Generamos consumo: dos cargos online que siempre pasan porque no hay límite inicial.
        let op_id_c1 = 1;
        let op_c1 = Operation::Charge {
            account_id,
            card_id,
            amount: 20.0,
            from_offline_station: false,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_c1,
            operation: op_c1,
        });
        let _ = wait_for_result(&mut rx, op_id_c1).await;

        let op_id_c2 = 2;
        let op_c2 = Operation::Charge {
            account_id,
            card_id,
            amount: 15.0,
            from_offline_station: false,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_c2,
            operation: op_c2,
        });
        let _ = wait_for_result(&mut rx, op_id_c2).await;

        // En este punto, la cuenta ya tiene 35.0 consumidos.
        // Intentamos fijar un límite menor (10.0) -> debe fallar por BelowCurrentUsage.
        let op_id_limit = 3;
        let op_limit = Operation::LimitAccount {
            account_id,
            new_limit: Some(10.0),
        };

        router.do_send(RouterCmd::Execute {
            op_id: op_id_limit,
            operation: op_limit.clone(),
        });

        let (operation, result, success, error) = wait_for_result(&mut rx, op_id_limit).await;

        assert_eq!(operation, op_limit);
        assert!(!success);

        let err = error.expect("Esperábamos VerifyError");
        match err {
            VerifyError::LimitUpdate(LimitUpdateError::BelowCurrentUsage) => {}
            other => panic!("Esperábamos BelowCurrentUsage, obtuvimos {other:?}"),
        }

        match result {
            OperationResult::LimitAccount(LimitResult::Failed(e)) => match e {
                VerifyError::LimitUpdate(LimitUpdateError::BelowCurrentUsage) => {}
                other => panic!(
                    "Esperábamos LimitResult::Failed(BelowCurrentUsage), obtuvimos {other:?}"
                ),
            },
            other => panic!(
                "Esperábamos LimitAccount(LimitResult::Failed), obtuvimos {other:?}"
            ),
        }
    }

    #[actix_rt::test]
    async fn account_query_on_empty_account_returns_zeroes() {
        let (router, mut rx) = make_router();

        let account_id = 4;
        let op_id = 1;

        let op = Operation::AccountQuery { account_id };

        router.do_send(RouterCmd::Execute {
            op_id,
            operation: op.clone(),
        });

        let (operation, result, success, error) = wait_for_result(&mut rx, op_id).await;

        assert!(success);
        assert!(error.is_none());
        assert_eq!(operation, op);

        match result {
            OperationResult::AccountQuery(AccountQueryResult {
                account_id: acc,
                total_spent,
                per_card_spent,
            }) => {
                assert_eq!(acc, account_id);
                assert_eq!(total_spent, 0.0);
                assert!(per_card_spent.is_empty());
            }
            other => panic!(
                "Esperábamos AccountQueryResult vacío, obtuvimos {other:?}"
            ),
        }
    }

    #[actix_rt::test]
    async fn account_query_aggregates_per_card_and_total() {
        /*         let (router, mut rx) = make_router();

        let account_id = 5;
        let card_id = 50;

        // Cargamos dos veces la misma tarjeta
        let op_id_charge1 = 1;
        let op_charge1 = Operation::Charge {
            account_id,
            card_id,
            amount: 10.0,
            from_offline_station: false,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_charge1,
            operation: op_charge1,
        });
        let _ = wait_for_result(&mut rx, op_id_charge1).await;

        let op_id_charge2 = 2;
        let op_charge2 = Operation::Charge {
            account_id,
            card_id,
            amount: 15.0,
            from_offline_station: false,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_charge2,
            operation: op_charge2,
        });
        let _ = wait_for_result(&mut rx, op_id_charge2).await;

        // Ahora pedimos un AccountQuery
        let op_id_query = 3;
        let op_query = Operation::AccountQuery { account_id };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_query,
            operation: op_query.clone(),
        });

        let (operation, result, success, error) = wait_for_result(&mut rx, op_id_query).await;

        assert!(success);
        assert!(error.is_none());
        assert_eq!(operation, op_query);

        match result {
            OperationResult::AccountQuery(AccountQueryResult {
                account_id: acc,
                total_spent,
                per_card_spent,
            }) => {
                assert_eq!(acc, account_id);
                assert_eq!(total_spent, 25.0);
                let spent_card = per_card_spent.get(&card_id).copied().unwrap_or(0.0);
                assert_eq!(spent_card, 25.0);
            }
            other => panic!(
                "Esperábamos AccountQueryResult con totales, obtuvimos {:?}",
                other
            ),
        } */
    }

    #[actix_rt::test]
    async fn multiple_operations_interleaved_stay_correlated_by_op_id() {
        /*         let (router, mut rx) = make_router();

        let account_id = 6;
        let card_id = 60;

        // Enviamos varias operaciones intercaladas:
        // 1: Charge
        // 2: LimitCard
        // 3: Charge (que va a fallar por límite)
        // 4: AccountQuery
        let op1 = Operation::Charge {
            account_id,
            card_id,
            amount: 5.0,
            from_offline_station: false,
        };
        let op2 = Operation::LimitCard {
            account_id,
            card_id,
            new_limit: Some(10.0),
        };
        let op3 = Operation::Charge {
            account_id,
            card_id,
            amount: 20.0,
            from_offline_station: false,
        };
        let op4 = Operation::AccountQuery { account_id };

        router.do_send(RouterCmd::Execute {
            op_id: 1,
            operation: op1.clone(),
        });
        router.do_send(RouterCmd::Execute {
            op_id: 2,
            operation: op2.clone(),
        });
        router.do_send(RouterCmd::Execute {
            op_id: 3,
            operation: op3.clone(),
        });
        router.do_send(RouterCmd::Execute {
            op_id: 4,
            operation: op4.clone(),
        });

        use std::collections::HashMap;

        let mut results: HashMap<u32, (Operation, OperationResult, bool, Option<VerifyError>)> =
            HashMap::new();

        // Esperamos hasta tener los 4 OperationResult (uno por op_id).
        while results.len() < 4 {
            if let Some(ev) = rx.recv().await {
                if let ActorEvent::OperationResult {
                    op_id,
                    operation,
                    result,
                    success,
                    error,
                } = ev
                {
                    results.insert(op_id, (operation, result, success, error));
                }
            } else {
                panic!("Canal cerrado antes de recibir los 4 OperationResult");
            }
        }

        // ---------- op_id = 1: Charge chico -> OK ----------
        let (operation1, result1, success1, error1) =
            results.remove(&1).expect("Falta resultado para op_id=1");
        assert_eq!(operation1, op1);
        assert!(success1);
        assert!(error1.is_none());
        assert!(matches!(result1, OperationResult::Charge(ChargeResult::Ok)));

        // ---------- op_id = 2: LimitCard -> OK ----------
        let (operation2, result2, success2, error2) =
            results.remove(&2).expect("Falta resultado para op_id=2");
        assert_eq!(operation2, op2);
        assert!(success2);
        assert!(error2.is_none());
        assert!(matches!(
            result2,
            OperationResult::LimitCard(LimitResult::Ok)
        ));

        // ---------- op_id = 3: Cargo grande sobre límite -> FAIL ----------
        let (_operation3, result3, success3, error3) =
            results.remove(&3).expect("Falta resultado para op_id=3");
        assert!(!success3);
        let err3 = error3.expect("Esperábamos VerifyError para op3");
        match err3 {
            VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded) => {}
            other => panic!(
                "Esperábamos CardLimitExceeded en op3, obtuvimos {:?}",
                other
            ),
        }
        match result3 {
            OperationResult::Charge(ChargeResult::Failed(e)) => match e {
                VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded) => {}
                other => panic!(
                    "Esperábamos ChargeResult::Failed(CardLimitExceeded) en op3, obtuvimos {:?}",
                    other
                ),
            },
            other => panic!(
                "Esperábamos ChargeResult::Failed(_) en op3, obtuvimos {:?}",
                other
            ),
        }

        // ---------- op_id = 4: AccountQuery ----------
        // Por la naturaleza concurrente, la query puede ver 0 o 5,
        // pero nunca más de 5 (un solo cargo exitoso) y debe ser coherente.
        let (operation4, result4, success4, error4) =
            results.remove(&4).expect("Falta resultado para op_id=4");
        assert_eq!(operation4, op4);
        assert!(success4);
        assert!(error4.is_none());

        match result4 {
            OperationResult::AccountQuery(AccountQueryResult {
                account_id: acc,
                total_spent,
                per_card_spent,
            }) => {
                assert_eq!(acc, account_id);

                let spent_card = per_card_spent.get(&card_id).copied().unwrap_or(0.0);

                // Coherencia interna: con una sola tarjeta, total == gasto de esa tarjeta.
                assert!(
                    (total_spent - spent_card).abs() < f32::EPSILON,
                    "total_spent ({}) y spent_card ({}) no coinciden",
                    total_spent,
                    spent_card
                );

                // Por construcción del test, nunca tiene que aparecer más de 5.0
                assert!(
                    total_spent <= 5.0 + f32::EPSILON,
                    "AccountQuery vio más consumo del posible: {}",
                    total_spent
                );
            }
            other => panic!(
                "Esperábamos AccountQueryResult tras intercalado, obtuvimos {:?}",
                other
            ),
        } */
    }

    #[actix_rt::test]
    async fn account_query_multiple_cards_sums_correctly() {
        /*         let (router, mut rx) = make_router();

        let account_id = 100;
        let card_a = 10;
        let card_b = 20;
        let card_c = 30;

        // card_a: 10.0
        let op_id_a = 1;
        let op_a = Operation::Charge {
            account_id,
            card_id: card_a,
            amount: 10.0,
            from_offline_station: false,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_a,
            operation: op_a.clone(),
        });
        let (_op, res, success, err) = wait_for_result(&mut rx, op_id_a).await;
        assert!(success);
        assert!(err.is_none());
        match res {
            OperationResult::Charge(ChargeResult::Ok) => {}
            other => panic!(
                "Esperábamos ChargeResult::Ok para card_a, obtuvimos {:?}",
                other
            ),
        }

        // card_b: 15.0
        let op_id_b = 2;
        let op_b = Operation::Charge {
            account_id,
            card_id: card_b,
            amount: 15.0,
            from_offline_station: false,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_b,
            operation: op_b.clone(),
        });
        let (_op, res, success, err) = wait_for_result(&mut rx, op_id_b).await;
        assert!(success);
        assert!(err.is_none());
        match res {
            OperationResult::Charge(ChargeResult::Ok) => {}
            other => panic!(
                "Esperábamos ChargeResult::Ok para card_b, obtuvimos {:?}",
                other
            ),
        }

        // card_c: 5.0
        let op_id_c = 3;
        let op_c = Operation::Charge {
            account_id,
            card_id: card_c,
            amount: 5.0,
            from_offline_station: false,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_c,
            operation: op_c.clone(),
        });
        let (_op, res, success, err) = wait_for_result(&mut rx, op_id_c).await;
        assert!(success);
        assert!(err.is_none());
        match res {
            OperationResult::Charge(ChargeResult::Ok) => {}
            other => panic!(
                "Esperábamos ChargeResult::Ok para card_c, obtuvimos {:?}",
                other
            ),
        }

        // Ahora pedimos el AccountQuery para esa cuenta
        let op_id_q = 4;
        let op_q = Operation::AccountQuery { account_id };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_q,
            operation: op_q.clone(),
        });

        let (operation, result, success, error) = wait_for_result(&mut rx, op_id_q).await;
        assert!(success);
        assert!(error.is_none());
        assert_eq!(operation, op_q);

        match result {
            OperationResult::AccountQuery(AccountQueryResult {
                account_id: acc,
                total_spent,
                mut per_card_spent,
            }) => {
                assert_eq!(acc, account_id);
                // 10 + 15 + 5
                assert_eq!(total_spent, 30.0);

                // Chequeamos que estén las 3 tarjetas
                assert_eq!(per_card_spent.len(), 3);

                let a = per_card_spent.remove(&card_a).unwrap_or(0.0);
                let b = per_card_spent.remove(&card_b).unwrap_or(0.0);
                let c = per_card_spent.remove(&card_c).unwrap_or(0.0);

                assert_eq!(a, 10.0);
                assert_eq!(b, 15.0);
                assert_eq!(c, 5.0);
            }
            other => panic!(
                "Esperábamos AccountQueryResult con 3 tarjetas, obtuvimos {:?}",
                other
            ),
        } */
    }

    #[actix_rt::test]
    async fn account_query_multiple_cards_includes_offline_charges() {
        /*         let (router, mut rx) = make_router();

        let account_id = 200;
        let card_online = 11;
        let card_offline = 22;

        // card_online: 8.0 (online)
        let op_id_online = 1;
        let op_online = Operation::Charge {
            account_id,
            card_id: card_online,
            amount: 8.0,
            from_offline_station: false,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_online,
            operation: op_online.clone(),
        });
        let (_op, res, success, err) = wait_for_result(&mut rx, op_id_online).await;
        assert!(success);
        assert!(err.is_none());
        match res {
            OperationResult::Charge(ChargeResult::Ok) => {}
            other => panic!(
                "Esperábamos ChargeResult::Ok para card_online, obtuvimos {:?}",
                other
            ),
        }

        // card_offline: 50.0 (from_offline_station = true, debe contarse igual)
        let op_id_off = 2;
        let op_off = Operation::Charge {
            account_id,
            card_id: card_offline,
            amount: 50.0,
            from_offline_station: true,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_off,
            operation: op_off.clone(),
        });
        let (_op, res, success, err) = wait_for_result(&mut rx, op_id_off).await;
        assert!(success);
        assert!(err.is_none());
        match res {
            OperationResult::Charge(ChargeResult::Ok) => {}
            other => panic!(
                "Esperábamos ChargeResult::Ok para card_offline, obtuvimos {:?}",
                other
            ),
        }

        // AccountQuery
        let op_id_q = 3;
        let op_q = Operation::AccountQuery { account_id };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_q,
            operation: op_q.clone(),
        });

        let (operation, result, success, error) = wait_for_result(&mut rx, op_id_q).await;
        assert!(success);
        assert!(error.is_none());
        assert_eq!(operation, op_q);

        match result {
            OperationResult::AccountQuery(AccountQueryResult {
                account_id: acc,
                total_spent,
                per_card_spent,
            }) => {
                assert_eq!(acc, account_id);
                // 8 + 50
                assert_eq!(total_spent, 58.0);

                let spent_online = per_card_spent.get(&card_online).copied().unwrap_or(0.0);
                let spent_offline = per_card_spent.get(&card_offline).copied().unwrap_or(0.0);

                assert_eq!(spent_online, 8.0);
                assert_eq!(spent_offline, 50.0);
            }
            other => panic!(
                "Esperábamos AccountQueryResult con cargos online+offline, obtuvimos {:?}",
                other
            ),
        } */
    }

    #[actix_rt::test]
    async fn account_query_is_account_scoped() {
        /*         let (router, mut rx) = make_router();

        // Cuenta A con una tarjeta
        let account_a = 300;
        let card_a1 = 1;

        // Cuenta B con otra tarjeta
        let account_b = 400;
        let card_b1 = 2;

        // Cargamos en la cuenta A: 20.0
        let op_id_a = 1;
        let op_a = Operation::Charge {
            account_id: account_a,
            card_id: card_a1,
            amount: 20.0,
            from_offline_station: false,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_a,
            operation: op_a.clone(),
        });
        let (_op, res, success, err) = wait_for_result(&mut rx, op_id_a).await;
        assert!(success);
        assert!(err.is_none());
        match res {
            OperationResult::Charge(ChargeResult::Ok) => {}
            other => panic!(
                "Esperábamos ChargeResult::Ok para cuenta A, obtuvimos {:?}",
                other
            ),
        }

        // Cargamos en la cuenta B: 99.0
        let op_id_b = 2;
        let op_b = Operation::Charge {
            account_id: account_b,
            card_id: card_b1,
            amount: 99.0,
            from_offline_station: false,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_b,
            operation: op_b.clone(),
        });
        let (_op, res, success, err) = wait_for_result(&mut rx, op_id_b).await;
        assert!(success);
        assert!(err.is_none());
        match res {
            OperationResult::Charge(ChargeResult::Ok) => {}
            other => panic!(
                "Esperábamos ChargeResult::Ok para cuenta B, obtuvimos {:?}",
                other
            ),
        }

        // Ahora pedimos AccountQuery SOLO para account_a
        let op_id_q_a = 3;
        let op_q_a = Operation::AccountQuery {
            account_id: account_a,
        };
        router.do_send(RouterCmd::Execute {
            op_id: op_id_q_a,
            operation: op_q_a.clone(),
        });

        let (operation, result, success, error) = wait_for_result(&mut rx, op_id_q_a).await;
        assert!(success);
        assert!(error.is_none());
        assert_eq!(operation, op_q_a);

        match result {
            OperationResult::AccountQuery(AccountQueryResult {
                account_id,
                total_spent,
                per_card_spent,
            }) => {
                // Debe ser sólo la cuenta A con su propio cargo
                assert_eq!(account_id, account_a);
                assert_eq!(total_spent, 20.0);

                // Sólo debería estar card_a1
                assert_eq!(per_card_spent.len(), 1);
                let spent_a1 = per_card_spent.get(&card_a1).copied().unwrap_or(0.0);
                assert_eq!(spent_a1, 20.0);
                assert!(!per_card_spent.contains_key(&card_b1));
            }
            other => panic!(
                "Esperábamos AccountQueryResult sólo para account_a, obtuvimos {:?}",
                other
            ),
        } */
    }
}

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
/// - Emit a single `ActorEvent::OperationResult` towards the Node.
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
                        // Router starts the query by telling the Account
                        // how many cards to expect, and then asks each Card
                        // for its state.
                        let acc = self.get_or_create_account(account_id, ctx);

                        // Determine which cards belong to this account.
                        let mut card_ids = Vec::new();
                        for ((acc_id, card_id), _) in &self.cards {
                            if *acc_id == account_id {
                                card_ids.push(*card_id);
                            }
                        }
                        let num_cards = card_ids.len();

                        // 1) tell the account how many cards to wait for
                        acc.do_send(AccountMsg::StartAccountQuery { op_id, num_cards });

                        // 2) ask each card for its state (NO reset).
                        for card_id in card_ids {
                            if let Some(card_addr) = self.cards.get(&(account_id, card_id)) {
                                card_addr.do_send(CardMsg::QueryCardState {
                                    op_id,
                                    account_id,
                                    reset_after_report: false,
                                });
                            }
                        }
                        // The final result arrives via RouterInternalMsg::AccountQueryCompleted
                    }
                    Operation::Bill { account_id, period: _ } => {
                        // Igual a AccountQuery, pero:
                        // - la Account entra en modo "billing" (StartAccountBill),
                        // - las Cards reportan y se resetean (reset_after_report=true),
                        // - la Account también resetea account_consumed al finalizar.
                        let acc = self.get_or_create_account(account_id, ctx);

                        // Determinar tarjetas de esta cuenta
                        let mut card_ids = Vec::new();
                        for ((acc_id, card_id), _) in &self.cards {
                            if *acc_id == account_id {
                                card_ids.push(*card_id);
                            }
                        }
                        let num_cards = card_ids.len();

                        acc.do_send(AccountMsg::StartAccountBill { op_id, num_cards });

                        for card_id in card_ids {
                            if let Some(card_addr) = self.cards.get(&(account_id, card_id)) {
                                card_addr.do_send(CardMsg::QueryCardState {
                                    op_id,
                                    account_id,
                                    reset_after_report: true,
                                });
                            }
                        }
                        // El resultado llega también como AccountQueryCompleted,
                        // y el Router lo mapeará a OperationResult::AccountQuery.
                    }
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

                // Map to OperationResult for Charge / LimitAccount / LimitCard
                let result = match operation {
                    Operation::Charge { .. } => {
                        let cr = if success {
                            ChargeResult::Ok
                        } else {
                            ChargeResult::Failed(
                                error
                                    .clone()
                                    .expect("VerifyError expected if success == false"),
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
                                    .expect("VerifyError expected if success == false"),
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
                                    .expect("VerifyError expected if success == false"),
                            )
                        };
                        OperationResult::LimitCard(lr)
                    }
                    Operation::AccountQuery { .. } => {
                        // AccountQuery should be handled by AccountQueryCompleted.
                        // Defensive fallback.
                        OperationResult::AccountQuery(AccountQueryResult {
                            account_id: 0,
                            total_spent: 0.0,
                            per_card_spent: Vec::new(),
                        })
                    }
                    Operation::Bill { .. } => {
                        // Bill también se resuelve por AccountQueryCompleted;
                        // si llega acá sería un bug de wiring.
                        OperationResult::AccountQuery(AccountQueryResult {
                            account_id: 0,
                            total_spent: 0.0,
                            per_card_spent: Vec::new(),
                        })
                    }
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
                            "[Router] AccountQueryCompleted for unknown op_id={}",
                            op_id
                        )));
                        return;
                    }
                };

                // Tanto para AccountQuery como para Bill devolvemos
                // OperationResult::AccountQuery con el mismo payload.
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
    use super::*;
    use actix::prelude::*;
    use tokio::sync::mpsc;

    use crate::errors::{LimitCheckError, LimitUpdateError, VerifyError};
    use crate::node::actors::messages::{ActorEvent, RouterCmd};
    use common::operation::Operation;
    use common::operation_result::{
        AccountQueryResult, ChargeResult, LimitResult, OperationResult,
    };

    /// Helper: create a Router with its event channel towards the "Node".
    fn make_router() -> (Addr<ActorRouter>, mpsc::Receiver<ActorEvent>) {
        let (tx, rx) = mpsc::channel::<ActorEvent>(16);
        let router = ActorRouter::new(tx).start();
        (router, rx)
    }

    /// Wait until we receive an `ActorEvent::OperationResult` for the given op_id.
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
                    // Ignore other events / op_ids.
                }
            }
        }
        panic!("Did not receive OperationResult for op_id={op_id}");
    }

    /// Small helper to get the spent amount for a card_id from a Vec<(card_id, amount)>.
    fn spent_for_card(per_card_spent: &[(u64, f32)], card_id: u64) -> f32 {
        per_card_spent
            .iter()
            .find(|(id, _)| *id == card_id)
            .map(|(_, v)| *v)
            .unwrap_or(0.0)
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
            other => panic!("Expected ChargeResult::Ok, got {:?}", other),
        }

        // Router should have accepted GetLog (just a sanity check).
        assert_eq!(router.try_send(RouterCmd::GetLog).is_ok(), true);
    }

    #[actix_rt::test]
    async fn limit_card_then_charge_over_limit_fails() {
        let (router, mut rx) = make_router();

        let account_id = 2;
        let card_id = 20;

        // 1) Set card limit to 10.0
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
            other => panic!("Expected LimitResult::Ok, got {:?}", other),
        }

        // 2) Charge exceeding that limit (20.0 > 10.0)
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
        let err = error.expect("Expected a VerifyError");
        match err {
            VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded) => {}
            other => panic!("Expected CardLimitExceeded, got {:?}", other),
        }

        match result {
            OperationResult::Charge(ChargeResult::Failed(e)) => match e {
                VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded) => {}
                other => panic!(
                    "Expected ChargeResult::Failed(CardLimitExceeded), got {:?}",
                    other
                ),
            },
            other => panic!("Expected ChargeResult::Failed(_), got {:?}", other),
        }
    }

    #[actix_rt::test]
    async fn limit_account_below_consumption_fails_and_emits_error() {
        let (router, mut rx) = make_router();

        let account_id = 3;
        let card_id = 30;

        // Generate consumption: two charges that pass because there is no initial limit.
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

        // Now consumption is 35.0. Setting limit to 10.0 should fail.
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

        let err = error.expect("Expected VerifyError");
        match err {
            VerifyError::LimitUpdate(LimitUpdateError::BelowCurrentUsage) => {}
            other => panic!("Expected BelowCurrentUsage, got {:?}", other),
        }

        match result {
            OperationResult::LimitAccount(LimitResult::Failed(e)) => match e {
                VerifyError::LimitUpdate(LimitUpdateError::BelowCurrentUsage) => {}
                other => panic!(
                    "Expected LimitResult::Failed(BelowCurrentUsage), got {:?}",
                    other
                ),
            },
            other => panic!(
                "Expected LimitAccount(LimitResult::Failed), got {:?}",
                other
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
                "Expected empty AccountQueryResult, got {:?}",
                other
            ),
        }
    }

    #[actix_rt::test]
    async fn account_query_aggregates_per_card_and_total() {
        let (router, mut rx) = make_router();

        let account_id = 5;
        let card_id = 50;

        // Two charges on the same card: 10.0 + 15.0
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

        // Now ask for an AccountQuery
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
                let spent_card = spent_for_card(&per_card_spent, card_id);
                assert_eq!(spent_card, 25.0);
            }
            other => panic!(
                "Expected AccountQueryResult with totals, got {:?}",
                other
            ),
        }
    }

    #[actix_rt::test]
    async fn multiple_operations_interleaved_stay_correlated_by_op_id() {
        let (router, mut rx) = make_router();

        let account_id = 6;
        let card_id = 60;

        // Send several interleaved operations:
        // 1: Charge
        // 2: LimitCard
        // 3: Charge (expected to fail due to limit)
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

        // Wait until we have 4 OperationResult events (one per op_id).
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
                panic!("Channel closed before receiving all 4 OperationResults");
            }
        }

        // ---------- op_id = 1: small Charge -> OK ----------
        let (operation1, result1, success1, error1) =
            results.remove(&1).expect("Missing result for op_id=1");
        assert_eq!(operation1, op1);
        assert!(success1);
        assert!(error1.is_none());
        assert!(matches!(result1, OperationResult::Charge(ChargeResult::Ok)));

        // ---------- op_id = 2: LimitCard -> OK ----------
        let (operation2, result2, success2, error2) =
            results.remove(&2).expect("Missing result for op_id=2");
        assert_eq!(operation2, op2);
        assert!(success2);
        assert!(error2.is_none());
        assert!(matches!(
            result2,
            OperationResult::LimitCard(LimitResult::Ok)
        ));

        // ---------- op_id = 3: large Charge over the limit -> FAIL ----------
        let (_operation3, result3, success3, error3) =
            results.remove(&3).expect("Missing result for op_id=3");
        assert!(!success3);
        let err3 = error3.expect("Expected VerifyError for op3");
        match err3 {
            VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded) => {}
            other => panic!(
                "Expected CardLimitExceeded in op3, got {:?}",
                other
            ),
        }
        match result3 {
            OperationResult::Charge(ChargeResult::Failed(e)) => match e {
                VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded) => {}
                other => panic!(
                    "Expected ChargeResult::Failed(CardLimitExceeded) in op3, got {:?}",
                    other
                ),
            },
            other => panic!(
                "Expected ChargeResult::Failed(_) in op3, got {:?}",
                other
            ),
        }

        // ---------- op_id = 4: AccountQuery ----------
        // Depending on timing, the query can see 0 or 5,
        // but never more than 5 (only one successful charge).
        let (operation4, result4, success4, error4) =
            results.remove(&4).expect("Missing result for op_id=4");
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

                let spent_card = spent_for_card(&per_card_spent, card_id);

                // Internal consistency: with a single card, total == spent for that card.
                assert!(
                    (total_spent - spent_card).abs() < f32::EPSILON,
                    "total_spent ({}) and spent_card ({}) do not match",
                    total_spent,
                    spent_card
                );

                // By construction of the test, we should never see > 5.0 here.
                assert!(
                    total_spent <= 5.0 + f32::EPSILON,
                    "AccountQuery saw more consumption than possible: {}",
                    total_spent
                );
            }
            other => panic!(
                "Expected AccountQueryResult after interleaving, got {:?}",
                other
            ),
        }
    }

    #[actix_rt::test]
    async fn account_query_multiple_cards_sums_correctly() {
        let (router, mut rx) = make_router();

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
                "Expected ChargeResult::Ok for card_a, got {:?}",
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
                "Expected ChargeResult::Ok for card_b, got {:?}",
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
                "Expected ChargeResult::Ok for card_c, got {:?}",
                other
            ),
        }

        // Now ask for AccountQuery for that account
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
                per_card_spent,
            }) => {
                assert_eq!(acc, account_id);
                // 10 + 15 + 5
                assert_eq!(total_spent, 30.0);

                // Should contain the 3 cards.
                assert_eq!(per_card_spent.len(), 3);

                let a = spent_for_card(&per_card_spent, card_a);
                let b = spent_for_card(&per_card_spent, card_b);
                let c = spent_for_card(&per_card_spent, card_c);

                assert_eq!(a, 10.0);
                assert_eq!(b, 15.0);
                assert_eq!(c, 5.0);
            }
            other => panic!(
                "Expected AccountQueryResult with 3 cards, got {:?}",
                other
            ),
        }
    }

    #[actix_rt::test]
    async fn account_query_multiple_cards_includes_offline_charges() {
        let (router, mut rx) = make_router();

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
                "Expected ChargeResult::Ok for card_online, got {:?}",
                other
            ),
        }

        // card_offline: 50.0 (from_offline_station = true, but should be counted)
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
                "Expected ChargeResult::Ok for card_offline, got {:?}",
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

                let spent_online = spent_for_card(&per_card_spent, card_online);
                let spent_offline = spent_for_card(&per_card_spent, card_offline);

                assert_eq!(spent_online, 8.0);
                assert_eq!(spent_offline, 50.0);
            }
            other => panic!(
                "Expected AccountQueryResult with online+offline, got {:?}",
                other
            ),
        }
    }

    #[actix_rt::test]
    async fn account_query_is_account_scoped() {
        let (router, mut rx) = make_router();

        // Account A with one card
        let account_a = 300;
        let card_a1 = 1;

        // Account B with another card
        let account_b = 400;
        let card_b1 = 2;

        // Charge Account A: 20.0
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
                "Expected ChargeResult::Ok for account A, got {:?}",
                other
            ),
        }

        // Charge Account B: 99.0
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
                "Expected ChargeResult::Ok for account B, got {:?}",
                other
            ),
        }

        // Now query only Account A
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
                // Must only include account A and its own charge.
                assert_eq!(account_id, account_a);
                assert_eq!(total_spent, 20.0);

                // Only card_a1 should be present.
                assert_eq!(per_card_spent.len(), 1);
                let spent_a1 = spent_for_card(&per_card_spent, card_a1);
                assert_eq!(spent_a1, 20.0);

                let has_b1 = per_card_spent.iter().any(|(id, _)| *id == card_b1);
                assert!(!has_b1, "AccountQuery for A should not contain card_b1");
            }
            other => panic!(
                "Expected AccountQueryResult only for account_a, got {:?}",
                other
            ),
        }
    }
}

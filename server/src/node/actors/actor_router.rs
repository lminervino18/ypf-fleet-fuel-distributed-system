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
                        for ((acc_id, card_id), _) in &self.cards {
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
                            per_card_spent: HashMap::new(),
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
                            "[Router] AccountQueryCompleted for unknown op_id={}",
                            op_id
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

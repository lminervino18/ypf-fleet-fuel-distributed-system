use actix::prelude::*;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::sync::mpsc;

use crate::node::actors::messages::{
    AccountMsg, ActorEvent, CardMsg, RouterCmd, RouterInternalMsg,
};

use super::account::AccountActor;
use super::card::CardActor;
use common::operation::{Operation, AccountSnapshot, CardSnapshot, DatabaseSnapshot};
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

    /// Canal para snapshots de DB en curso (GetDatabase).
    pending_db_snapshots: HashMap<u32, PendingDatabaseSnapshot>,

    /// Channel for sending ActorEvent messages to the Node.
    pub event_tx: mpsc::Sender<ActorEvent>,
}

/// Estado interno mientras se arma un snapshot de base de datos.
struct PendingDatabaseSnapshot {
    addr: SocketAddr,
    expected_accounts: usize,
    expected_cards: usize,
    accounts: Vec<AccountSnapshot>,
    cards: Vec<CardSnapshot>,
}

impl ActorRouter {
    /// Create a new ActorRouter with the given event channel.
    pub fn new(event_tx: mpsc::Sender<ActorEvent>) -> Self {
        Self {
            accounts: HashMap::new(),
            cards: HashMap::new(),
            operations: HashMap::new(),
            pending_db_snapshots: HashMap::new(),
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

    /// Intentar completar un snapshot de DB (GetDatabase) si ya llegaron todos los fragments.
    fn try_complete_db_snapshot(&mut self, op_id: u32) {
        let done = match self.pending_db_snapshots.get(&op_id) {
            Some(p) => {
                p.accounts.len() == p.expected_accounts && p.cards.len() == p.expected_cards
            }
            None => return,
        };

        if !done {
            return;
        }

        // Ya están todos los snapshots; los sacamos del mapa.
        let pending = match self.pending_db_snapshots.remove(&op_id) {
            Some(p) => p,
            None => return,
        };

        let operation = match self.operations.get(&op_id) {
            Some(op) => op.clone(),
            None => {
                self.emit(ActorEvent::Debug(format!(
                    "[Router] DB snapshot completed for unknown op_id={}",
                    op_id
                )));
                return;
            }
        };

        let snapshot = DatabaseSnapshot {
            addr: pending.addr,
            accounts: pending.accounts,
            cards: pending.cards,
        };

        let result = OperationResult::DatabaseSnapshot(snapshot);

        self.emit(ActorEvent::OperationResult {
            op_id,
            operation,
            result,
            success: true,
            error: None,
        });
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
                        for (acc_id, card_id) in self.cards.keys() {
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

                    Operation::GetDatabase { addr } => {
                        let expected_accounts = self.accounts.len();
                        let expected_cards = self.cards.len();

                        if expected_accounts == 0 && expected_cards == 0 {
                            // DB vacía: respondemos directamente sin mandar mensajes.
                            let snapshot = DatabaseSnapshot {
                                addr,
                                accounts: Vec::new(),
                                cards: Vec::new(),
                            };

                            let operation = Operation::GetDatabase { addr };
                            let result = OperationResult::DatabaseSnapshot(snapshot);

                            self.emit(ActorEvent::OperationResult {
                                op_id,
                                operation,
                                result,
                                success: true,
                                error: None,
                            });
                            return;
                        }

                        // Registramos el pending de snapshot
                        self.pending_db_snapshots.insert(
                            op_id,
                            PendingDatabaseSnapshot {
                                addr,
                                expected_accounts,
                                expected_cards,
                                accounts: Vec::new(),
                                cards: Vec::new(),
                            },
                        );

                        // Pedimos snapshot a todas las cuentas
                        for (_id, acc_addr) in &self.accounts {
                            acc_addr.do_send(AccountMsg::GetSnapshot { op_id });
                        }

                        // Y a todas las tarjetas
                        for ((_acc_id, _card_id), card_addr) in &self.cards {
                            card_addr.do_send(CardMsg::GetSnapshot { op_id });
                        }
                    }

                    Operation::ReplaceDatabase { snapshot } => {
                        // Aplicar snapshot a todas las cuentas
                        for acc_snap in &snapshot.accounts {
                            let acc = self.get_or_create_account(acc_snap.account_id, ctx);
                            acc.do_send(AccountMsg::ReplaceState {
                                new_limit: acc_snap.limit,
                                new_consumed: acc_snap.consumed,
                            });
                        }

                        // Aplicar snapshot a todas las tarjetas
                        for card_snap in &snapshot.cards {
                            let card = self.get_or_create_card(card_snap.account_id, card_snap.card_id, ctx);
                            card.do_send(CardMsg::ReplaceState {
                                new_limit: card_snap.limit,
                                new_consumed: card_snap.consumed,
                            });
                        }

                        let operation = Operation::ReplaceDatabase { snapshot };
                        let result = OperationResult::ReplaceDatabase;

                        self.emit(ActorEvent::OperationResult {
                            op_id,
                            operation,
                            result,
                            success: true,
                            error: None,
                        });
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
                    Operation::GetDatabase { .. } => {
                        // No debería resolverse por OperationCompleted.
                        // Fallback defensivo: devolvemos ReplaceDatabase (no usado).
                        OperationResult::ReplaceDatabase
                    }
                    Operation::ReplaceDatabase { .. } => {
                        // Tampoco se espera por OperationCompleted, pero por completitud:
                        OperationResult::ReplaceDatabase
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
                            "[Router] AccountQueryCompleted for unknown op_id={op_id}"
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

            RouterInternalMsg::AccountSnapshotCollected { op_id, snapshot } => {
                if let Some(p) = self.pending_db_snapshots.get_mut(&op_id) {
                    p.accounts.push(snapshot);
                    self.try_complete_db_snapshot(op_id);
                } else {
                    self.emit(ActorEvent::Debug(format!(
                        "[Router] AccountSnapshotCollected for unknown op_id={}",
                        op_id
                    )));
                }
            }

            RouterInternalMsg::CardSnapshotCollected { op_id, snapshot } => {
                if let Some(p) = self.pending_db_snapshots.get_mut(&op_id) {
                    p.cards.push(snapshot);
                    self.try_complete_db_snapshot(op_id);
                } else {
                    self.emit(ActorEvent::Debug(format!(
                        "[Router] CardSnapshotCollected for unknown op_id={}",
                        op_id
                    )));
                }
            }

            RouterInternalMsg::Debug(msg) => {
                self.emit(ActorEvent::Debug(msg));
            }
        }
    }
}

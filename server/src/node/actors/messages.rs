use actix::prelude::*;
use std::collections::HashMap;

use crate::errors::VerifyError;
use common::operation::{Operation, AccountSnapshot, CardSnapshot};
use common::operation_result::OperationResult;

/// Events sent by the ActorRouter to the Node.
#[derive(Debug, Clone)]
pub enum ActorEvent {
    /// Final outcome for a given operation.
    OperationResult {
        op_id: u32,
        operation: Operation,
        result: OperationResult,
        success: bool,
        error: Option<VerifyError>,
    },

    /// Generic debug / diagnostic message.
    Debug(String),
}

/// Requests sent by the Node to the ActorRouter.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum RouterCmd {
    /// Execute a complete business operation.
    Execute { op_id: u32, operation: Operation },

    /// Debug / introspection of the router.
    GetLog,
}

/// Messages handled by AccountActor.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum AccountMsg {
    /// Charge requested by a specific card.
    ApplyChargeFromCard {
        op_id: u32,
        amount: f32,
        card_id: u64,
        from_offline_station: bool,
        reply_to: Recipient<AccountChargeReply>,
    },

    /// Request to change the account-wide limit.
    ApplyAccountLimit { op_id: u32, new_limit: Option<f32> },

    /// Inicio de un query de cuenta (no resetea nada).
    StartAccountQuery { op_id: u32, num_cards: usize },

    /// Inicio de un Bill de cuenta (al final resetea consumos).
    StartAccountBill { op_id: u32, num_cards: usize },

    /// Respuesta de una tarjeta a un query/bill de cuenta.
    CardQueryReply {
        op_id: u32,
        card_id: u64,
        consumed: f32,
    },

    /// Pedirle al AccountActor un snapshot completo de su estado
    /// (límite + consumo), para construir la DatabaseSnapshot.
    ///
    /// El resultado vuelve al Router como
    /// `RouterInternalMsg::AccountSnapshotCollected { .. }`.
    GetSnapshot {
        op_id: u32,
    },

    /// Reemplazar el estado interno de la cuenta según un snapshot.
    ///
    /// - `new_limit`: nuevo límite de cuenta (o None si sin límite).
    /// - `new_consumed`: nuevo consumo acumulado de la cuenta.
    ReplaceState {
        new_limit: Option<f32>,
        new_consumed: f32,
    },
}

/// Reply from AccountActor back to CardActor for a specific charge.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct AccountChargeReply {
    pub op_id: u32,
    pub success: bool,
    pub error: Option<VerifyError>,
}

/// Messages handled by CardActor.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum CardMsg {
    /// Execute a charge for this card.
    ExecuteCharge {
        op_id: u32,
        account_id: u64,
        card_id: u64,
        amount: f32,
        from_offline_station: bool,
    },

    /// Execute a card-limit change.
    ExecuteLimitChange { op_id: u32, new_limit: Option<f32> },

    /// Query this card's current consumption (used by account queries and bills).
    ///
    /// - `reset_after_report = false`: AccountQuery normal.
    /// - `reset_after_report = true`: Bill → se resetea `consumed` tras reportar.
    QueryCardState {
        op_id: u32,
        account_id: u64,
        reset_after_report: bool,
    },

    /// Pedirle al CardActor un snapshot completo de su estado
    /// (límite + consumo), para construir la DatabaseSnapshot.
    ///
    /// El resultado vuelve al Router como
    /// `RouterInternalMsg::CardSnapshotCollected { .. }`.
    GetSnapshot {
        op_id: u32,
    },

    /// Reemplazar el estado interno de la tarjeta según un snapshot.
    ///
    /// - `new_limit`: nuevo límite de tarjeta (o None si sin límite).
    /// - `new_consumed`: nuevo consumo acumulado de la tarjeta.
    ReplaceState {
        new_limit: Option<f32>,
        new_consumed: f32,
    },

    /// Generic debug / diagnostic for the card.
    Debug(String),
}

/// Internal messages from AccountActor/CardActor to ActorRouter.
#[derive(Debug, Message)]
#[rtype(result = "()")]
pub enum RouterInternalMsg {
    /// Una operación de negocio terminó (charge / limit).
    OperationCompleted {
        op_id: u32,
        success: bool,
        error: Option<VerifyError>,
    },

    /// Un query o un bill de cuenta terminó.
    ///
    /// Para Bill, los actores ya habrán reseteado sus consumos, pero acá
    /// devolvemos el mismo tipo de payload que para AccountQuery.
    AccountQueryCompleted {
        op_id: u32,
        account_id: u64,
        total_spent: f32,
        per_card_spent: Vec<(u64, f32)>,
    },

    /// Snapshot de una cuenta, para construir la DatabaseSnapshot
    /// en respuesta a `Operation::GetDatabase`.
    AccountSnapshotCollected {
        op_id: u32,
        snapshot: AccountSnapshot,
    },

    /// Snapshot de una tarjeta, para construir la DatabaseSnapshot
    /// en respuesta a `Operation::GetDatabase`.
    CardSnapshotCollected {
        op_id: u32,
        snapshot: CardSnapshot,
    },

    /// Internal debug / diagnostics.
    Debug(String),
}

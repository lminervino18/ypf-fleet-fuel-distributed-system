use actix::prelude::*;
use std::collections::HashMap;

use crate::errors::VerifyError;
use common::operation::Operation;
use common::operation_result::OperationResult;

/// Events sent by the ActorRouter to the Node.
///
/// Ahora incluye un `OperationResult` de alto nivel que matchea con
/// la `Operation` original (Charge / LimitAccount / LimitCard / AccountQuery).
#[derive(Debug, Clone)]
pub enum ActorEvent {
    /// Final outcome for a given operation.
    OperationResult {
        op_id: u32,
        operation: Operation,
        result: OperationResult,
        /// Para compat con el código actual del nodo; se puede ir
        /// eliminando cuando uses solo `OperationResult`.
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
///
/// Flujos:
/// - Charges:
///     CardActor → AccountActor → AccountChargeReply → CardActor.
/// - Account limit changes:
///     Router → AccountActor → RouterInternalMsg::OperationCompleted.
/// - Account queries:
///     Router → AccountActor::StartAccountQuery
///          → Router envía CardMsg::QueryCardState a todas las cards
///          → CardActor → AccountActor::CardQueryReply (una por tarjeta)
///          → AccountActor → RouterInternalMsg::AccountQueryCompleted
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

    /// Inicio de un query de cuenta.
    ///
    /// `num_cards` indica cuántas tarjetas se van a consultar; la Account
    /// espera exactamente esa cantidad de `CardQueryReply` antes de
    /// responderle al Router.
    StartAccountQuery { op_id: u32, num_cards: usize },

    /// Respuesta de una tarjeta a un query de cuenta.
    ///
    /// Cada CardActor responde con su consumo actual, y la Account
    /// va acumulando hasta tener las `num_cards` esperadas.
    CardQueryReply {
        op_id: u32,
        card_id: u64,
        consumed: f32,
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

    /// Query this card's current consumption (used by account queries).
    QueryCardState { op_id: u32, account_id: u64 },

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

    /// Un query de cuenta terminó (todas las tarjetas respondieron).
    AccountQueryCompleted {
        op_id: u32,
        account_id: u64,
        total_spent: f32,
        per_card_spent: HashMap<u64, f32>,
    },

    /// Internal debug / diagnostics.
    Debug(String),
}

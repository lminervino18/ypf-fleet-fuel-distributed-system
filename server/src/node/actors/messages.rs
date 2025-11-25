use actix::prelude::*;

use crate::errors::VerifyError;
use common::operation::Operation;
use common::operation_result::OperationResult;

/// Events sent by the ActorRouter to the Node.
#[derive(Debug, Clone)]
pub enum ActorEvent {
    /// Final outcome for a given operation.
    OperationResult {
        op_id: u32,
        operation: Operation,
        result: OperationResult,
        #[allow(dead_code)]
        success: bool,
        #[allow(dead_code)]
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
    #[allow(dead_code)]
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
        #[allow(dead_code)]
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

    /// Internal debug / diagnostics.
    Debug(String),
}

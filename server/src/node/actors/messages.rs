use actix::prelude::*;

use crate::errors::VerifyError;
use common::operation::{AccountSnapshot, CardSnapshot, Operation};
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

    /// Start an account query (does not reset consumption).
    StartAccountQuery { op_id: u32, num_cards: usize },

    /// Start an account billing operation (resets consumption after completion).
    StartAccountBill { op_id: u32, num_cards: usize },

    /// Reply from a card to an account query or billing request.
    CardQueryReply {
        op_id: u32,
        card_id: u64,
        consumed: f32,
    },

    /// Request a complete snapshot of the account state (limit + consumed).
    ///
    /// The result is delivered back to the Router as
    /// `RouterInternalMsg::AccountSnapshotCollected { .. }`.
    GetSnapshot { op_id: u32 },

    /// Replace the internal account state using a snapshot.
    ///
    /// - `new_limit`: new account limit (`None` means no limit).
    /// - `new_consumed`: new accumulated consumption.
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
    /// If `reset_after_report` is false this is a normal AccountQuery.
    /// If `reset_after_report` is true this is part of a Bill and the card
    /// should reset its `consumed` value after reporting.
    QueryCardState {
        op_id: u32,
        account_id: u64,
        reset_after_report: bool,
    },

    /// Request a complete snapshot of the card state (limit + consumed).
    ///
    /// The result is delivered back to the Router as
    /// `RouterInternalMsg::CardSnapshotCollected { .. }`.
    GetSnapshot { op_id: u32 },

    /// Replace the internal card state using a snapshot.
    ///
    /// - `new_limit`: new card limit (`None` means no limit).
    /// - `new_consumed`: new accumulated consumption.
    ReplaceState {
        new_limit: Option<f32>,
        new_consumed: f32,
    },

    #[allow(dead_code)]
    /// Generic debug / diagnostic for the card.
    Debug(String),
}

/// Internal messages from AccountActor/CardActor to ActorRouter.
#[derive(Debug, Message)]
#[rtype(result = "()")]
pub enum RouterInternalMsg {
    /// A business operation has finished (e.g., charge or limit change).
    OperationCompleted {
        op_id: u32,
        success: bool,
        error: Option<VerifyError>,
    },

    /// An account query or a billing run has completed.
    ///
    /// For billing, actors will have already reset their consumption, but the
    /// payload is identical to an AccountQuery result.
    AccountQueryCompleted {
        op_id: u32,
        account_id: u64,
        total_spent: f32,
        per_card_spent: Vec<(u64, f32)>,
    },

    /// Account snapshot fragment collected while assembling a DatabaseSnapshot.
    AccountSnapshotCollected {
        op_id: u32,
        snapshot: AccountSnapshot,
    },

    /// Card snapshot fragment collected while assembling a DatabaseSnapshot.
    CardSnapshotCollected { op_id: u32, snapshot: CardSnapshot },

    /// Internal debug / diagnostics.
    Debug(String),
}


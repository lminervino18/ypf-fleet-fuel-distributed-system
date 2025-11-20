// src/actors/messages.rs

use actix::prelude::*;
use crate::domain::Operation;
use crate::errors::{ApplyError, VerifyError};

/// Events sent by the ActorRouter to the Node.
///
/// Used to report the result of verification and application phases,
/// as well as for generic debugging.
#[derive(Debug, Clone)]
pub enum ActorEvent {
    /// Response to Verify(op).
    VerifyResult {
        op_id: u64,
        operation: Operation,
        allowed: bool,
        error: Option<VerifyError>,
    },

    /// Response to Apply(op).
    ApplyResult {
        op_id: u64,
        operation: Operation,
        success: bool,
        error: Option<ApplyError>,
    },

    /// Generic debug message.
    Debug(String),
}

/// Requests sent by the Node to the ActorRouter.
///
/// Used to initiate verification and application phases, and for future
/// extensions such as retrieving the local log.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum RouterCmd {
    /// Verification phase.
    Verify {
        op_id: u64,
        operation: Operation,
    },

    /// Application phase (after successful Verify).
    Apply {
        op_id: u64,
        operation: Operation,
    },

    /// Future use: return local log.
    GetLog,
}

/// Messages handled by AccountActor.
///
/// Used for verifying and applying charges and limit changes at the account level.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum AccountMsg {
    VerifyCharge {
        amount: f64,
        op_id: u64,
    },

    VerifyLimitChange {
        new_limit: Option<f64>,
        op_id: u64,
    },

    ApplyCharge {
        card_id: u64,
        amount: f64,
        op_id: u64,
    },

    ApplyLimitChange {
        new_limit: Option<f64>,
        op_id: u64,
    },
}

/// Messages handled by CardActor.
///
/// Used for verifying and applying charges and limit changes at the card level.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum CardMsg {
    VerifyCharge {
        amount: f64,
        op_id: u64,
        account_id: u64,
        card_id: u64,
    },

    VerifyLimitChange {
        new_limit: Option<f64>,
        op_id: u64,
    },

    ApplyCharge {
        amount: f64,
        op_id: u64,
    },

    ApplyLimitChange {
        new_limit: Option<f64>,
        op_id: u64,
    },

    Debug(String),
}

/// Internal messages from AccountActor/CardActor to ActorRouter.
///
/// Used to report verification and application results, and for debugging.
#[derive(Debug, Message)]
#[rtype(result = "()")]
pub enum RouterInternalMsg {
    VerifyResult {
        op_id: u64,
        allowed: bool,
        error: Option<VerifyError>,
    },

    ApplyResult {
        op_id: u64,
        success: bool,
        error: Option<ApplyError>,
    },

    Debug(String),
}

use actix::prelude::*;

use crate::domain::Operation;
use crate::errors::VerifyError;

/// Events sent by the ActorRouter to the Node.
///
/// The router exposes a single high-level result:
/// the outcome of executing a complete business operation
/// (verification + state changes).
#[derive(Debug, Clone)]
pub enum ActorEvent {
    /// Final outcome for a given operation.
    ///
    /// - `success == true`  → all checks passed and state was updated.
    /// - `success == false` → some business rule failed; from the Node
    ///   perspective the operation is rejected.
    OperationResult {
        op_id: u64,
        operation: Operation,
        success: bool,
        /// Domain/business error, if any (limits, invalid updates, etc.).
        error: Option<VerifyError>,
    },

    /// Generic debug / diagnostic message.
    Debug(String),
}

/// Requests sent by the Node to the ActorRouter.
///
/// The Node no longer performs a separate Verify/Apply protocol.
/// Instead, it asks the router to *execute* a full operation:
/// the router will validate invariants and apply the state changes
/// if allowed, and then return a single `OperationResult`.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum RouterCmd {
    /// Execute a complete business operation.
    Execute {
        op_id: u64,
        operation: Operation,
    },

    /// Debug / introspection of the router.
    GetLog,
}

/// Messages handled by AccountActor.
///
/// These are "low-level" operations relative to the Node, but from the
/// router's point of view they are internal implementation details.
///
/// For charges:
/// - CardActor sends `ApplyChargeFromCard` to AccountActor.
/// - AccountActor verifies account-wide limits, applies the change if
///   allowed, and replies back to the card via `AccountChargeReply`.
///
/// For account limit changes:
/// - Router sends `ApplyAccountLimit` directly to AccountActor.
/// - AccountActor verifies + applies, then notifies the router via
///   `RouterInternalMsg::OperationCompleted`.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum AccountMsg {
    /// Charge requested by a specific card.
    ///
    /// The account must:
    /// 1) verify account-wide limits for the charge,
    /// 2) apply the charge to its own state if allowed,
    /// 3) reply to the card via `reply_to`.
    ApplyChargeFromCard {
        op_id: u64,
        amount: f64,
        card_id: u64,
        /// Where the account will send the result of this charge.
        reply_to: Recipient<AccountChargeReply>,
    },

    /// Request to change the account-wide limit.
    ///
    /// The account must:
    /// 1) verify that `new_limit` is not below already consumed usage,
    /// 2) apply the new limit if allowed,
    /// 3) notify the router via `RouterInternalMsg::OperationCompleted`.
    ApplyAccountLimit {
        op_id: u64,
        new_limit: Option<f64>,
    },
}

/// Reply from AccountActor back to CardActor for a specific charge.
///
/// This is used so that the card can finish the operation (by updating
/// its own local state) and then notify the router that the whole
/// charge (card + account) has completed.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct AccountChargeReply {
    pub op_id: u64,
    pub success: bool,
    pub error: Option<VerifyError>,
}

/// Messages handled by CardActor.
///
/// From the Node’s perspective there is only a single high-level
/// `Operation::Charge` or `Operation::LimitCard`, but internally we
/// map them to:
///
/// - `ExecuteCharge`: card-level execution of a charge, including:
///   * local limit checks (card),
///   * sending a request to the account,
///   * applying card state after account confirms.
/// - `ExecuteLimitChange`: change the limit of this card only.
///
/// The card will report final outcomes back to the router via
/// `RouterInternalMsg::OperationCompleted`.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum CardMsg {
    /// Execute a charge for this card.
    ExecuteCharge {
        op_id: u64,
        account_id: u64,
        card_id: u64,
        amount: f64,
    },

    /// Execute a card-limit change.
    ExecuteLimitChange {
        op_id: u64,
        new_limit: Option<f64>,
    },

    /// Generic debug / diagnostic for the card.
    Debug(String),
}

/// Internal messages from AccountActor/CardActor to ActorRouter.
///
/// The router receives *completed* operations (from the card or the
/// account) and turns them into `ActorEvent::OperationResult` for the
/// Node.
///
/// Semantics:
/// - `success == true`  and `error == None` → operation applied correctly.
/// - `success == false` → business failure.
#[derive(Debug, Message)]
#[rtype(result = "()")]
pub enum RouterInternalMsg {
    /// A business operation has finished from the perspective of the
    /// actor layer (card and/or account).
    OperationCompleted {
        op_id: u64,
        success: bool,
        error: Option<VerifyError>,
    },

    /// Internal debug / diagnostics.
    Debug(String),
}

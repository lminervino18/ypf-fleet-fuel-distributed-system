use actix::prelude::*;

use crate::errors::VerifyError;
use common::operation::Operation;

/// Events sent by the ActorRouter to the Node.
///
/// The router exposes a single high-level result:
/// the outcome of executing a complete business operation
/// (verification + state changes).
#[derive(Debug, Clone)]
pub enum ActorEvent {
    /// Final outcome for a given operation.
    ///
    /// - `success == true`  → all checks passed and state was updated,
    ///   or the operation came from an offline station and was applied
    ///   unconditionally by design.
    /// - `success == false` → some business rule failed for an ONLINE
    ///   operation; from the Node perspective the operation is rejected.
    OperationResult {
        op_id: u32,
        operation: Operation,
        success: bool,
        /// Domain/business error, if any (limits, invalid updates, etc.).
        /// For offline-replayed charges this will always be `None`.
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
    ///
    /// For `Operation::Charge` this includes:
    /// - card-level limit checks in CardActor (unless
    ///   `from_offline_station == true`),
    /// - account-level limit checks in AccountActor (unless
    ///   `from_offline_station == true`),
    /// - applying card + account consumption.
    Execute { op_id: u32, operation: Operation },

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
/// - AccountActor:
///     * if `from_offline_station == false`:
///         - verifies account-wide limits,
///         - applies the change if allowed,
///     * if `from_offline_station == true`:
///         - **skips all limit checks** and unconditionally applies,
/// - then replies back to the card via `AccountChargeReply`.
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
    /// 1) if `from_offline_station == false`:
    ///       - verify account-wide limits for the charge,
    ///       - apply the charge to its own state if allowed,
    ///         if `from_offline_station == true`:
    ///       - skip all limit checks and apply unconditionally,
    /// 2) reply to the card via `reply_to`.
    ApplyChargeFromCard {
        op_id: u32,
        amount: f32,
        card_id: u64,
        /// Whether this charge comes from an offline-station replay.
        /// If true, the AccountActor will skip all limit checks and
        /// always apply the charge.
        from_offline_station: bool,
        /// Where the account will send the result of this charge.
        reply_to: Recipient<AccountChargeReply>,
    },

    /// Request to change the account-wide limit.
    ///
    /// The account must:
    /// 1) verify that `new_limit` is not below already consumed usage,
    /// 2) apply the new limit if allowed,
    /// 3) notify the router via `RouterInternalMsg::OperationCompleted`.
    ApplyAccountLimit { op_id: u32, new_limit: Option<f32> },

    /// Execute a query for this account.
    ExecuteQueryAccount { op_id: u32 },

    /// Execute a query for the cards of this account.
    ExecuteQueryCards { op_id: u32 },

    /// Execute billing for this account for a given period (optional).
    ExecuteBilling { op_id: u32, period: Option<String> },
}

/// Reply from AccountActor back to CardActor for a specific charge.
///
/// This is used so that the card can finish the operation (by updating
/// its own local state) and then notify the router that the whole
/// charge (card + account) has completed.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub struct AccountChargeReply {
    pub op_id: u32,
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
///     When `from_offline_station == true`, **all limit checks are
///     skipped**; the charge is treated as already confirmed and must
///     be applied.
/// - `ExecuteLimitChange`: change the limit of this card only.
///
/// The card will report final outcomes back to the router via
/// `RouterInternalMsg::OperationCompleted`.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum CardMsg {
    /// Execute a charge for this card.
    ExecuteCharge {
        op_id: u32,
        account_id: u64,
        card_id: u64,
        amount: f32,
        /// Whether this charge originates from a previously OFFLINE
        /// station. If true, CardActor will skip card-level limit
        /// checks and forward it to AccountActor as an offline replay.
        from_offline_station: bool,
    },

    /// Execute a card-limit change.
    ExecuteLimitChange { op_id: u32, new_limit: Option<f32> },

    /// Execute a query for this card.
    ExecuteQueryCard { op_id: u32 },

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
/// - `success == true` and `error == None` → operation applied correctly
///   (either via normal checks or as an offline replay).
/// - `success == false` → business failure for an ONLINE operation.
#[derive(Debug, Message)]
#[rtype(result = "()")]
pub enum RouterInternalMsg {
    /// A business operation has finished from the perspective of the
    /// actor layer (card and/or account).
    OperationCompleted {
        op_id: u32,
        success: bool,
        error: Option<VerifyError>,
    },

    /// Internal debug / diagnostics.
    Debug(String),
}

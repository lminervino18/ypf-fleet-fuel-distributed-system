use actix::prelude::*;

/// Distributed operation that may be applied to the local state
/// and potentially replicated by the Node.
///
/// Only `Charge` is state-mutating; `CheckLimit` is a logical query.
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    /// Logical: "Can we charge this amount for this card and account?"
    CheckLimit {
        account_id: u64,
        card_id: u64,
        amount: f64,
        request_id: u64,
    },

    /// Mutating operation: apply a charge to card + account.
    Charge {
        account_id: u64,
        card_id: u64,
        amount: f64,
        timestamp: u64,
        op_id: u64,
    },
}

/// Scope for limit-related operations.
#[derive(Debug, Clone)]
pub enum LimitScope {
    Card,
    Account,
}

/// Errors when checking limits.
#[derive(Debug, Clone)]
pub enum LimitCheckError {
    CardLimitExceeded,
    AccountLimitExceeded,
}

/// Errors when updating limits.
#[derive(Debug, Clone)]
pub enum LimitUpdateError {
    /// Attempting to set a limit below the amount already consumed.
    BelowCurrentUsage,
}

/// Events emitted by the actor world to the Node (over the mpsc channel).
#[derive(Debug, Clone)]
pub enum ActorEvent {
    /// Result of a limit authorization flow (card + account).
    ///
    /// - `allowed = true`  → charge is authorized.
    /// - `allowed = false` → `error` explains why.
    LimitCheckResult {
        request_id: u64,
        allowed: bool,
        error: Option<LimitCheckError>,
    },

    /// A charge has been applied to the local state.
    /// The Node can use this for replication/commit logic.
    ChargeApplied {
        operation: Operation,
    },

    /// A limit (card or account) has been successfully updated.
    LimitUpdated {
        scope: LimitScope,
        account_id: u64,
        card_id: Option<u64>,
        new_limit: Option<f64>,
    },

    /// A limit update was rejected (e.g., below current usage).
    LimitUpdateFailed {
        scope: LimitScope,
        account_id: u64,
        card_id: Option<u64>,
        request_id: u64,
        error: LimitUpdateError,
    },

    /// Generic debug/info message.
    Debug(String),
}

/// Commands that the Node sends into the actor world via ActorRouter.
///
/// This is the public API for a station node:
/// - authorize a charge,
/// - apply a charge (without checking limits),
/// - update card/account limits,
/// - list local accounts.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum RouterCmd {
    /// Full authorization flow:
    /// Router → CardActor.CheckLimit → (maybe) AccountActor.CheckAccountLimit
    AuthorizeCharge {
        account_id: u64,
        card_id: u64,
        amount: f64,
        request_id: u64,
    },

    /// Apply a charge directly, assuming it was authorized before.
    /// Router → CardActor.ApplyCharge → AccountActor.ApplyCharge
    ApplyCharge {
        account_id: u64,
        card_id: u64,
        amount: f64,
        timestamp: u64,
        op_id: u64,
        request_id: u64,
    },

    /// Update the limit for a single card.
    ///
    /// `new_limit = None` means "no limit".
    UpdateCardLimit {
        account_id: u64,
        card_id: u64,
        new_limit: Option<f64>,
        request_id: u64,
    },

    /// Update the limit for the whole account.
    ///
    /// `new_limit = None` means "no limit".
    UpdateAccountLimit {
        account_id: u64,
        new_limit: Option<f64>,
        request_id: u64,
    },

    /// Debug: print the list of active local accounts.
    ListAccounts,
}

/// Messages handled by `AccountActor`.
///
/// AccountActor does:
/// - account-wide limit checks,
/// - account-wide consumption,
/// - applying charges to the account,
/// - updating account limit.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum AccountMsg {
    /// Called by CardActor once card-level limit has passed:
    /// "please check the account-wide limit for this amount".
    CheckAccountLimit {
        amount: f64,
        request_id: u64,
    },

    /// Apply a charge directly to the account (no limit check).
    ApplyCharge {
        card_id: u64,
        amount: f64,
        request_id: u64,
        timestamp: u64,
        op_id: u64,
    },

    /// Update the account limit (must be >= already consumed).
    UpdateAccountLimit {
        new_limit: Option<f64>,
        request_id: u64,
    },
}

/// Messages handled by `CardActor`.
///
/// CardActor is responsible for:
/// - its own limit,
/// - its own consumed amount (only this card),
/// - short-circuiting failed authorizations before hitting the account.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum CardMsg {
    /// Check the card-specific limit.
    ///
    /// If the card cannot accept the charge, it will notify ActorRouter
    /// directly via RouterInternalMsg::LimitCheckResult and will NOT ask the account.
    /// If it can accept the charge, it will forward the request to
    /// the account (AccountMsg::CheckAccountLimit).
    CheckLimit {
        amount: f64,
        request_id: u64,
        account_id: u64,
        card_id: u64,
    },

    /// Card-only part of applying a charge: update its consumed amount.
    ///
    /// No limit checks happen here; those must be done beforehand via
    /// the AuthorizeCharge flow.
    ApplyCharge {
        amount: f64,
    },

    /// Update card limit (must be >= already consumed by the card).
    UpdateLimit {
        new_limit: Option<f64>,
        request_id: u64,
    },

    /// Generic debug message.
    Placeholder(String),
}

/// Internal messages from AccountActor / CardActor back to ActorRouter.
///
/// The Node never sees these directly; the router converts them into ActorEvent.
#[derive(Debug, Message)]
#[rtype(result = "()")]
pub enum RouterInternalMsg {
    LimitCheckResult {
        request_id: u64,
        allowed: bool,
        error: Option<LimitCheckError>,
    },
    ChargeApplied {
        operation: Operation,
    },
    LimitUpdated {
        scope: LimitScope,
        account_id: u64,
        card_id: Option<u64>,
        new_limit: Option<f64>,
    },
    LimitUpdateFailed {
        scope: LimitScope,
        account_id: u64,
        card_id: Option<u64>,
        request_id: u64,
        error: LimitUpdateError,
    },
    Debug(String),
}

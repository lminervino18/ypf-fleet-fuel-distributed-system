use crate::{VerifyError, operation::DatabaseSnapshot};

/// Result of a high-level operation.
///
/// Each variant corresponds to an `Operation`:
///
/// - `Charge`           → `OperationResult::Charge(ChargeResult)`
/// - `LimitAccount`     → `OperationResult::LimitAccount(LimitResult)`
/// - `LimitCard`        → `OperationResult::LimitCard(LimitResult)`
/// - `AccountQuery`     → `OperationResult::AccountQuery(AccountQueryResult)`
/// - `GetDatabase`      → `OperationResult::DatabaseSnapshot(DatabaseSnapshot)`
/// - `ReplaceDatabase`  → `OperationResult::ReplaceDatabase`
#[derive(Debug, Clone, PartialEq)]
pub enum OperationResult {
    /// Result of an `Operation::Charge`.
    Charge(ChargeResult),

    /// Result of an `Operation::LimitAccount`.
    LimitAccount(LimitResult),

    /// Result of an `Operation::LimitCard`.
    LimitCard(LimitResult),

    /// Result of an `Operation::AccountQuery` or `Bill`.
    AccountQuery(AccountQueryResult),

    /// Result of an `Operation::GetDatabase`.
    ///
    /// Contains the full snapshot (including the `snapshot.addr` field).
    DatabaseSnapshot(DatabaseSnapshot),

    /// Acknowledgement for `Operation::ReplaceDatabase`.
    ReplaceDatabase,
}

/// Specific result for a `Charge`.
///
/// Essentially: success (Ok) or a verification failure (`VerifyError`).
#[derive(Debug, Clone, PartialEq)]
pub enum ChargeResult {
    Ok,
    Failed(VerifyError),
}

/// Generic result type for limit operations
/// (`LimitAccount` / `LimitCard`).
#[derive(Debug, Clone, PartialEq)]
pub enum LimitResult {
    Ok,
    Failed(VerifyError),
}

/// Result of an account query (`AccountQuery`).
///
/// Fields:
/// - `account_id`: the queried account id.
/// - `total_spent`: total spending for the account.
/// - `per_card_spent`: list of pairs (card_id, amount) with per-card spending.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountQueryResult {
    pub account_id: u64,
    pub total_spent: f32,
    pub per_card_spent: Vec<(u64, f32)>,
}

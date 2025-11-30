use crate::{VerifyError, operation::DatabaseSnapshot};

/// Resultado de una operación de alto nivel.
///
/// Cada variante matchea con una `Operation`:
/// - `Charge`           → `OperationResult::Charge(ChargeResult)`
/// - `LimitAccount`     → `OperationResult::LimitAccount(LimitResult)`
/// - `LimitCard`        → `OperationResult::LimitCard(LimitResult)`
/// - `AccountQuery`     → `OperationResult::AccountQuery(AccountQueryResult)`
/// - `GetDatabase`      → `OperationResult::DatabaseSnapshot(DatabaseSnapshot)`
/// - `ReplaceDatabase`  → `OperationResult::ReplaceDatabase`
#[derive(Debug, Clone, PartialEq)]
pub enum OperationResult {
    /// Resultado de un `Operation::Charge`.
    Charge(ChargeResult),

    /// Resultado de un `Operation::LimitAccount`.
    LimitAccount(LimitResult),

    /// Resultado de un `Operation::LimitCard`.
    LimitCard(LimitResult),

    /// Resultado de un `Operation::AccountQuery` o `Bill`.
    AccountQuery(AccountQueryResult),

    /// Resultado de un `Operation::GetDatabase`.
    ///
    /// Contiene el snapshot completo (incluye el `addr` embebido en
    /// `snapshot.addr`).
    DatabaseSnapshot(DatabaseSnapshot),

    /// Ack de `Operation::ReplaceDatabase`.
    ReplaceDatabase,
}

/// Resultado específico de un `Charge`.
///
/// Básicamente: OK o error de verificación (`VerifyError`).
#[derive(Debug, Clone, PartialEq)]
pub enum ChargeResult {
    Ok,
    Failed(VerifyError),
}

/// Resultado genérico para operaciones de límite
/// (`LimitAccount` / `LimitCard`).
#[derive(Debug, Clone, PartialEq)]
pub enum LimitResult {
    Ok,
    Failed(VerifyError),
}

/// Resultado de una consulta de cuenta (`AccountQuery`).
///
/// - `account_id`: la cuenta consultada
/// - `total_spent`: gasto total de la cuenta
/// - `per_card_spent`: mapa (card_id → gasto de esa tarjeta)
#[derive(Debug, Clone, PartialEq)]
pub struct AccountQueryResult {
    pub account_id: u64,
    pub total_spent: f32,
    pub per_card_spent: Vec<(u64, f32)>,
}

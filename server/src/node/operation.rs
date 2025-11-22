/// High-level operation the system can perform.
///
/// Used between:
/// - Station ↔ Leader/Replica ↔ ActorRouter
/// - ActorRouter ↔ AccountActor / CardActor
///
/// These operations are purely business-level, but they also carry
/// a small piece of metadata:
///
/// - `from_offline_station`: whether the operation *originated* while
///   the station/node was in "offline" mode.
///
/// This flag allows the actor layer (and logging) to distinguish:
/// - "live" online traffic from the pumps,
/// - replay or reconciliation of operations that were accepted while
///   the node was offline.
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    /// Charge an amount to a card (affects card + account).
    ///
    /// Fields:
    /// - `account_id`: target account,
    /// - `card_id`: target card within that account,
    /// - `amount`: amount to charge,
    /// - `from_offline_station`: true if this operation corresponds to
    ///   a charge that was originally accepted while the station/node
    ///   was in offline mode.
    Charge {
        op_id: u32,
        account_id: u64,
        card_id: u64,
        amount: f64,
        from_offline_station: bool,
    },

    /// Change the account-wide limit.
    ///
    /// Fields:
    /// - `account_id`: target account,
    /// - `new_limit`: new account-wide limit (`None` means "no limit"),
    /// - `from_offline_station`: true if this limit change was originated
    ///   while the station/node was offline (for example, a replay).
    LimitAccount {
        op_id: u32,
        account_id: u64,
        new_limit: Option<f64>,
    },

    /// Change the limit for a specific card within an account.
    ///
    /// Fields:
    /// - `account_id`: account that owns the card,
    /// - `card_id`: card identifier inside that account,
    /// - `new_limit`: new per-card limit (`None` means "no limit"),
    /// - `from_offline_station`: true if this limit change comes from
    ///   an operation originally accepted in offline mode.
    LimitCard {
        op_id: u32,
        account_id: u64,
        card_id: u64,
        new_limit: Option<f64>,
    },
}

/// Scope of a limit (card or account), useful for logging
/// and error messages.
#[derive(Debug, Clone)]
pub enum LimitScope {
    Card,
    Account,
}

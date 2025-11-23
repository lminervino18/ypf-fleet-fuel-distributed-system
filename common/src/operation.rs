/// High-level operation the system can perform.
///
/// Used between:
/// - Station ↔ Leader/Replica ↔ ActorRouter
/// - ActorRouter ↔ AccountActor / CardActor
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
        account_id: u64,
        card_id: u64,
        amount: f32,
        from_offline_station: bool,
    },

    /// Change the account-wide limit.
    ///
    /// Fields:
    /// - `account_id`: target account,
    /// - `new_limit`: new account-wide limit (`None` means "no limit").
    LimitAccount {
        account_id: u64,
        new_limit: Option<f32>,
    },

    /// Change the limit for a specific card within an account.
    ///
    /// Fields:
    /// - `account_id`: account that owns the card,
    /// - `card_id`: card identifier inside that account,
    /// - `new_limit`: new per-card limit (`None` means "no limit").
    LimitCard {
        account_id: u64,
        card_id: u64,
        new_limit: Option<f32>,
    },

    /// Query current consumption for an account.
    ///
    /// The response will contain:
    /// - total consumption of the account,
    /// - a per-card breakdown (card_id -> consumption).
    AccountQuery { account_id: u64 },

    /// Perform billing for an account, optionally for a specific period.
    Bill {
        account_id: u64,
        period: Option<String>,
    },
}

/// Scope of a limit (card or account), useful for logging
/// and error messages.
#[derive(Debug, Clone)]
pub enum LimitScope {
    Card,
    Account,
}

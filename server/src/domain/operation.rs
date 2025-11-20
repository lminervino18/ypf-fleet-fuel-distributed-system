/// High-level operation the system can perform.
///
/// Se usa entre:
/// - Station ↔ Leader ↔ ActorRouter
/// - ActorRouter ↔ AccountActor / CardActor
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    /// Charge an amount to a card (affects card + account).
    Charge {
        account_id: u64,
        card_id: u64,
        amount: f64,
    },

    /// Change the account-wide limit.
    /// `new_limit = None` means "no limit".
    LimitAccount {
        account_id: u64,
        new_limit: Option<f64>,
    },

    /// Change the limit for a specific card within an account.
    /// `new_limit = None` means "no limit".
    LimitCard {
        account_id: u64,
        card_id: u64,
        new_limit: Option<f64>,
    },
}

/// Scope of a limit (card or account), useful for logging.
#[derive(Debug, Clone)]
pub enum LimitScope {
    Card,
    Account,
}

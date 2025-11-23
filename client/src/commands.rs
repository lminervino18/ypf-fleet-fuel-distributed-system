//! CLI parser for the client
use clap::Subcommand;
use common::operation::Operation;

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Limit the amounts available in the account
    LimitAccount {
        /// Amount limit (decimal)
        #[arg(long)]
        amount: f64,
    },

    /// Limit the amounts available on a specific card
    LimitCard {
        /// Card identifier
        #[arg(long)]
        card_id: String,
        /// Amount limit (decimal)
        #[arg(long)]
        amount: f64,
    },

    /// Query the account balance
    QueryAccount,

    /// Query balances of all cards for the account.
    QueryCards,

    /// Perform billing for the account.
    Bill {
        /// Optional billing period (ej. "2025-10").
        #[arg(long)]
        period: Option<String>,
    },
}

impl From<Commands> for Operation {
    fn from(cmd: Commands) -> Self {
        match cmd {
            Commands::LimitAccount { amount } => Operation::LimitAccount {
                account_id: 0, // to be filled by the client
                new_limit: Some(amount as f32),
            },
            Commands::LimitCard { card_id, amount } => Operation::LimitCard {
                account_id: 0, // to be filled by the client
                card_id: card_id.parse().unwrap_or(0),
                new_limit: Some(amount as f32),
            },
            Commands::QueryAccount => Operation::QueryAccount {
                account_id: 0, // to be filled by the client
            },
            Commands::QueryCards => Operation::QueryCards {
                account_id: 0, // to be filled by the client
            },
            Commands::Bill { period } => Operation::Bill {
                account_id: 0, // to be filled by the client
                period,
            },
        }
    }
}

impl Clone for Commands {
    fn clone(&self) -> Self {
        match self {
            Commands::LimitAccount { amount } => Commands::LimitAccount { amount: *amount },
            Commands::LimitCard { card_id, amount } => Commands::LimitCard {
                card_id: card_id.clone(),
                amount: *amount,
            },
            Commands::QueryAccount => Commands::QueryAccount,
            Commands::QueryCards => Commands::QueryCards,
            Commands::Bill { period } => Commands::Bill {
                period: period.clone(),
            },
        }
    }
}

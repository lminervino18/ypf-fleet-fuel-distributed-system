//! CLI parser for the client
use clap::Subcommand;

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

//! CLI parser for the client
use clap::Subcommand;
use common::operation::Operation;

/// Administrator command
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Limit the amounts available in the account.
    LimitAccount {
        /// Account identifier
        #[arg(long)]
        account_id: u64,
        /// Amount limit (decimal)
        #[arg(long)]
        amount: f32,
    },

    /// Limit the amounts available on a specific card.
    LimitCard {
        /// Account identifier
        #[arg(long)]
        account_id: u64,
        /// Card identifier
        #[arg(long)]
        card_id: u64,
        /// Amount limit (decimal)
        #[arg(long)]
        amount: f32,
    },

    /// Query the account balance and the balances of its cards.
    AccountQuery {
        /// Account identifier
        #[arg(long)]
        account_id: u64,
    },

    /// Perform billing for the account with a given period (optional).
    Bill {
        /// Account identifier
        #[arg(long)]
        account_id: u64,
        /// Optional billing period (ej. "2025-10").
        #[arg(long)]
        period: Option<String>,
    },
}

impl From<Command> for Operation {
    fn from(cmd: Command) -> Self {
        match cmd {
            Command::LimitAccount { account_id, amount } => Operation::LimitAccount {
                account_id,
                new_limit: if amount > 0.0 { Some(amount) } else { None },
            },
            Command::LimitCard { account_id, card_id, amount } => Operation::LimitCard {
                account_id,
                card_id,
                new_limit: if amount > 0.0 { Some(amount) } else { None },
            },
            Command::AccountQuery { account_id } => Operation::AccountQuery {
                account_id,
            },
            Command::Bill { account_id, period } => Operation::Bill {
                account_id,
                period,
            },
        }
    }
}

impl Clone for Command {
    fn clone(&self) -> Self {
        match self {
            Command::LimitAccount { account_id,amount } => Command::LimitAccount {
                account_id: *account_id,
                amount: *amount
            },
            Command::LimitCard { account_id, card_id, amount } => Command::LimitCard {
                account_id: *account_id,
                card_id: *card_id,
                amount: *amount,
            },
            Command::AccountQuery { account_id } => Command::AccountQuery {
                account_id: *account_id
            },
            Command::Bill { account_id, period } => Command::Bill {
                account_id: *account_id,
                period: period.clone(),
            },
        }
    }
}

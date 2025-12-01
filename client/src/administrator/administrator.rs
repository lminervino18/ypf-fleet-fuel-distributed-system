//! Administrator client utilities used to interact with a YPF Ruta node.
//!
//! This module provides an interactive administrator that:
//! - binds to a local socket address,
//! - maintains a Connection to a target node,
//! - converts stdin-style commands into cluster Operations,
//! - sends a request and waits for the correlated response.
//!
//! The administrator is intentionally simple: it performs one request at a time
//! and ignores unrelated messages from the node (e.g., internal replication or
//! membership events).

use common::errors::{AppResult, LimitCheckError, LimitUpdateError, VerifyError};
use common::operation::Operation;
use common::operation_result::{AccountQueryResult, ChargeResult, LimitResult, OperationResult};
use common::{Connection, Message};

use std::net::SocketAddr;

/// High-level administrator commands.
///
/// `account_id` is fixed when the admin client is started and commands that
/// target a different account will assert.
#[derive(Debug, Clone)]
pub enum AdminCommand {
    /// Set or clear an account-wide limit.
    ///
    /// If `amount` is positive, the limit is set to `amount`.
    /// If `amount` is zero or negative, the limit is cleared (no limit).
    LimitAccount {
        account_id: u64,
        amount: f32,
    },

    /// Set or clear a per-card limit.
    ///
    /// If `amount` is positive, the card limit is set to `amount`.
    /// If `amount` is zero or negative, the limit is cleared.
    LimitCard {
        account_id: u64,
        card_id: u64,
        amount: f32,
    },

    /// Query account summary (total and per-card spending).
    AccountQuery {
        account_id: u64,
    },

    /// Request a billing report for the account (optional period string).
    Bill {
        account_id: u64,
        period: Option<String>,
    },
}

/// Interactive administrator client.
///
/// Behavior:
/// - binds a local endpoint at `bind_addr`,
/// - keeps a Connection alive,
/// - maps each logical command to an `Operation` and sends it as a
///   `Message::Request` to `target_node`,
/// - waits (synchronously) for the matching `Message::Response`.
pub struct Administrator {
    bind_addr: SocketAddr,
    target_node: SocketAddr,
    account_id: u64,
    connection: Connection,
    next_req_id: u32,
}

impl Administrator {
    /// Create a new Administrator and open the underlying `Connection`.
    ///
    /// The `Connection` abstraction is the same used by node clients; the
    /// admin uses it to send requests and receive responses.
    pub async fn new(
        bind_addr: SocketAddr,
        target_node: SocketAddr,
        account_id: u64,
    ) -> AppResult<Self> {
        let connection = Connection::start(bind_addr, 16).await?;
        Ok(Self {
            bind_addr,
            target_node,
            account_id,
            connection,
            next_req_id: 1,
        })
    }

    /// Execute a single admin command.
    ///
    /// Steps:
    /// 1. Build the corresponding `Operation`.
    /// 2. Send it as `Message::Request` to the target node.
    /// 3. Wait for a `Message::Response` with the same `req_id`.
    pub async fn handle_command(&mut self, cmd: AdminCommand) -> AppResult<()> {
        let op = self.build_operation(cmd);

        let req_id = self.next_req_id;
        self.next_req_id = self.next_req_id.wrapping_add(1);

        let msg = Message::Request {
            req_id,
            op,
            addr: self.bind_addr,
        };

        self.connection.send(msg, &self.target_node).await?;

        loop {
            let msg = self.connection.recv().await?;
            match msg {
                Message::Response {
                    req_id: r,
                    op_result,
                } if r == req_id => {
                    self.handle_op_result(op_result);
                    break;
                }
                Message::Response { req_id: other, .. } => {
                    // Ignore responses for other request ids (not expected in this simple client).
                    eprintln!("[administrator] ignoring Response for unexpected req_id={other}");
                }
                other => {
                    // The admin is not part of Raft/election; ignore unrelated messages.
                    eprintln!("[administrator] ignoring unexpected message from node: {other:?}");
                }
            }
        }

        Ok(())
    }

    /// Map an `AdminCommand` to the cluster `Operation`.
    ///
    /// The admin enforces that commands only target the account it was started for.
    fn build_operation(&self, cmd: AdminCommand) -> Operation {
        match cmd {
            AdminCommand::LimitAccount { account_id, amount } => {
                // Only allow modifying the configured admin account.
                assert_eq!(
                    account_id, self.account_id,
                    "LimitAccount should only be used for the admin account_id"
                );

                Operation::LimitAccount {
                    account_id,
                    new_limit: if amount > 0.0 { Some(amount) } else { None },
                }
            }

            AdminCommand::LimitCard {
                account_id,
                card_id,
                amount,
            } => {
                assert_eq!(
                    account_id, self.account_id,
                    "LimitCard should only be used for the admin account_id"
                );

                Operation::LimitCard {
                    account_id,
                    card_id,
                    new_limit: if amount > 0.0 { Some(amount) } else { None },
                }
            }

            AdminCommand::AccountQuery { account_id } => Operation::AccountQuery { account_id },

            AdminCommand::Bill { account_id, period } => Operation::Bill { account_id, period },
        }
    }

    /// Print a concise, human-friendly summary of an `OperationResult`.
    ///
    /// The output is intentionally compact and aimed at interactive use.
    fn handle_op_result(&self, op_result: OperationResult) {
        match op_result {
            OperationResult::Charge(res) => match res {
                ChargeResult::Ok => {
                    println!("CHARGE: OK");
                }
                ChargeResult::Failed(err) => {
                    println!("CHARGE: ERROR - {}", describe_verify_error(&err));
                }
            },

            OperationResult::LimitAccount(res) => match res {
                LimitResult::Ok => {
                    println!("LIMIT_ACCOUNT: OK");
                }
                LimitResult::Failed(err) => {
                    println!("LIMIT_ACCOUNT: ERROR - {}", describe_verify_error(&err));
                }
            },

            OperationResult::LimitCard(res) => match res {
                LimitResult::Ok => {
                    println!("LIMIT_CARD: OK");
                }
                LimitResult::Failed(err) => {
                    println!("LIMIT_CARD: ERROR - {}", describe_verify_error(&err));
                }
            },

            OperationResult::AccountQuery(AccountQueryResult {
                account_id,
                total_spent,
                mut per_card_spent,
            }) => {
                // Sort by card_id for stable, readable output.
                per_card_spent.sort_by_key(|(card_id, _)| *card_id);

                println!("ACCOUNT_QUERY: OK");
                println!("  account_id={account_id}");
                println!("  total_spent={total_spent:.2}");

                if per_card_spent.is_empty() {
                    println!("  no per-card spending recorded");
                } else {
                    for (card_id, spent) in per_card_spent {
                        println!("  card_id={card_id} spent={spent:.2}");
                    }
                }
            }

            _ => {
                // This branch should be unreachable: the admin should not receive
                // low-level database operation results here.
                panic!("[FATAL] Unknown operation result");
            }
        }
    }
}

/// Return a short human-readable description for a `VerifyError`.
fn describe_verify_error(err: &VerifyError) -> &'static str {
    match err {
        VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded) => "card limit exceeded",
        VerifyError::ChargeLimit(LimitCheckError::AccountLimitExceeded) => "account limit exceeded",
        VerifyError::LimitUpdate(LimitUpdateError::BelowCurrentUsage) => {
            "new limit is below current usage"
        }
    }
}

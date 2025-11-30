use common::errors::{AppResult, LimitCheckError, LimitUpdateError, VerifyError};
use common::operation::Operation;
use common::operation_result::{AccountQueryResult, ChargeResult, LimitResult, OperationResult};
use common::{Connection, Message};

use std::net::SocketAddr;

/// High-level admin commands.
///
/// `account_id` is fixed by how you start the admin binary.
#[derive(Debug, Clone)]
pub enum AdminCommand {
    LimitAccount {
        account_id: u64,
        amount: f32,
    },
    LimitCard {
        account_id: u64,
        card_id: u64,
        amount: f32,
    },
    AccountQuery {
        account_id: u64,
    },
    Bill {
        account_id: u64,
        period: Option<String>,
    },
}

/// Interactive administrator:
/// - binds to `bind_addr`,
/// - keeps a `Connection` alive,
/// - each stdin command becomes an `Operation` sent to `target_node`.
pub struct Administrator {
    bind_addr: SocketAddr,
    target_node: SocketAddr,
    account_id: u64,
    connection: Connection,
    next_req_id: u32,
}

impl Administrator {
    /// Create the admin and open the `Connection` (same abstraction as node_client).
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

    /// Execute a single command:
    /// - build `Operation`,
    /// - send it as `Message::Request`,
    /// - wait for a matching `Message::Response`.
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
                    // Response for a different request id (unexpected for this simple admin).
                    eprintln!("[administrator] ignoring Response for unexpected req_id={other}");
                }
                other => {
                    // The admin is not part of Raft / election / etc. Ignore all that.
                    eprintln!("[administrator] ignoring unexpected message from node: {other:?}");
                }
            }
        }

        Ok(())
    }

    /// Map `AdminCommand` to the real `Operation` used by the cluster.
    fn build_operation(&self, cmd: AdminCommand) -> Operation {
        match cmd {
            AdminCommand::LimitAccount { account_id, amount } => {
                // Only allow touching the account id this admin was started with.
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

    /// Print a simple, stdout-friendly summary of the `OperationResult`.
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
                // Sort by card_id for a stable, easy-to-read output.
                per_card_spent.sort_by_key(|(card_id, _)| *card_id);

                println!("ACCOUNT_QUERY: OK");
                println!("  account_id={account_id}");
                println!("  total_spent={:.2}", total_spent);

                if per_card_spent.is_empty() {
                    println!("  no per-card spending recorded");
                } else {
                    for (card_id, spent) in per_card_spent {
                        println!("  card_id={} spent={:.2}", card_id, spent);
                    }
                }
            }

            _ => {
                // no le puede venir op result de base de datos
                panic!("[FATAL] Unknown operation result");
            }
        }
    }
}

/// Map a `VerifyError` to a short, human-readable message.
fn describe_verify_error(err: &VerifyError) -> &'static str {
    match err {
        VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded) => "card limit exceeded",
        VerifyError::ChargeLimit(LimitCheckError::AccountLimitExceeded) => "account limit exceeded",
        VerifyError::LimitUpdate(LimitUpdateError::BelowCurrentUsage) => {
            "new limit is below current usage"
        }
    }
}

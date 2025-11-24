use std::env;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::process::ExitCode;

use common::errors::{AppError, AppResult};

mod administrator;
use crate::administrator::{AdminCommand, Administrator};

#[tokio::main]
async fn main() -> ExitCode {
    match async_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[administrator] fatal error: {e:?}");
            ExitCode::FAILURE
        }
    }
}

/// Usage:
///   administrator <bind_addr> <target_node_addr> <account_id>
///
/// Examples:
///   administrator 127.0.0.1:9000 127.0.0.1:5000 100
///   administrator 127.0.0.1:9001 127.0.0.1:5001 200
///
/// Then, via stdin you can type:
///   help
///   limit-account 1000.0
///   limit-card 55 500.0
///   account-query
///   bill
///   bill 2025-10
///   exit
async fn async_main() -> AppResult<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 4 {
        eprintln!("Usage:");
        eprintln!("  {} <bind_addr> <target_node_addr> <account_id>", args[0]);
        eprintln!();
        eprintln!("Then, use stdin to send commands. Type 'help' to see available commands.");
        return Ok(());
    }

    let bind_addr: SocketAddr = args[1].parse().map_err(|e| {
        AppError::Config(format!("invalid bind_addr '{}': {e}", args[1]))
    })?;

    let target_node: SocketAddr = args[2].parse().map_err(|e| {
        AppError::Config(format!("invalid target_node_addr '{}': {e}", args[2]))
    })?;

    let account_id: u64 = args[3].parse().map_err(|e| {
        AppError::Config(format!("invalid account_id '{}': {e}", args[3]))
    })?;

    println!("[administrator] bind_addr   = {bind_addr}");
    println!("[administrator] target_node = {target_node}");
    println!("[administrator] account_id  = {account_id}");

    let mut admin = Administrator::new(bind_addr, target_node, account_id).await?;

    println!();
    println!("Interactive administrator is ready.");
    println!("Type 'help' to see commands, 'exit' to quit.");

    let stdin = io::stdin();
    loop {
        print!("admin> ");
        io::stdout().flush().ok();

        let mut line = String::new();
        if stdin.read_line(&mut line).is_err() {
            eprintln!("\n[administrator] failed to read from stdin, exiting.");
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.eq_ignore_ascii_case("exit") || line.eq_ignore_ascii_case("quit") {
            println!("[administrator] exit");
            break;
        }

        if line.eq_ignore_ascii_case("help") {
            print_help();
            continue;
        }

        match parse_line_to_command(line, account_id) {
            Ok(cmd) => {
                if let Err(e) = admin.handle_command(cmd).await {
                    eprintln!("[administrator] error executing command: {e:?}");
                }
            }
            Err(msg) => {
                eprintln!("[administrator] {msg}");
            }
        }
    }

    Ok(())
}

fn print_help() {
    println!();
    println!("Available commands:");
    println!("  help");
    println!("      Show this help message.");
    println!("  exit / quit");
    println!("      Exit the administrator.");
    println!("  limit-account <amount>");
    println!("      Update the account limit (for the fixed account_id you started with).");
    println!("      amount > 0  => sets that limit.");
    println!("      amount <= 0 => removes the limit (None).");
    println!("  limit-card <card_id> <amount>");
    println!("      Update the limit for a specific card (card_id) of the account.");
    println!("  account-query");
    println!("      Query the account state (balances, per-card spending, etc.).");
    println!("  bill [period]");
    println!("      Trigger billing for the account. Optional period, e.g. 2025-10.");
    println!();
}

fn parse_line_to_command(line: &str, account_id: u64) -> Result<AdminCommand, String> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return Err("empty line".to_string());
    }

    let cmd = parts[0];
    let args = &parts[1..];

    match cmd {
        "limit-account" => {
            if args.len() != 1 {
                return Err("Usage: limit-account <amount>".to_string());
            }
            let amount: f32 = args[0].parse().map_err(|e| {
                format!("invalid amount '{}': {e}", args[0])
            })?;
            Ok(AdminCommand::LimitAccount { account_id, amount })
        }

        "limit-card" => {
            if args.len() != 2 {
                return Err("Usage: limit-card <card_id> <amount>".to_string());
            }
            let card_id: u64 = args[0].parse().map_err(|e| {
                format!("invalid card_id '{}': {e}", args[0])
            })?;
            let amount: f32 = args[1].parse().map_err(|e| {
                format!("invalid amount '{}': {e}", args[1])
            })?;
            Ok(AdminCommand::LimitCard {
                account_id,
                card_id,
                amount,
            })
        }

        "account-query" => {
            if !args.is_empty() {
                return Err("Usage: account-query".to_string());
            }
            Ok(AdminCommand::AccountQuery { account_id })
        }

        "bill" => {
            if args.len() > 1 {
                return Err("Usage: bill [period]".to_string());
            }
            let period = args.get(0).map(|s| s.to_string());
            Ok(AdminCommand::Bill { account_id, period })
        }

        other => Err(format!(
            "unknown command '{other}'. Type 'help' to see available commands."
        )),
    }
}

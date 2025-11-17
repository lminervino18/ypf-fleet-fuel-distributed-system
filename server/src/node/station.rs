//! Station (pump) simulation module.
//!
//! This module simulates a YPF Ruta station with a set of pumps:
//! - Reads commands from stdin,
//! - For each command, selects a pump, account, card, and amount,
//! - Sends a single logical "charge request" to the Node,
//! - Receives a single result "allowed or not" from the Node,
//! - Enforces that each pump can only have **one in-flight operation** at a time.
//!
//! Command format (one per line):
//!   <pump_id> <account_id> <card_id> <amount>
//!
//! Examples:
//!   0 1 10 50.0
//!   1 2 20 150.25
//!
//! Special commands:
//!   help   -> Print usage instructions
//!   quit   -> Stop the simulator
//!   exit   -> Stop the simulator
//!
//! If a pump already has a pending operation, new commands for that pump
//! are rejected until the previous one finishes.

use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

use std::collections::HashMap;

use crate::actors::types::LimitCheckError;
use crate::errors::{AppResult, AppError};

/// Message sent from the Station (pumps) to the Node.
///
/// The Station only knows about a **single logical operation**: "charge".
/// Internally, the Node may perform multi-step work (check limit, apply charge, etc.),
/// but that is abstracted away from the Station.
#[derive(Debug)]
pub enum StationToNodeMsg {
    /// Request to perform a charge coming from a specific pump.
    ///
    /// The Node must:
    /// - Check limits (card + account),
    /// - If allowed, apply the charge,
    /// - Reply back with a `ChargeResult`.
    ChargeRequest {
        pump_id: usize,
        account_id: u64,
        card_id: u64,
        amount: f64,
        request_id: u64,
    },
}

/// Message sent from the Node to the Station (pumps).
///
/// This abstracts the whole flow as a single result:
/// "the charge was allowed and applied" or "it was rejected".
#[derive(Debug)]
pub enum NodeToStationMsg {
    /// Final result of a charge request.
    ChargeResult {
        request_id: u64,
        allowed: bool,
        /// Optional reason if `allowed == false`.
        error: Option<LimitCheckError>,
    },
}

/// Internal representation of a pending pump request.
#[derive(Debug)]
struct PumpRequest {
    pump_id: usize,
    account_id: u64,
    card_id: u64,
    amount: f64,
    request_id: u64,
}

/// Run the station simulator.
///
/// - `num_pumps`: How many pumps are available (pump IDs go from `0` to `num_pumps - 1`).
/// - `to_node_tx`: Channel Station → Node.
/// - `from_node_rx`: Channel Node → Station.
///
/// The function returns when:
/// - Stdin reaches EOF, or
/// - The user types `quit` / `exit`, or
/// - The `from_node_rx` channel is closed.
pub async fn run_station_simulator(
    num_pumps: usize,
    mut to_node_tx: mpsc::Sender<StationToNodeMsg>,
    mut from_node_rx: mpsc::Receiver<NodeToStationMsg>,
) -> AppResult<()> {
    if num_pumps == 0 {
        println!("[Station] No pumps configured, simulator will not run.");
        return Ok(());
    }

    println!(
        "[Station] Starting station simulator with {} pumps (ids: 0..={})",
        num_pumps,
        num_pumps - 1
    );
    print_help();

    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();

    // For each pump: None = idle, Some(request_id) = busy with that request
    let mut in_flight_by_pump: Vec<Option<u64>> = vec![None; num_pumps];

    // Map request_id -> PumpRequest, so we can match NodeToStation responses
    let mut requests: HashMap<u64, PumpRequest> = HashMap::new();

    // Simple monotonic counter for request IDs
    let mut next_request_id: u64 = 1;

    loop {
        let line_fut = lines.next_line();
        let event_fut = from_node_rx.recv();

        tokio::select! {
            // ========================
            // stdin (user commands)
            // ========================
            line_res = line_fut => {
                match line_res {
                    Ok(Some(line)) => {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }

                        if line.eq_ignore_ascii_case("quit") || line.eq_ignore_ascii_case("exit") {
                            println!("[Station] Received 'quit', shutting down simulator.");
                            break;
                        }

                        if line.eq_ignore_ascii_case("help") {
                            print_help();
                            continue;
                        }

                        match handle_user_command(
                            line,
                            num_pumps,
                            &mut in_flight_by_pump,
                            &mut requests,
                            &mut next_request_id,
                        ) {
                            Ok(Some(req_id)) => {
                                // Copy necessary data to local variables
                                // to avoid holding a borrow of `requests` during the await.
                                let (pump_id, account_id, card_id, amount, request_id) =
                                    match requests.get(&req_id) {
                                        Some(req) => (
                                            req.pump_id,
                                            req.account_id,
                                            req.card_id,
                                            req.amount,
                                            req.request_id,
                                        ),
                                        None => {
                                            eprintln!(
                                                "[Station][INTERNAL] request_id={} not found right after creation",
                                                req_id
                                            );
                                            // For safety, free the pump if it was marked
                                            for slot in &mut in_flight_by_pump {
                                                if *slot == Some(req_id) {
                                                    *slot = None;
                                                    break;
                                                }
                                            }
                                            continue;
                                        }
                                    };

                                println!(
                                    "[Station] pump={} -> ChargeRequest(account={}, card={}, amount={}, request_id={})",
                                    pump_id, account_id, card_id, amount, request_id
                                );

                                let msg = StationToNodeMsg::ChargeRequest {
                                    pump_id,
                                    account_id,
                                    card_id,
                                    amount,
                                    request_id,
                                };

                                if let Err(e) = to_node_tx.send(msg).await {
                                    eprintln!(
                                        "[Station][to-node][ERROR] Failed to send ChargeRequest: {}",
                                        e
                                    );
                                    // Rollback: free pump and remove request
                                    in_flight_by_pump[pump_id] = None;
                                    requests.remove(&request_id);
                                }
                            }
                            Ok(None) => {
                                // No request created
                            }
                            Err(e) => {
                                eprintln!("[Station][input][ERROR] {}", e);
                            }
                        }
                    }
                    Ok(None) => {
                        println!("[Station] Stdin closed (EOF), stopping simulator.");
                        break;
                    }
                    Err(e) => {
                        eprintln!("[Station][input][ERROR] Failed to read line: {}", e);
                        // keep looping
                    }
                }
            }

            // ========================
            // Node → Station events (responses)
            // ========================
            maybe_evt = event_fut => {
                match maybe_evt {
                    Some(evt) => {
                        handle_node_event(evt, &mut in_flight_by_pump, &mut requests);
                    }
                    None => {
                        println!("[Station] Node->Station channel closed, stopping simulator.");
                        break;
                    }
                }
            }
        }
    }

    println!("[Station] Simulator stopped.");
    Ok(())
}

/// Print usage instructions for the simulator.
fn print_help() {
    println!();
    println!("=== Station / pump simulator commands ===");
    println!("Format:");
    println!("  <pump_id> <account_id> <card_id> <amount>");
    println!();
    println!("Examples:");
    println!("  0 1 10 50.0    # pump 0, account 1, card 10, amount 50.0");
    println!("  1 2 20 125.5   # pump 1, account 2, card 20, amount 125.5");
    println!();
    println!("Special commands:");
    println!("  help           # print this help");
    println!("  quit / exit    # stop the simulator");
    println!();
}

/// Parsed user command, before assigning request_id.
#[derive(Debug)]
struct ParsedCommand {
    pump_id: usize,
    account_id: u64,
    card_id: u64,
    amount: f64,
}

/// Parse a user command line into a `ParsedCommand`.
fn parse_command(line: &str, num_pumps: usize) -> Result<ParsedCommand, String> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() != 4 {
        return Err(format!(
            "Invalid command format. Expected: <pump_id> <account_id> <card_id> <amount>, got {} tokens",
            parts.len()
        ));
    }

    let pump_id: usize = parts[0]
        .parse()
        .map_err(|_| format!("Invalid pump_id: '{}'", parts[0]))?;

    if pump_id >= num_pumps {
        return Err(format!(
            "pump_id {} out of range. Valid range: 0..={}",
            pump_id,
            num_pumps - 1
        ));
    }

    let account_id: u64 = parts[1]
        .parse()
        .map_err(|_| format!("Invalid account_id: '{}'", parts[1]))?;

    let card_id: u64 = parts[2]
        .parse()
        .map_err(|_| format!("Invalid card_id: '{}'", parts[2]))?;

    let amount: f64 = parts[3]
        .parse()
        .map_err(|_| format!("Invalid amount: '{}'", parts[3]))?;

    Ok(ParsedCommand {
        pump_id,
        account_id,
        card_id,
        amount,
    })
}

/// Handle a single user command line.
///
/// This function:
/// - Parses `<pump_id> <account_id> <card_id> <amount>`,
/// - Checks if the pump is idle,
/// - Allocates a new `request_id`,
/// - Registers the `PumpRequest` and marks the pump as busy.
///
/// It **does not** send anything to the Node; that is done by the caller
/// (so we can `await` on the channel send).
///
/// Returns:
/// - `Ok(Some(request_id))` if a new request was created,
/// - `Ok(None)` if nothing was created,
/// - `Err(String)` on validation/parsing error.
fn handle_user_command(
    line: &str,
    num_pumps: usize,
    in_flight_by_pump: &mut [Option<u64>],
    requests: &mut HashMap<u64, PumpRequest>,
    next_request_id: &mut u64,
) -> Result<Option<u64>, String> {
    let parsed = parse_command(line, num_pumps)?;

    // Check if the pump is already busy
    if let Some(existing_req) = in_flight_by_pump[parsed.pump_id] {
        return Err(format!(
            "Pump {} is busy with request_id {}. Wait for the result before sending another command.",
            parsed.pump_id, existing_req
        ));
    }

    // Allocate a new request_id
    let request_id = *next_request_id;
    *next_request_id += 1;

    // Register the request
    requests.insert(
        request_id,
        PumpRequest {
            pump_id: parsed.pump_id,
            account_id: parsed.account_id,
            card_id: parsed.card_id,
            amount: parsed.amount,
            request_id,
        },
    );

    // Mark pump as busy
    in_flight_by_pump[parsed.pump_id] = Some(request_id);

    Ok(Some(request_id))
}

/// Handle a `NodeToStationMsg` coming back from the Node.
///
/// This is a **single-step** result:
/// - The Node already did the authorization and (if allowed) the charge.
/// - So we only need to log and free the pump.
fn handle_node_event(
    msg: NodeToStationMsg,
    in_flight_by_pump: &mut [Option<u64>],
    requests: &mut HashMap<u64, PumpRequest>,
) {
    match msg {
        NodeToStationMsg::ChargeResult {
            request_id,
            allowed,
            error,
        } => {
            // Find original request
            let Some(req) = requests.remove(&request_id) else {
                println!(
                    "[Station][WARN] Received ChargeResult for unknown request_id={}",
                    request_id
                );
                return;
            };

            let pump_id = req.pump_id;

            if allowed {
                println!(
                    "[Station] pump={} -> CHARGE OK (request_id={}, account={}, card={}, amount={})",
                    pump_id, request_id, req.account_id, req.card_id, req.amount
                );
            } else {
                println!(
                    "[Station] pump={} -> CHARGE DENIED (request_id={}, account={}, card={}, amount={}, error={:?})",
                    pump_id, request_id, req.account_id, req.card_id, req.amount, error
                );
            }

            // Free pump
            in_flight_by_pump[pump_id] = None;
        }
    }
}


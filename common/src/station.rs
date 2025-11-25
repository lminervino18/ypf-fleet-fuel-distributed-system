use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use rand::Rng;
use std::collections::HashMap;

use crate::errors::{AppError, AppResult, VerifyError};

/// Message sent from the Station (pumps) to the Node.
///
/// Same abstraction as before:
/// - it only knows about the logical "charge" operation,
/// - plus connect / disconnect commands.
#[derive(Debug)]
pub enum StationToNodeMsg {
    /// Charge request from a pump.
    ChargeRequest {
        account_id: u64,
        card_id: u64,
        amount: f32,
        request_id: u32,
    },

    /// Ask the node to switch to OFFLINE mode.
    DisconnectNode,

    /// Ask the node to switch back to ONLINE mode.
    ConnectNode,
}

/// Message sent from the Node back to the Station.
#[derive(Debug)]
pub enum NodeToStationMsg {
    /// Final result of a charge request.
    ChargeResult {
        request_id: u32,
        allowed: bool,
        /// Optional error when `allowed == false`.
        error: Option<VerifyError>,
    },

    /// Informational / debug messages from the node.
    Debug(String),
}

/// Internal representation of a pending pump request.
#[derive(Debug)]
struct PumpRequest {
    pump_id: usize,
    account_id: u64,
    card_id: u64,
    amount: f32,
    request_id: u32,
}

/// Parsed user command (from stdin) before assigning `request_id`.
#[derive(Debug)]
struct ParsedCommand {
    pump_id: usize,
    account_id: u64,
    card_id: u64,
    amount: f32,
}

/// High-level wrapper that encapsulates the station simulator.
///
/// - Internally runs an async task that calls `run_station_simulator`.
/// - Exposes only `start()`, `recv()` and `send()` to the outside.
pub struct Station {
    /// Messages coming from the station simulator to the node (the node calls `recv()`).
    from_station_rx: mpsc::Receiver<StationToNodeMsg>,
    /// Messages that the node sends back to the station (the node calls `send()`).
    to_station_tx: mpsc::Sender<NodeToStationMsg>,
    /// Handle of the background task. It is kept alive while `Station` is in scope.
    _task: JoinHandle<()>,
}

impl Station {
    /// Start the simulated station with `num_pumps` pumps.
    ///
    /// Spawns a task that:
    /// - reads stdin,
    /// - emits `StationToNodeMsg` for the node,
    /// - consumes `NodeToStationMsg` coming from the node.
    pub async fn start(num_pumps: usize) -> AppResult<Self> {
        // Channel Station -> Node (from simulator task to the caller).
        let (station_to_node_tx, station_to_node_rx) = mpsc::channel::<StationToNodeMsg>(128);

        // Channel Node -> Station (from caller to the simulator task).
        let (node_to_station_tx, node_to_station_rx) = mpsc::channel::<NodeToStationMsg>(128);

        // Spawn the task that runs the real simulator.
        let task = tokio::spawn(async move {
            let _ = run_station_simulator(num_pumps, station_to_node_tx, node_to_station_rx).await;
        });

        Ok(Self {
            from_station_rx: station_to_node_rx,
            to_station_tx: node_to_station_tx,
            _task: task,
        })
    }

    /// Receive a logical message from the station simulator.
    ///
    /// The node typically uses it inside a `select!`:
    ///
    pub async fn recv(&mut self) -> Option<StationToNodeMsg> {
        self.from_station_rx.recv().await
    }

    /// Send a result or debug message to the station simulator.
    ///
    /// If the channel is closed, returns `AppError::Channel`.
    pub async fn send(&self, msg: NodeToStationMsg) -> AppResult<()> {
        self.to_station_tx
            .send(msg)
            .await
            .map_err(|e| AppError::Channel {
                details: format!("failed to send NodeToStationMsg: {e}"),
            })
    }
}

impl Drop for Station {
    /// When `Station` goes out of scope, abort the background task.
    fn drop(&mut self) {
        self._task.abort();
    }
}

/// Main function of the station simulator.
///
/// - Reads commands from stdin.
/// - Sends `StationToNodeMsg` to the node.
/// - Receives `NodeToStationMsg` from the node.
///
/// It is private to the module: only used by the task created in `Station::start`.
async fn run_station_simulator(
    num_pumps: usize,
    to_node_tx: mpsc::Sender<StationToNodeMsg>,
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
    // print_help();

    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();

    // For each pump: None = idle, Some(request_id) = busy.
    let mut in_flight_by_pump: Vec<Option<u32>> = vec![None; num_pumps];

    // request_id -> PumpRequest
    let mut requests: HashMap<u32, PumpRequest> = HashMap::new();

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

                        if line.eq_ignore_ascii_case("disconnect") {
                            let msg = StationToNodeMsg::DisconnectNode;
                            if let Err(e) = to_node_tx.send(msg).await {
                                eprintln!(
                                    "[Station][to-node][ERROR] Failed to send DisconnectNode: {e}"
                                );
                            }
                            continue;
                        }

                        if line.eq_ignore_ascii_case("connect") {
                            let msg = StationToNodeMsg::ConnectNode;
                            if let Err(e) = to_node_tx.send(msg).await {
                                eprintln!(
                                    "[Station][to-node][ERROR] Failed to send ConnectNode: {e}"
                                );
                            }
                            continue;
                        }

                        match handle_user_command(
                            line,
                            num_pumps,
                            &mut in_flight_by_pump,
                            &mut requests,
                        ) {
                            Ok(Some(req_id)) => {
                                // Copy local data so we don't hold a borrow on `requests` during `await`.
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
                                            println!(
                                                "[Station][INTERNAL] request_id={req_id} not found right after creation"
                                            );
                                            // For safety, free the pump if it was marked.
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
                                    "[Station] pump={pump_id} -> ChargeRequest(account={account_id}, card={card_id}, amount={amount}, request_id={request_id})"
                                );

                                let msg = StationToNodeMsg::ChargeRequest {
                                    account_id,
                                    card_id,
                                    amount,
                                    request_id,
                                };

                                if let Err(e) = to_node_tx.send(msg).await {
                                    eprintln!(
                                        "[Station][to-node][ERROR] Failed to send ChargeRequest: {e}"
                                    );
                                    // Rollback: free pump and remove request.
                                    in_flight_by_pump[pump_id] = None;
                                    requests.remove(&request_id);
                                }
                            }
                            Ok(None) => {
                                // No new request created (e.g. parse error already logged).
                            }
                            Err(e) => {
                                eprintln!("[Station][input][ERROR] {e}");
                            }
                        }
                    }
                    Ok(None) => {
                        println!("[Station] Stdin closed (EOF), stopping simulator.");
                        break;
                    }
                    Err(e) => {
                        eprintln!("[Station][input][ERROR] Failed to read line: {e}");
                        // Continue loop.
                    }
                }
            }

            // ========================
            // Node → Station (responses)
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

/// Print usage instructions for the station simulator.
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
    println!("  disconnect     # ask the node to switch into OFFLINE mode");
    println!("  connect        # ask the node to switch back into ONLINE mode");
    println!();
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

    let amount: f32 = parts[3]
        .parse()
        .map_err(|_| format!("Invalid amount: '{}'", parts[3]))?;

    Ok(ParsedCommand {
        pump_id,
        account_id,
        card_id,
        amount,
    })
}

/// Handle a single user command:
/// - parse,
/// - check that the pump is idle,
/// - allocate a `request_id`,
/// - register the `PumpRequest` and mark the pump as busy.
///
/// Returns:
/// - `Ok(Some(request_id))` if a new request was created,
/// - `Ok(None)` if nothing was created,
/// - `Err(String)` if there was a parse/validation error.
fn handle_user_command(
    line: &str,
    num_pumps: usize,
    in_flight_by_pump: &mut [Option<u32>],
    requests: &mut HashMap<u32, PumpRequest>,
) -> Result<Option<u32>, String> {
    let parsed = parse_command(line, num_pumps)?;

    // Check if the pump is already busy.
    if let Some(existing_req) = in_flight_by_pump[parsed.pump_id] {
        return Err(format!(
            "Pump {} is busy with request_id {}. Wait for the result before sending another command.",
            parsed.pump_id, existing_req
        ));
    }

        // Allocate a new random request_id (non-zero, not currently in use)
    let mut rng = rand::thread_rng();
    let request_id = loop {
        let candidate: u32 = rng.r#gen();
        if candidate != 0
            && !requests.contains_key(&candidate)
            && !in_flight_by_pump.iter().any(|&slot| slot == Some(candidate))
        {
            break candidate;
        }
    };


    // Register the request.
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

    // Mark pump as busy.
    in_flight_by_pump[parsed.pump_id] = Some(request_id);

    Ok(Some(request_id))
}

/// Handle a `NodeToStationMsg` coming from the node.
fn handle_node_event(
    msg: NodeToStationMsg,
    in_flight_by_pump: &mut [Option<u32>],
    requests: &mut HashMap<u32, PumpRequest>,
) {
    match msg {
        NodeToStationMsg::ChargeResult {
            request_id,
            allowed,
            error,
        } => {
            // Find original request.
            let Some(req) = requests.remove(&request_id) else {
                println!(
                    "[Station][WARN] Received ChargeResult for unknown request_id={request_id}"
                );
                return;
            };

            let pump_id = req.pump_id;

            if allowed {
                println!(
                    "[Station][RESULT] pump={} -> CHARGE OK (request_id={}, account={}, card={}, amount={})",
                    pump_id, request_id, req.account_id, req.card_id, req.amount
                );
            } else {
                println!(
                    "[Station][RESULT] pump={} -> CHARGE DENIED (request_id={}, account={}, card={}, amount={}, error={:?})",
                    pump_id, request_id, req.account_id, req.card_id, req.amount, error
                );
            }

            // Free pump.
            in_flight_by_pump[pump_id] = None;
        }

        NodeToStationMsg::Debug(text) => {
            println!("[Station][DEBUG] {text}");
        }
    }
}

// client/src/node_client/main.rs

use std::env;
use std::net::SocketAddr;
use std::process::ExitCode;

use common::errors::{AppError, AppResult};
use common::{Connection, Station};

// 👇 ESTA es la forma correcta dentro del mismo bin:
mod node_client;
use crate::node_client::NodeClient;
// (también podrías hacer `use node_client::NodeClient;`)

#[tokio::main]
async fn main() -> ExitCode {
    match async_main().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[node_client] fatal error: {e:?}");
            ExitCode::FAILURE
        }
    }
}

/// Parse CLI, create Connection + Station, and start the thin forwarding node.
///
/// Usage:
///   node_client <bind_addr> <max_connections> <num_pumps> <known_node_1> [<known_node_2> ...]
///
/// Example:
///   node_client 127.0.0.1:6000 128 4 127.0.0.1:5000 127.0.0.1:5001
async fn async_main() -> AppResult<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 5 {
        eprintln!("Usage:");
        eprintln!(
            "  {} <bind_addr> <max_connections> <num_pumps> <known_node_1> [<known_node_2> ...]",
            args[0]
        );
        return Ok(());
    }

    // Local address used by this forwarding node (just like leader/replica).
    let bind_addr: SocketAddr = args[1]
        .parse()
        .map_err(|e| AppError::Config(
            format!("invalid bind_addr '{}': {e}", args[1]),
        ))?;

    let max_connections: usize = args[2]
        .parse()
        .map_err(|e| AppError::Config(
            format!("invalid max_connections '{}': {e}", args[2]),
        ))?;

    let num_pumps: usize = args[3]
        .parse()
        .map_err(|e| AppError::Config(
            format!("invalid num_pumps '{}': {e}", args[3]),
        ))?;

    let mut known_nodes = Vec::new();
    for raw in &args[4..] {
        let addr: SocketAddr = raw
            .parse()
            .map_err(|e| AppError::Config(
                format!("invalid known_node address '{}': {e}", raw),
            ))?;
        known_nodes.push(addr);
    }

    if known_nodes.is_empty() {
        eprintln!("[node_client] No known nodes provided; nothing to forward to.");
        return Ok(());
    }

    println!("[node_client] starting with:");
    println!("  bind_addr       = {}", bind_addr);
    println!("  max_connections = {}", max_connections);
    println!("  num_pumps       = {}", num_pumps);
    println!("  known_nodes     = {:?}", known_nodes);

    // Shared TCP abstraction (same type as leader/replica).
    let connection = Connection::start(bind_addr, max_connections).await?;

    // Local pump simulator (same Station abstraction used in the server).
    let station = Station::start(num_pumps).await?;

    // Thin client node: pumps on stdin, forwards to known cluster nodes.
    let client = NodeClient::new(bind_addr, known_nodes);

    client.run(connection, station).await
}

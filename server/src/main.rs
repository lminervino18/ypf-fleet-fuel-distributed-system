use clap::Parser;
use std::net::SocketAddr;
mod connection;
mod node;
mod actors;
mod errors;

use node::{Node, NodeRole};
use errors::{AppError, AppResult};

/// YPF Ruta — Distributed server binary
///
/// Launch a single node participating in the distributed YPF Ruta system.
/// Nodes may run in one of three roles: leader, replica or station.
/// Use `--help` to view command-line options and examples.
#[derive(Parser, Debug)]
#[command(name = "ypf_server")]
#[command(about = "Distributed node for the YPF Ruta system")]
struct Args {
    /// Node role to run. Allowed values: "leader", "replica", "station".
    #[arg(long)]
    role: String,

    /// IP address for the node to bind or use as its source address.
    #[arg(long)]
    ip: String,

    /// TCP port to bind/connect on (1-65535).
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    /// Coordinates as two floats: latitude then longitude.
    #[arg(long, num_args = 2, value_names = ["LAT", "LON"])]
    coords: Vec<f64>,

    /// Leader address (required for replica and station roles).
    #[arg(long)]
    leader: Option<String>,

    /// Zero or more replica addresses (only used when running as leader).
    #[arg(long, num_args = 0.., value_name = "IP:PORT")]
    replicas: Vec<String>,

    /// Maximum number of simultaneous TCP connections (leader only).
    #[arg(long, default_value_t = 16)]
    max_conns: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Run the application and exit with a non-zero code on fatal error.
    if let Err(e) = run().await {
        eprintln!("[FATAL] {e:?}");
        std::process::exit(1);
    }
    Ok(())
}

async fn run() -> AppResult<()> {
    let args = Args::parse();

    // Parse and validate coordinates: expect exactly two values (lat, lon).
    if args.coords.len() != 2 {
        return Err(AppError::InvalidCoords { lat: f64::NAN, lon: f64::NAN });
    }
    let coords = (args.coords[0], args.coords[1]);

    // Interpret the role argument into the NodeRole enum.
    let role = match args.role.as_str() {
        "leader" => NodeRole::Leader,
        "replica" => NodeRole::Replica,
        "station" => NodeRole::Station,
        other => return Err(AppError::Config(format!("Invalid role: {}", other))),
    };

    // Validate and parse the optional leader address when required.
    let leader_addr = match (&role, &args.leader) {
        (NodeRole::Leader, _) => None,
        (_, Some(addr_str)) => {
            let addr: SocketAddr = addr_str.parse().map_err(|_| AppError::Config(format!(
                "Invalid leader address '{}'", addr_str
            )))?;
            Some(addr)
        }
        (_, None) => {
            // Replica and station roles must specify --leader.
            return Err(AppError::Config(
                "Missing --leader argument for replica/station".to_string(),
            ));
        }
    };

    // For leaders, parse replica addresses and warn on invalid entries.
    let replica_addrs: Vec<SocketAddr> = if role == NodeRole::Leader {
        if args.replicas.is_empty() {
            println!("[WARN] Leader started with no replicas");
        }
        args.replicas
            .iter()
            .filter_map(|s| match s.parse::<SocketAddr>() {
                Ok(addr) => Some(addr),
                Err(_) => {
                    eprintln!("[WARN] Skipping invalid replica address '{}'", s);
                    None
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    println!(
        "[BOOT] role={:?} ip={}:{} coords=({:.5}, {:.5}) max_conns={} replicas={:?} leader={:?}",
        role, args.ip, args.port, coords.0, coords.1, args.max_conns, replica_addrs, leader_addr
    );

    // Construct the Node with the validated configuration and run it.
    let node = Node::new(
        role,
        args.ip.clone(),
        args.port,
        coords,
        leader_addr,
        replica_addrs,
        args.max_conns,
    )
    .await?;

    node.run().await
}

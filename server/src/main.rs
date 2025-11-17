mod actors;
mod errors;
mod node;

use clap::{Parser, Subcommand};
use errors::AppResult;
use node::{Leader, Replica};
use std::{net::SocketAddr, str::FromStr};

/// YPF Ruta — Distributed server binary
///
/// Launch a single node participating in the distributed YPF Ruta system.
/// Nodes may run in one of three roles: leader, replica or station.
///
/// For example, to run as a replica use:
///
/// `cargo run --bin server -- replica --leader-addr="127.0.0.1:12346"`
///
/// Use `--help` to view command-line options and examples.
#[derive(Parser, Debug)]
#[command(
    name = "ypf_server",
    about = "Distributed node for the YPF Ruta system",
    subcommand_required = false,
    subcommand = "station"
)]
struct Args {
    /// Address for the node to bind.
    #[arg(long, value_name = "IP:PORT", default_value_t = SocketAddr::from_str("127.0.0.1:12345").unwrap())]
    address: SocketAddr,
    /// Coordinates as two floats: latitude then longitude.
    #[arg(long, num_args=2, value_names = ["LAT", "LON"], default_values_t = vec![0f64, 0f64])]
    coords: Vec<f64>,
    /// Node role to run. Allowed values: "leader", "replica", "station".
    #[command(subcommand)]
    role: RoleArgs,
}

#[derive(Subcommand, Debug)]
enum RoleArgs {
    Station {
        /// Leader address.
        #[arg(long, value_name = "IP:PORT")]
        leader_addr: SocketAddr,
    },
    Leader {
        /// Zero or more replica addresses.
        #[arg(long, num_args = 0.., value_name = "IP:PORT")]
        replicas: Vec<SocketAddr>,
        /// Maximum number of simultaneous TCP connections.
        #[arg(long, default_value_t = 16)]
        max_conns: usize,
    },
    Replica {
        /// Leader address.
        #[arg(long, value_name = "IP:PORT")]
        leader_addr: SocketAddr,
        /// Zero or more replica addresses.
        #[arg(long, num_args = 0.., value_name = "IP:PORT")]
        other_replicas: Vec<SocketAddr>,
        /// Maximum number of simultaneous TCP connections.
        #[arg(long, default_value_t = 16)]
        max_conns: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Err(e) = run().await {
        eprintln!("[FATAL] {e:?}");
        std::process::exit(1);
    }

    Ok(())
}

async fn run() -> AppResult<()> {
    let args = Args::parse();
    let coords = (args.coords[0], args.coords[1]);
    match args.role {
        RoleArgs::Leader {
            replicas,
            max_conns,
        } => Leader::start(args.address, coords, replicas, max_conns).await,
        RoleArgs::Replica {
            leader_addr,
            other_replicas,
            max_conns,
        } => Replica::start(args.address, coords, leader_addr, other_replicas, max_conns).await,
        RoleArgs::Station { leader_addr } => {
            // TODO: implement default "station" node behaviour
            todo!()
        }
    }
}

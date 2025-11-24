mod errors;
mod node;

use clap::{Parser, Subcommand};
use errors::AppResult;
use node::{Leader, Replica};
use std::{net::SocketAddr, process::ExitCode, str::FromStr};

/// YPF Ruta — Distributed server binary
///
/// Launch a node participating in the distributed YPF Ruta system.
/// A node can run as:
///   • leader
///   • replica
///   • station  (future dedicated mode)
///
/// Example (leader):
///     cargo run --bin server -- leader --pumps 4
///
/// Example (replica):
///     cargo run --bin server -- replica --leader-addr 127.0.0.1:5000 --pumps 4
#[derive(Parser, Debug)]
#[command(
    name = "ypf_server",
    about = "Distributed node for the YPF Ruta system"
)]
struct Args {
    /// Address for the node to bind.
    #[arg(
        long,
        value_name = "IP:PORT",
        default_value_t = SocketAddr::from_str("127.0.0.1:12345").unwrap()
    )]
    address: SocketAddr,

    /// Coordinates (lat lon).
    #[arg(
        long,
        num_args = 2,
        value_names = ["LAT", "LON"],
        default_values_t = vec![0.0, 0.0]
    )]
    coords: Vec<f64>,

    /// Number of pumps used by the internal station simulator.
    #[arg(long, default_value_t = 4)]
    pumps: usize,

    /// Node role to run.
    #[command(subcommand)]
    role: RoleArgs,
}

#[derive(Subcommand, Debug)]
enum RoleArgs {
    /// Standalone station client (future feature).
    Station {
        #[arg(long, value_name = "IP:PORT")]
        leader_addr: SocketAddr,
    },

    /// Leader node.
    ///
    /// Initial cluster membership will be discovered dynamically
    /// via Join / ClusterView messages; the CLI no longer needs replica
    /// addresses here.
    Leader {
        #[arg(long, default_value_t = 16)]
        max_conns: usize,
    },

    /// Replica node.
    ///
    /// The replica only needs to know the leader address; the rest of the
    /// cluster topology will be learned from the leader via Join / ClusterView.
    Replica {
        #[arg(long, value_name = "IP:PORT")]
        leader_addr: SocketAddr,

        #[arg(long, default_value_t = 16)]
        max_conns: usize,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(_) => ExitCode::SUCCESS,
        Err(_e) => {
            //println!("[FATAL] {e:?}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> AppResult<()> {
    let args = Args::parse();

    let coords = (args.coords[0], args.coords[1]);
    let pumps = args.pumps;

    match args.role {
        RoleArgs::Leader { max_conns } => {
            // Leader::start(address, coords, max_conns, pumps)
            // Membership is initially seeded with self; other nodes
            // will be discovered via Join / ClusterView messages.
            Leader::start(args.address, coords, max_conns, pumps).await?;
        }

        RoleArgs::Replica {
            leader_addr,
            max_conns,
        } => {
            // Replica::start(address, leader_addr, coords, max_conns, pumps)
            // The replica only knows the leader at startup; any
            // other peers will be learned from the leader.
            Replica::start(args.address, leader_addr, coords, max_conns, pumps).await?;
        }

        RoleArgs::Station { leader_addr: _ } => {
            // When the dedicated Station mode is implemented, this branch will call:
            //
            // Station::start(args.address, coords, leader_addr, pumps).await?;
            //
            todo!("Station node mode is not implemented yet");
        }
    }

    Ok(())
}

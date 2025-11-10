use clap::{Parser, builder::TypedValueParser};
use std::net::SocketAddr;
use anyhow::Context; // still used at the very top-level for user-facing context

mod connection;
mod node;
mod actors;
mod errors; // ✅ new error module

use node::{NeighborNode, Node};
use errors::{AppError, AppResult};

/// YPF Distributed Node
///
/// Example:
///   server --ip 127.0.0.1 --port 9001 --coords -34.60 -58.38 \
///          --neighbors 127.0.0.1:9002 -34.61 -58.39 127.0.0.1:9003 -34.62 -58.40
#[derive(Parser, Debug)]
#[command(name = "ypf_server")]
#[command(about = "Base distributed node for the YPF Ruta system")]
struct Args {
    /// IP address to listen on (e.g. 0.0.0.0 or 127.0.0.1)
    #[arg(long)]
    ip: String,

    /// Port to bind (e.g. 9001)
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    /// Coordinates (latitude, longitude). Exactly two values are required.
    ///
    /// Example: --coords -34.60 -58.38
    #[arg(long, num_args = 2, value_names = ["LAT", "LON"])]
    coords: Vec<f64>,

    /// Neighbor nodes as repeating triplets: <ip:port> <lat> <lon>
    ///
    /// Example:
    ///   --neighbors 127.0.0.1:9002 -34.61 -58.39 127.0.0.1:9003 -34.62 -58.40
    ///
    /// Tip: if your shell confuses negatives as flags, you can use:
    ///   --neighbors -- 127.0.0.1:9002 "-34.61" "-58.39" 127.0.0.1:9003 "-34.62" "-58.40"
    #[arg(long, num_args = 0.., allow_hyphen_values = true, value_name = "IP:PORT/LAT/LON")]
    neighbors: Vec<String>,

    /// Maximum simultaneous TCP connections
    #[arg(long, default_value_t = 16)]
    max_conns: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Err(e) = run().await {
        eprintln!("[FATAL] {e:?}");
        std::process::exit(1);
    }
    Ok(())
}

/// Core execution logic (returns structured `AppError`)
async fn run() -> AppResult<()> {
    let args = Args::parse();

    // --- Parse coordinates ---
    let lat = *args
        .coords
        .get(0)
        .ok_or_else(|| AppError::InvalidCoords {
            lat: f64::NAN,
            lon: f64::NAN,
        })?;

    let lon = *args
        .coords
        .get(1)
        .ok_or_else(|| AppError::InvalidCoords {
            lat,
            lon: f64::NAN,
        })?;

    let coords = (lat, lon);

    // --- Parse neighbors with strict validation ---
    let neighbors = parse_neighbors(&args.neighbors)?;

    println!(
        "[BOOT] ip={}:{} coords=({:.5}, {:.5}) max_conns={} neighbors={}",
        args.ip,
        args.port,
        coords.0,
        coords.1,
        args.max_conns,
        neighbors.len()
    );

    let node = Node::new(
        args.ip.clone(),
        args.port,
        coords,
        neighbors,
        args.max_conns,
    )
    .await
    .map_err(|e| AppError::ActorSystem {
        details: format!("failed to construct Node: {e}"),
    })?;

    println!(
        "[INFO] Node listening on {}:{} (max {} connections)",
        args.ip, args.port, args.max_conns
    );
    println!("[INFO] Press Ctrl+C to quit");

    node.run().await.map_err(|e| AppError::ActorSystem {
        details: format!("node run failed: {e}"),
    })?;

    Ok(())
}

/// Parses the neighbors vector into structured triplets (ip:port, lat, lon)
fn parse_neighbors(raw: &[String]) -> AppResult<Vec<NeighborNode>> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }

    if raw.len() % 3 != 0 {
        return Err(AppError::InvalidNeighbor {
            details: format!(
                "Invalid neighbor list: must be triplets <ip:port> <lat> <lon>. Got {} items.",
                raw.len()
            ),
        });
    }

    let mut out = Vec::with_capacity(raw.len() / 3);
    let mut i = 0;

    while i < raw.len() {
        let addr_str = &raw[i];
        let lat_str = &raw[i + 1];
        let lon_str = &raw[i + 2];

        // Validate ip:port
        let _addr_parsed: SocketAddr = addr_str
            .parse()
            .map_err(|_| AppError::InvalidNeighbor {
                details: format!("invalid neighbor address '{}'", addr_str),
            })?;

        // Validate lat/lon
        let lat: f64 = lat_str
            .parse()
            .map_err(|_| AppError::InvalidNeighbor {
                details: format!("invalid latitude '{}'", lat_str),
            })?;

        let lon: f64 = lon_str
            .parse()
            .map_err(|_| AppError::InvalidNeighbor {
                details: format!("invalid longitude '{}'", lon_str),
            })?;

        out.push(NeighborNode {
            addr: addr_str.clone(),
            coords: (lat, lon),
        });

        i += 3;
    }

    Ok(out)
}

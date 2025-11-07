use anyhow::{Context, Result};
use clap::{Parser, builder::TypedValueParser};
use std::net::SocketAddr;

mod connection;
mod node;
mod actors;
use node::{NeighborNode, Node};

/// YPF Distributed Node
///
/// Examples:
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
    /// Examples:
    ///   --neighbors 127.0.0.1:9002 -34.61 -58.39 127.0.0.1:9003 -34.62 -58.40
    ///
    /// Tip: si tu shell confunde los negativos como flags, podés usar:
    ///   --neighbors -- 127.0.0.1:9002 "-34.61" "-58.39" 127.0.0.1:9003 "-34.62" "-58.40"
    #[arg(long, num_args = 0.., allow_hyphen_values = true, value_name = "IP:PORT/LAT/LON")]
    neighbors: Vec<String>,

    /// Maximum simultaneous TCP connections
    #[arg(long, default_value_t = 16)]
    max_conns: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Parse self coordinates
    let coords = (
        *args
            .coords
            .get(0)
            .context("coords requires 2 values: <lat> <lon> (missing lat)")?,
        *args
            .coords
            .get(1)
            .context("coords requires 2 values: <lat> <lon> (missing lon)")?,
    );

    // Parse neighbors: triplets (ip:port, lat, lon) con validación estricta
    let neighbors = parse_neighbors(&args.neighbors)
        .context("failed to parse --neighbors triplets <ip:port> <lat> <lon>")?;

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
    .context("failed to construct Node")?;

    println!(
        "[INFO] Node listening on {}:{} (max {} connections)",
        args.ip, args.port, args.max_conns
    );
    println!("[INFO] Press Ctrl+C to quit");

    node.run().await.context("node run failed")?;
    Ok(())
}

/// Parses the neighbors vector into structured data (triplets).
fn parse_neighbors(raw: &[String]) -> Result<Vec<NeighborNode>> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }

    if raw.len() % 3 != 0 {
        anyhow::bail!(
            "Invalid neighbor list: must be triplets of <ip:port> <lat> <lon>. Got {} items.",
            raw.len()
        );
    }

    let mut out = Vec::with_capacity(raw.len() / 3);
    let mut i = 0;

    while i < raw.len() {
        let addr_str = &raw[i];
        let lat_str = &raw[i + 1];
        let lon_str = &raw[i + 2];

        // valida ip:port
        let _addr_parsed: SocketAddr = addr_str
            .parse()
            .with_context(|| format!("invalid neighbor address '{}'", addr_str))?;

        // valida lat/lon
        let lat: f64 = lat_str
            .parse()
            .with_context(|| format!("invalid latitude '{}'", lat_str))?;
        let lon: f64 = lon_str
            .parse()
            .with_context(|| format!("invalid longitude '{}'", lon_str))?;

        out.push(NeighborNode {
            addr: addr_str.clone(),
            coords: (lat, lon),
        });

        i += 3;
    }

    Ok(out)
}

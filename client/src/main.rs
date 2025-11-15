mod cli;
use crate::cli::Cli;
use clap::Parser;
use std::net::SocketAddr;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    println!("[CLIENT] parsed command: {:?}", cli.command);
    println!("[CLIENT] parsed server: {:?}", cli.server);
    // Parse server address
    let addr: SocketAddr = cli
        .server
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid server address '{}': {}", cli.server, e))?;

    println!("[CLIENT] connecting to server at {}", addr);

    Ok(())
}

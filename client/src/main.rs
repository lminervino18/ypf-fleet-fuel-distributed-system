mod cli;
use crate::cli::Cli;
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    println!("[CLIENT] parsed command: {:?}", cli.command);
    println!("[CLIENT] parsed server: {:?}", cli.server);
    // TODO: send parsed command to the server (network / connection manager)

    Ok(())
}

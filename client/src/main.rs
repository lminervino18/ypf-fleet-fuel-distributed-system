mod cli;
use crate::cli::Cli;
use clap::Parser;

// Parse-only client entrypoint. TODO: implement sending logic to server.
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    println!("[CLIENT] parsed command: {:?}", cli.command);
    // TODO: send parsed command to the server (network / connection manager)

    Ok(())
}

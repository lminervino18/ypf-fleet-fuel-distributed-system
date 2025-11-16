mod cli;
mod commands;
use crate::cli::Cli;

fn main() -> anyhow::Result<()> {
    let cli = Cli::new();
    let mut tcp_stream = cli.connect()?;
    cli.send_command(&mut tcp_stream)?;

    Ok(())
}

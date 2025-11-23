use std::io::{Read, Write};
use clap::Parser;
use crate::commands::Commands;
use common::operation::Operation;

/// YPF client
#[derive(Parser, Debug)]
#[command(name = "ypf_client")]
#[command(about = "YPF Ruta client - admin CLI")]
pub struct Cli {
    /// Server address
    #[arg(long, default_value = "127.0.0.1:9000")]
    pub server: String,

    #[command(subcommand)]
    pub command: Commands,
}

impl Cli {
    /// Create a new CLI instance from command line arguments
    pub fn new() -> Self {
        let cli = Cli::parse();
        println!("[CLIENT] parsed command: {:?}", cli.command);
        println!("[CLIENT] parsed server: {:?}", cli.server);
        cli
    }

    /// Connect to the server specified in the CLI arguments
    pub fn connect(&self) -> anyhow::Result<std::net::TcpStream> {
        let addr: std::net::SocketAddr = self
            .server
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid server address '{}': {e}", self.server))?;

        println!("[CLIENT] connecting to server at {addr}");
        // Connect to server
        let tcp_stream = std::net::TcpStream::connect(addr)
            .map_err(|e| anyhow::anyhow!("failed to connect to server at {addr}: {e}"))?;
        println!("[CLIENT] successfully connected to server at {addr}");
        Ok(tcp_stream)
    }

    pub fn send_command(&self, tcp_stream: &mut std::net::TcpStream) -> anyhow::Result<()> {
        println!("[CLIENT] sending command: {:?}", self.command);
        let op = Operation::from(self.command.clone());
        let op_srl: Vec<u8> = op.into();
        println!("[CLIENT] serialized operation: {op_srl:?}");
        tcp_stream
            .write_all(&op_srl)
            .map_err(|e| anyhow::anyhow!("failed to send operation to server: {e}"))?;

        let mut buf = [0u8; 1024];
        tcp_stream.read(&mut buf)
            .map_err(|e| anyhow::anyhow!("failed to read response from server: {e}"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing_default_server() {
        let args = vec!["ypf_client", "query-account"];
        let cli = Cli::parse_from(args);
        assert_eq!(cli.server, "127.0.0.1:9000");
    }

    #[test]
    fn test_cli_parsing_limit_account() {
        let args = vec![
            "ypf_client",
            "--server",
            "127.0.0.1:9000",
            "limit-account",
            "--amount",
            "1000.0",
        ];
        let cli = Cli::parse_from(args);
        assert_eq!(cli.server, "127.0.0.1:9000");
        match cli.command {
            Commands::LimitAccount { amount } => {
                assert_eq!(amount, 1000.0);
            }
            _ => panic!("Expected LimitAccount command"),
        }
    }

    #[test]
    fn test_cli_parsing_limit_card() {
        let args = vec![
            "ypf_client",
            "--server",
            "127.0.0.1:9000",
            "limit-card",
            "--card-id",
            "card",
            "--amount",
            "500.0",
        ];
        let cli = Cli::parse_from(args);
        assert_eq!(cli.server, "127.0.0.1:9000");
        match cli.command {
            Commands::LimitCard { card_id, amount } => {
                assert_eq!(card_id, "card");
                assert_eq!(amount, 500.0);
            }
            _ => panic!("Expected LimitCard command"),
        }
    }

    #[test]
    fn test_cli_parsing_query_account() {
        let args = vec!["ypf_client", "query-account"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::QueryAccount => {}
            _ => panic!("Expected QueryAccount command"),
        }
    }

    #[test]
    fn test_cli_parsing_query_cards() {
        let args = vec!["ypf_client", "query-cards"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::QueryCards => {}
            _ => panic!("Expected QueryCards command"),
        }
    }

    #[test]
    fn test_cli_parsing_bill() {
        let args = vec!["ypf_client", "bill", "--period", "2025-10"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Bill { period } => {
                assert_eq!(period, Some("2025-10".to_string()));
            }
            _ => panic!("Expected Bill command"),
        }
    }

    #[test]
    fn test_cli_parsing_bill_no_period() {
        let args = vec!["ypf_client", "bill"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Commands::Bill { period } => {
                assert_eq!(period, None);
            }
            _ => panic!("Expected Bill command"),
        }
    }

    #[test]
    fn test_cli_parsing_invalid_command() {
        let args = vec!["ypf_client", "invalid-command"];
        let result = Cli::try_parse_from(args);
        assert!(result.is_err());
    }
}

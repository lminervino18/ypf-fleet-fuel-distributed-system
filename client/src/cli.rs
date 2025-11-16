use clap::Parser;
use crate::commands::Commands;

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

    pub fn send_command(&self, _tcp_stream: &mut std::net::TcpStream) -> anyhow::Result<()> {
        println!("[CLIENT] sending command: {:?}", self.command);
        // Here you would serialize the command and send it over the tcp_stream
        Ok(())
    }
}

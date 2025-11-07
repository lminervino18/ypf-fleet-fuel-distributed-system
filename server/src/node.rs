use crate::connection::{ConnEvent, ConnectionManager, ManagerCmd};
use rand::seq::SliceRandom;
use rand::{rngs::StdRng, SeedableRng};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::sleep;

/// Represents a neighboring node in the distributed network
#[derive(Debug, Clone)]
pub struct NeighborNode {
    pub addr: String,
    pub coords: (f64, f64),
}

/// Represents a distributed node in the system
pub struct Node {
    pub id: u64,
    pub ip: String,
    pub port: u16,
    pub coords: (f64, f64),
    pub neighbors: Vec<NeighborNode>,
    pub max_conns: usize,

    /// Channel to notify the ConnectionManager about new accepted connections
    pub conn_tx: mpsc::Sender<ConnEvent>,

    /// Channel to command the ConnectionManager (e.g., send messages)
    pub manager_cmd: mpsc::Sender<ManagerCmd>,
}

impl Node {
    /// Initialize a new node and spawn its connection manager task
    pub async fn new(
        ip: String,
        port: u16,
        coords: (f64, f64),
        neighbors: Vec<NeighborNode>,
        max_conns: usize,
    ) -> anyhow::Result<Self> {
        // Events channel (Node -> Manager)
        let (conn_tx, conn_rx) = mpsc::channel::<ConnEvent>(64);

        // Commands channel (Node -> Manager)
        let (cmd_tx, cmd_rx) = mpsc::channel::<ManagerCmd>(64);

        // Manager's internal event echo channel (for readers to notify closures)
        let (mgr_evt_tx, _mgr_evt_rx_dummy) = mpsc::channel::<ConnEvent>(1);
        // We actually use `conn_rx` for incoming events from Node; `mgr_evt_tx` is cloned inside manager readers.

        // Build manager
        let manager = ConnectionManager::new(max_conns, conn_rx, mgr_evt_tx.clone(), cmd_rx, cmd_tx.clone());

        // Spawn manager task (takes ownership safely)
        tokio::spawn(manager.run());

        Ok(Self {
            id: rand::random::<u64>(),
            ip,
            port,
            coords,
            neighbors,
            max_conns,
            conn_tx,
            manager_cmd: cmd_tx,
        })
    }

    /// Run the node: start listener and periodic random sender
    pub async fn run(self) -> anyhow::Result<()> {
        let addr: SocketAddr = format!("{}:{}", self.ip, self.port).parse()?;
        let listener = TcpListener::bind(addr).await?;

        println!("[INFO] Node {} listening on {}:{}", self.id, self.ip, self.port);
        println!("[INFO] Known neighbors: {:?}", self.neighbors);

        // 🔹 Listener task: accept inbound TCP connections and forward them to the manager
        let listener_task = {
            let conn_tx = self.conn_tx.clone();
            tokio::spawn(async move {
                let mut next_id = 0u64;
                println!("[LISTENER] TCP listener task started");

                loop {
                    match listener.accept().await {
                        Ok((stream, peer)) => {
                            next_id += 1;
                            if let Err(e) = conn_tx.send(ConnEvent::NewConn(next_id, stream, peer)).await {
                                eprintln!("[ERROR] Could not register incoming connection: {}", e);
                            }
                        }
                        Err(e) => eprintln!("[ERROR] Failed to accept connection: {}", e),
                    }
                }
            })
        };

        // Sender task: periodically send a message to a random neighbor via the manager
        let sender_task = {
            let manager_cmd = self.manager_cmd.clone();
            let neighbors = self.neighbors.clone();
            let id = self.id;

            tokio::spawn(async move {
                let mut rng = StdRng::from_entropy();

                loop {
                    if let Some(n) = neighbors.choose(&mut rng) {
                        if let Ok(addr) = n.addr.parse::<SocketAddr>() {
                            let msg = format!("hello from node {}\n", id);
                            if let Err(e) = manager_cmd.send(ManagerCmd::SendTo(addr, msg)).await {
                                eprintln!("[WARN] Failed to queue SendTo to {}: {}", addr, e);
                            } else {
                                println!("[SEND-REQ] Node {} requested send to {}", id, addr);
                            }
                        }
                    }

                    sleep(Duration::from_secs(5)).await;
                }
            })
        };

        tokio::try_join!(listener_task, sender_task)?;
        Ok(())
    }
}

use crate::connection::{ConnectionManager, InboundEvent, ManagerCmd};
use rand::seq::SliceRandom;
use rand::{rngs::StdRng, SeedableRng};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

/// A neighboring node in the distributed network
#[derive(Debug, Clone)]
pub struct NeighborNode {
    pub addr: String,
    pub coords: (f64, f64),
}

/// A distributed node that owns business logic and delegates all TCP to ConnectionManager
pub struct Node {
    pub id: u64,
    pub ip: String,
    pub port: u16,
    pub coords: (f64, f64),
    pub neighbors: Vec<NeighborNode>,
    pub max_conns: usize,

    /// Commands → ConnectionManager (send, shutdown, etc.)
    pub manager_cmd: mpsc::Sender<ManagerCmd>,
    /// Inbound events ← ConnectionManager (payloads, closures)
    pub inbound_rx: mpsc::Receiver<InboundEvent>,
}

impl Node {
    /// Build the node and start the TCP service inside ConnectionManager
    pub async fn new(
        ip: String,
        port: u16,
        coords: (f64, f64),
        neighbors: Vec<NeighborNode>,
        max_conns: usize,
    ) -> anyhow::Result<Self> {
        let listen_addr: SocketAddr = format!("{}:{}", ip, port).parse()?;
        let (manager_cmd, inbound_rx) = ConnectionManager::start(listen_addr, max_conns);

        Ok(Self {
            id: rand::random::<u64>(),
            ip,
            port,
            coords,
            neighbors,
            max_conns,
            manager_cmd,
            inbound_rx,
        })
    }

    /// Run the node:
    /// 1) consume inbound messages from ConnectionManager
    /// 2) periodically send a message to a random neighbor (demo)
    pub async fn run(mut self) -> anyhow::Result<()> {
        println!(
            "[INFO] Node listening on {}:{} (max {} connections)",
            self.ip, self.port, self.max_conns
        );
        println!(
            "[INFO] Node {} booted. Known neighbors: {:?}",
            self.id, self.neighbors
        );
        println!("[INFO] Press Ctrl+C to quit");

        // Task A: inbound consumer (messages from TCP → Node)
        let mut inbound = self.inbound_rx;
        let inbound_task = tokio::spawn(async move {
            while let Some(evt) = inbound.recv().await {
                match evt {
                    InboundEvent::Received { peer, payload } => {
                        // Here you can deserialize and dispatch to Gateway/actors later.
                        println!("[INBOUND from {}] {}", peer, payload);
                    }
                    InboundEvent::ConnClosed { peer } => {
                        println!("[INBOUND] Connection closed by {}", peer);
                    }
                }
            }
            println!("[INBOUND] Channel closed; inbound consumer exiting");
        });

        // Task B: demo sender — ping a random neighbor every 5s
        let neighbors = self.neighbors.clone();
        let cmd_tx = self.manager_cmd.clone();
        let id = self.id;
        let sender_task = tokio::spawn(async move {
            let mut rng = StdRng::from_entropy();

            loop {
                if let Some(n) = neighbors.choose(&mut rng) {
                    if let Ok(addr) = n.addr.parse::<SocketAddr>() {
                        let msg = format!("hello from node {}\n", id);
                        if let Err(e) = cmd_tx.send(ManagerCmd::SendTo(addr, msg)).await {
                            eprintln!("[WARN] Failed to queue SendTo to {}: {}", addr, e);
                        } else {
                            println!("[SEND-REQ] Node {} requested send to {}", id, addr);
                        }
                    }
                }

                sleep(Duration::from_secs(5)).await;
            }
        });

        tokio::try_join!(inbound_task, sender_task)?;
        Ok(())
    }
}

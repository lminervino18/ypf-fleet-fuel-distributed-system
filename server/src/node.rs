use crate::connection::{ConnectionManager, InboundEvent, ManagerCmd};
use crate::actors::actor_router::ActorRouter;
use crate::actors::types::{RouterCmd, ActorAddr, ActorMsg};
use crate::errors::{AppError, AppResult};

use actix::prelude::*;
use std::future::pending;
use std::net::SocketAddr;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, Duration};

/// Represents a neighboring node in the distributed network.
#[derive(Debug, Clone)]
pub struct NeighborNode {
    pub addr: String,
    pub coords: (f64, f64),
}

/// The main distributed node instance.
/// Each node:
///  - Manages its TCP connections through a ConnectionManager.
///  - Hosts an Actix ActorRouter for local actors and message routing.
///  - Communicates with neighboring nodes over TCP.
pub struct Node {
    pub id: u64,
    pub ip: String,
    pub port: u16,
    pub coords: (f64, f64),
    pub neighbors: Vec<NeighborNode>,
    pub max_conns: usize,

    pub manager_cmd: mpsc::Sender<ManagerCmd>,
    pub inbound_rx: mpsc::Receiver<InboundEvent>,
    pub router: Addr<ActorRouter>,
}

impl Node {
    /// Creates a new distributed node.
    /// 
    /// Responsibilities:
    /// - Parse and bind the TCP listener.
    /// - Spawn the `ConnectionManager`.
    /// - Launch a dedicated Actix system for the ActorRouter.
    pub async fn new(
        ip: String,
        port: u16,
        coords: (f64, f64),
        neighbors: Vec<NeighborNode>,
        max_conns: usize,
    ) -> AppResult<Self> {
        // Parse the socket address (validated by AppError)
        let listen_addr: SocketAddr = format!("{}:{}", ip, port)
            .parse()
            .map_err(|e| AppError::AddrParse { source: e })?;

        // Start TCP connection manager
        let (manager_cmd, inbound_rx) = ConnectionManager::start(listen_addr, max_conns);

        // Assign a random node ID (useful for tracing messages)
        let id = rand::random::<u64>();

        // Prepare communication channel to get router address
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();
        let manager_cmd_for_router = manager_cmd.clone();
        let self_node_id = id;

        // Spawn Actix System in a separate OS thread
        std::thread::spawn(move || {
            let sys = actix::System::new();

            sys.block_on(async move {
                // Initialize router actor inside Actix
                let router = ActorRouter::new(self_node_id, manager_cmd_for_router).start();

                // Send back the router's address to this thread
                if let Err(_) = router_tx.send(router.clone()) {
                    eprintln!("[ERROR] Failed to send router address back to Node");
                }

                // Bootstrap example: create a local subscriber actor
                router.do_send(RouterCmd::CreateSubscriber { card_id: 1 });

                // Keep Actix system alive
                pending::<()>().await;
            });
        });

        // Wait for the router to be ready
        let router = router_rx.await.map_err(|e| AppError::ActorSystem {
            details: format!("failed to receive router address: {e}"),
        })?;

        Ok(Self {
            id,
            ip,
            port,
            coords,
            neighbors,
            max_conns,
            manager_cmd,
            inbound_rx,
            router,
        })
    }

    /// Runs the node:
    /// 1. Starts listening for inbound messages from the ConnectionManager.
    /// 2. Forwards messages to the ActorRouter.
    /// 3. Periodically sends test pings to neighbors.
    pub async fn run(mut self) -> AppResult<()> {
        println!(
            "[INFO] Node {} listening on {}:{} (max {} connections)",
            self.id, self.ip, self.port, self.max_conns
        );
        println!("[INFO] Known neighbors: {:?}", self.neighbors);

        // ---- Task 1: Handle inbound events (TCP → Node) ----
        let mut inbound = self.inbound_rx;
        let router_addr = self.router.clone();
        tokio::spawn(async move {
            while let Some(evt) = inbound.recv().await {
                match evt {
                    InboundEvent::Received { peer, payload } => {
                        println!("[INBOUND from {}] {}", peer, payload);
                        router_addr.do_send(RouterCmd::NetIn {
                            from: peer,
                            bytes: payload.into_bytes(),
                        });
                    }
                    InboundEvent::ConnClosed { peer } => {
                        println!("[INFO] Connection closed by {}", peer);
                    }
                }
            }
        });

        // ---- Task 2: Send a test message to the first neighbor ----
        let cmd_tx = self.manager_cmd.clone();
        let neighbors = self.neighbors.clone();
        let my_id = self.id;
        tokio::spawn(async move {
            sleep(Duration::from_secs(2)).await;
            if let Some(first) = neighbors.first() {
                match first.addr.parse::<SocketAddr>() {
                    Ok(addr) => {
                        let msg = format!("MSG {} PING\n", my_id);
                        println!("[TEST] Node {} sending ping to {}", my_id, addr);
                        if let Err(e) = cmd_tx.send(ManagerCmd::SendTo(addr, msg)).await {
                            eprintln!("[WARN] Failed to send ping: {e}");
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "[WARN] Invalid neighbor address '{}': {}",
                            first.addr, e
                        );
                    }
                }
            }
        });

        // ---- Keep node alive ----
        Ok(pending::<()>().await)
    }
}

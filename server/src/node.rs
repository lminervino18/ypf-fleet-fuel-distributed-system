use crate::connection::{ConnectionManager, InboundEvent, ManagerCmd};
use crate::actors::actor_router::ActorRouter;
use crate::actors::types::RouterCmd;

use actix::prelude::*;
use std::future::pending;
use std::net::SocketAddr;
use tokio::sync::{mpsc, oneshot};

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

    /// ActorRouter address (lives in its own Actix System thread)
    pub router: Addr<ActorRouter>,
}

impl Node {
    /// Build the node, start TCP (ConnectionManager), and start the ActorRouter system.
    pub async fn new(
        ip: String,
        port: u16,
        coords: (f64, f64),
        neighbors: Vec<NeighborNode>,
        max_conns: usize,
    ) -> anyhow::Result<Self> {
        let listen_addr: SocketAddr = format!("{}:{}", ip, port).parse()?;
        let (manager_cmd, inbound_rx) = ConnectionManager::start(listen_addr, max_conns);

        let id = rand::random::<u64>();

        // Spawn an Actix System on its own thread and get back the router address via oneshot.
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();
        let manager_cmd_for_router = manager_cmd.clone();
        let self_node_id = id;

        std::thread::spawn(move || {
            let sys = actix::System::new();

            sys.block_on(async move {
                // Start the router actor
                let router = ActorRouter::new(self_node_id, manager_cmd_for_router).start();

                // Send the Addr back to the creator
                let _ = router_tx.send(router.clone());

                // Bootstrap example: create a local Subscriber (change card_id as needed)
                router.do_send(RouterCmd::CreateSubscriber { card_id: 1 });

                // Keep the Actix System alive for the process lifetime
                pending::<()>().await;
            });
        });

        // Wait for the router address and store it on the Node
        let router = router_rx.await?;

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

    /// Run the node:
    /// 1) Consume inbound messages from the ConnectionManager (later forward to router)
    pub async fn run(mut self) -> anyhow::Result<()> {
        println!(
            "[INFO] Node listening on {}:{} (max {} connections)",
            self.ip, self.port, self.max_conns
        );
        println!("[INFO] Node {} booted. Known neighbors: {:?}", self.id, self.neighbors);
        println!("[INFO] ActorRouter is ready at: {:?}", self.router);
        println!("[INFO] Press Ctrl+C to quit");

        // Inbound consumer: TCP → Node (when wire format is ready, forward to router)
        let mut inbound = self.inbound_rx;
        let router_addr = self.router.clone();
        let _inbound_task = tokio::spawn(async move {
            while let Some(evt) = inbound.recv().await {
                match evt {
                    InboundEvent::Received { peer, payload } => {
                        // TODO: parse bytes → ProtocolMessage and forward to router:
                        // router_addr.do_send(RouterCmd::NetIn { from: peer, bytes: payload.into_bytes() });
                        println!("[INBOUND from {}] {}", peer, payload);
                    }
                    InboundEvent::ConnClosed { peer } => {
                        println!("[INBOUND] Connection closed by {}", peer);
                    }
                }
            }
            println!("[INBOUND] Channel closed; inbound consumer exiting");
        });

        // Keep Node alive (replace with graceful shutdown if needed)
        Ok(pending::<()>().await)
    }
}

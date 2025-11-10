use crate::connection::{ConnectionManager, InboundEvent, ManagerCmd};
use crate::actors::actor_router::ActorRouter;
use crate::actors::types::{RouterCmd, ActorAddr, ActorMsg};

use actix::prelude::*;
use std::future::pending;
use std::net::SocketAddr;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, Duration};

#[derive(Debug, Clone)]
pub struct NeighborNode {
    pub addr: String,
    pub coords: (f64, f64),
}

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

        // Spawn Actix System and create router
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();
        let manager_cmd_for_router = manager_cmd.clone();
        let self_node_id = id;

        std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(self_node_id, manager_cmd_for_router).start();
                let _ = router_tx.send(router.clone());
                router.do_send(RouterCmd::CreateSubscriber { card_id: 1 });
                pending::<()>().await;
            });
        });

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

    pub async fn run(mut self) -> anyhow::Result<()> {
        println!(
            "[INFO] Node {} listening on {}:{} ({} conns)",
            self.id, self.ip, self.port, self.max_conns
        );
        println!("[INFO] Neighbors: {:?}", self.neighbors);

        // --- Task 1: inbound consumer ---
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

        // --- Task 2: small test after boot ---
        let cmd_tx = self.manager_cmd.clone();
        let neighbors = self.neighbors.clone();
        let my_id = self.id;
        tokio::spawn(async move {
            sleep(Duration::from_secs(2)).await;
            if let Some(first) = neighbors.first() {
                let addr: SocketAddr = first.addr.parse().unwrap();
                let msg = format!("MSG {} PING\n", my_id);
                println!("[TEST] Node {} sending ping to {}", my_id, addr);
                let _ = cmd_tx.send(ManagerCmd::SendTo(addr, msg)).await;
            }
        });

        Ok(pending::<()>().await)
    }
}

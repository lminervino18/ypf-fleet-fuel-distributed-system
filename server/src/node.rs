use crate::connection::{ConnectionManager, InboundEvent, ManagerCmd};
use crate::actors::actor_router::ActorRouter;
use crate::actors::types::RouterCmd;
use crate::errors::{AppError, AppResult};

use actix::prelude::*;
use std::future::pending;
use std::net::SocketAddr;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, Duration};

/// Role of a node in the YPF Ruta distributed system.
///
/// - Leader: coordinates replicas and accepts connections from stations/replicas.
/// - Replica: connects to a leader and acts as a passive replica.
/// - Station: edge node that represents a physical station and forwards requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeRole {
    Leader,
    Replica,
    Station,
}

/// Distributed node participating in the YPF Ruta system.
///
/// A Node encapsulates:
/// - networking (TCP) via the ConnectionManager,
/// - a local ActorRouter (Actix) for application logic,
/// - role-specific behaviour (leader, replica, station).
///
/// The Node is responsible for wiring background tasks that bridge the async
/// Tokio runtime (network IO) and the Actix actor system (application logic).
pub struct Node {
    pub id: u64,
    pub role: NodeRole,
    pub ip: String,
    pub port: u16,
    pub coords: (f64, f64),
    pub max_conns: usize,

    // Networking
    pub leader_addr: Option<SocketAddr>,
    pub replicas: Vec<SocketAddr>,

    // Communication primitives
    pub manager_cmd: mpsc::Sender<ManagerCmd>,
    pub inbound_rx: mpsc::Receiver<InboundEvent>,
    pub router: Addr<ActorRouter>,
}

impl Node {
    /// Create and initialize a new Node.
    ///
    /// This sets up the TCP listener/manager, starts the Actix actor router in
    /// a separate thread, and returns a Node configured for the requested role.
    /// Validation errors are returned as AppError variants.
    pub async fn new(
        role: NodeRole,
        ip: String,
        port: u16,
        coords: (f64, f64),
        leader_addr: Option<SocketAddr>,
        replicas: Vec<SocketAddr>,
        max_conns: usize,
    ) -> AppResult<Self> {
        let listen_addr: SocketAddr = format!("{}:{}", ip, port)
            .parse()
            .map_err(|e| AppError::AddrParse { source: e })?;

        // --- Initialize TCP layer ---
        let (manager_cmd, inbound_rx) = ConnectionManager::start(listen_addr, max_conns);

        let id = rand::random::<u64>();

        // Convertir réplicas a String para pasarlas al router
        let replicas_str: Vec<String> = replicas.iter().map(|addr| addr.to_string()).collect();

        // --- Spawn ActorRouter in Actix system ---
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();
        let manager_cmd_for_router = manager_cmd.clone();

        std::thread::spawn(move || {
            let sys = actix::System::new();

            sys.block_on(async move {
                let router = ActorRouter::new(manager_cmd_for_router, replicas_str).start();

                if router_tx.send(router.clone()).is_err() {
                    eprintln!("[ERROR] Failed to send router address to Node");
                }

                pending::<()>().await;
            });
        });

        let router = router_rx.await.map_err(|e| AppError::ActorSystem {
            details: format!("failed to receive router address: {e}"),
        })?;

        Ok(Self {
            id,
            role,
            ip,
            port,
            coords,
            max_conns,
            leader_addr,
            replicas,
            manager_cmd,
            inbound_rx,
            router,
        })
    }

    /// Run the node's main logic.
    ///
    /// This method:
    /// 1. Spawns a Tokio task to forward inbound network events to the ActorRouter.
    /// 2. Performs role-specific initialization (leader/replica/station).
    /// 3. Awaits forever (the node process is long-lived).
    ///
    /// Returns an AppResult that only errors on fatal misconfiguration or startup failure.
    pub async fn run(self) -> AppResult<()> {
        println!(
            "[INFO] Node {} ({:?}) listening on {}:{} (max {} conns)",
            self.id, self.role, self.ip, self.port, self.max_conns
        );

        // ---- 1. Handle inbound messages from ConnectionManager ----
        // Spawn a background task that receives inbound network events from
        // the ConnectionManager and forwards them to the ActorRouter actor.
        let mut inbound = self.inbound_rx;
        let router_addr = self.router.clone();
        let manager_cmd = self.manager_cmd.clone();
        let role = self.role.clone();
        let replicas = self.replicas.clone();
        let leader_addr = self.leader_addr;
        let node_id = self.id;

        tokio::spawn(async move {
            while let Some(evt) = inbound.recv().await {
                match evt {
                    InboundEvent::Received { peer, payload } => {
                        // Forward raw bytes to the ActorRouter for higher-level processing.
                        println!("[INBOUND from {}] {}", peer, payload);
                        router_addr.do_send(RouterCmd::NetIn {
                            from: peer,
                            bytes: payload.into_bytes(),
                        });
                    }
                    InboundEvent::ConnClosed { peer } => {
                        // Connection teardown notification; application may react or ignore.
                        println!("[INFO] Connection closed by {}", peer);
                    }
                }
            }
        });

        // ---- 2. Role-specific initialization ----
        // Delegate to small helpers which encapsulate role behaviour and side-effects.
        match role {
            NodeRole::Leader => {
                Self::run_as_leader(manager_cmd, replicas, node_id).await?;
            }
            NodeRole::Replica => {
                if let Some(leader) = leader_addr {
                    Self::run_as_replica(manager_cmd, leader, node_id).await?;
                } else {
                    return Err(AppError::Config("Replica missing leader address".into()));
                }
            }
            NodeRole::Station => {
                if let Some(leader) = leader_addr {
                    Self::run_as_station(manager_cmd, leader, node_id).await?;
                } else {
                    return Err(AppError::Config("Station missing leader address".into()));
                }
            }
        }

        // Keep the node alive indefinitely; callers expect this process to run forever.
        Ok(pending::<()>().await)
    }

    // ==========================
    // ==== Role Behaviors ======
    // ==========================

    /// Leader behaviour: periodically check replica reachability and perform leader-only setup.
    ///
    /// Currently sends a simple ping message to each configured replica after a short delay.
    async fn run_as_leader(
        manager_cmd: mpsc::Sender<ManagerCmd>,
        replicas: Vec<SocketAddr>,
        node_id: u64,
    ) -> AppResult<()> {
        println!(
            "[ROLE] Leader initialized with {} replicas: {:?}",
            replicas.len(),
            replicas
        );

        // Fire-and-forget task: attempt to contact replicas to verify connectivity.
        tokio::spawn(async move {
            sleep(Duration::from_secs(3)).await;
            for replica in replicas {
                let msg = format!("MSG {} PING_FROM_LEADER\n", node_id);
                println!("[LEADER] Pinging replica {}", replica);
                if let Err(e) = manager_cmd.send(ManagerCmd::SendTo(replica, msg)).await {
                    eprintln!("[WARN] Failed to ping replica {}: {}", replica, e);
                }
            }
        });

        Ok(())
    }

    /// Replica startup behaviour: announce presence to the leader.
    async fn run_as_replica(
        manager_cmd: mpsc::Sender<ManagerCmd>,
        leader: SocketAddr,
        node_id: u64,
    ) -> AppResult<()> {
        println!("[ROLE] Replica connecting to leader at {}", leader);

        // Send a single hello message; errors map to channel errors.
        let msg = format!("MSG {} HELLO_FROM_REPLICA\n", node_id);
        manager_cmd
            .send(ManagerCmd::SendTo(leader, msg))
            .await
            .map_err(|e| AppError::Channel {
                details: e.to_string(),
            })?;

        Ok(())
    }

    /// Station startup behaviour: announce presence to the leader.
    async fn run_as_station(
        manager_cmd: mpsc::Sender<ManagerCmd>,
        leader: SocketAddr,
        node_id: u64,
    ) -> AppResult<()> {
        println!("[ROLE] Station connecting to leader at {}", leader);

        // Send a single hello message; errors map to channel errors.
        let msg = format!("MSG {} HELLO_FROM_STATION\n", node_id);
        manager_cmd
            .send(ManagerCmd::SendTo(leader, msg))
            .await
            .map_err(|e| AppError::Channel {
                details: e.to_string(),
            })?;

        Ok(())
    }
}

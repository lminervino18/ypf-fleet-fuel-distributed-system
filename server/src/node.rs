use crate::actors::actor_router::ActorRouter;
use crate::actors::types::RouterCmd;
use crate::connection_manager::{ConnectionManager, InboundEvent, ManagerCmd};
use crate::errors::{AppError, AppResult};
use crate::node_message::NodeMessage;
use crate::operation::Operation;

use actix::prelude::*;
use std::collections::HashMap;
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

    // Operations
    pub operations: HashMap<u8, Operation>,

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
            operations: HashMap::new(),
            manager_cmd,
            inbound_rx,
            router,
        })
    }

    pub async fn handle_accept(&mut self, op: Operation) {
        let Some(leader_addr) = self.leader_addr else {
            // TODO: si soy líder ignoro, pero si no es un error, y por lo tanto debería inciciar
            // elección
            return;
        };

        if self.operations.contains_key(&op.id) {
            // ya me lo habían mandado
        }

        let id = op.id;
        self.operations.insert(id, op.clone());

        self.manager_cmd
            .send(ManagerCmd::SendTo(
                leader_addr,
                NodeMessage::Learn(op).into(),
            ))
            .await
            .unwrap()
    }

    pub fn handle_learn(&mut self, op: Operation) {
        // para el líder
        // a las réplicas no les llega
        // cuando al líder le llegan todos los learn (o la mayoría) aplica la operación
    }

    pub async fn handle_msg(&mut self, msg: NodeMessage) {
        match msg {
            NodeMessage::Accept(op) => {
                self.handle_accept(op).await;
            }
            NodeMessage::Learn(op) => {
                self.handle_learn(op);
            }
            _ => {}
        }
    }

    pub async fn run(&mut self) -> AppResult<()> {
        println!(
            "[INFO] Node {} ({:?}) listening on {}:{} (max {} conns)",
            self.id, self.role, self.ip, self.port, self.max_conns
        );

        while let Some(evt) = self.inbound_rx.recv().await {
            match evt {
                InboundEvent::Received { peer, payload } => {
                    // TODO: handle this error
                    self.handle_msg(payload.try_into().unwrap()).await;
                }
                InboundEvent::ConnClosed { peer } => {
                    println!("[INFO] Connection closed by {}", peer);
                }
            }
        }

        pending::<()>().await;
        Ok(())
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

        Ok(())
    }

    /// Replica startup behaviour: announce presence to the leader.
    async fn run_as_replica(
        manager_cmd: mpsc::Sender<ManagerCmd>,
        leader: SocketAddr,
        node_id: u64,
    ) -> AppResult<()> {
        println!("[ROLE] Replica connecting to leader at {}", leader);

        Ok(())
    }

    /// Station startup behaviour: announce presence to the leader.
    async fn run_as_station(
        manager_cmd: mpsc::Sender<ManagerCmd>,
        leader: SocketAddr,
        node_id: u64,
    ) -> AppResult<()> {
        println!("[ROLE] Station connecting to leader at {}", leader);

        Ok(())
    }
}

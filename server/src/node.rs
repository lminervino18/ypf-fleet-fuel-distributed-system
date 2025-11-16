use crate::actors::actor_router::ActorRouter;
use crate::connection_manager::{ConnectionManager, InboundEvent, ManagerCmd};
use crate::errors::{AppError, AppResult};
use crate::node_message::NodeMessage;
use crate::operation::Operation;

use actix::prelude::*;
use std::collections::HashMap;
use std::future::pending;
use std::net::SocketAddr;
use tokio::sync::{mpsc, oneshot};

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
    pub leader_addr: Option<SocketAddr>,
    pub replicas: Vec<SocketAddr>,
    pub operations: HashMap<u8, Operation>,
    pub connection_tx: mpsc::Sender<ManagerCmd>,
    pub connection_rx: mpsc::Receiver<InboundEvent>,
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

        let (connection_tx, connection_rx) = ConnectionManager::start(listen_addr, max_conns);
        let id = rand::random::<u64>();
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();
        std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new().start();
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
            connection_tx,
            connection_rx,
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
        self.connection_tx
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

    pub async fn handle_node_msg(&mut self, msg: NodeMessage) {
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

        while let Some(evt) = self.connection_rx.recv().await {
            match evt {
                InboundEvent::Received { peer, payload } => {
                    self.handle_node_msg(payload.try_into().unwrap()).await;
                }
                InboundEvent::ConnClosed { peer } => {
                    println!("[INFO] Connection closed by {}", peer);
                }
            }
        }

        pending::<()>().await;
        Ok(())
    }
}

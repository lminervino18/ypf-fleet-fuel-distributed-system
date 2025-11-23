use super::{
    actors::{actor_router::ActorRouter, ActorEvent, RouterCmd},
    election::bully::Bully,
    node::Node,
    utils::get_id_given_addr,
};
use crate::errors::{AppError, AppResult};
use actix::{Actor, Addr};
use common::{
    operation::Operation, operation_result::OperationResult, Connection, Message, NodeToStationMsg,
    Station, StationToNodeMsg,
};
use std::{
    collections::{HashMap, VecDeque},
    future::pending,
    net::SocketAddr,
    sync::Arc,
};
use tokio::sync::{mpsc, oneshot, Mutex};

/// Replica node.
///
/// Misma idea que el Leader:
/// - recibe logs del líder (`Message::Log`),
/// - guarda las operaciones,
/// - las va “commit-eando” contra el `ActorRouter`,
/// - cuando el router termina una op, la réplica le manda un `Ack` al líder.
///
/// Para las requests que vienen de la estación:
/// - si está ONLINE, las proxy-a al líder con `Message::Request`,
/// - si está OFFLINE, las mete en `offline_queue` y contesta OK a la estación.
#[derive(Debug, Clone)]
struct StationPendingCharge {
    pump_id: u32,
    account_id: u64,
    card_id: u64,
    amount: f32,
}

#[derive(Debug, Clone)]
struct OfflineQueuedCharge {
    request_id: u64,
    account_id: u64,
    card_id: u64,
    amount: f32,
}

pub struct Replica {
    id: u64,
    coords: (f64, f64),
    address: SocketAddr,
    leader_addr: SocketAddr,
    members: HashMap<u64, SocketAddr>,
    bully: Arc<Mutex<Bully>>,
    operations: HashMap<u32, Operation>,
    is_offline: bool,
    offline_queue: VecDeque<Operation>,
    station_requests: HashMap<u64, StationPendingCharge>,
    router: Addr<ActorRouter>,
}

impl Node for Replica {
    fn is_offline(&self) -> bool {
        self.is_offline
    }

    fn log_offline_operation(&mut self, op: Operation) {
        self.offline_queue.push_back(op);
    }

    fn get_address(&self) -> SocketAddr {
        self.address
    }

    async fn handle_operation_result(
        &mut self,
        connection: &mut Connection,
        _station: &mut Station,
        op_id: u32,
        _operation: Operation,
        _result: OperationResult,
    ) {
        connection
            .send(Message::Ack { op_id: op_id + 1 }, &self.leader_addr)
            .await
            .unwrap();
    }

    async fn handle_request(
        &mut self,
        connection: &mut Connection,
        op_id: u32,
        op: Operation,
        addr: SocketAddr,
    ) -> AppResult<()> {
        // redirijo al líder
        connection
            .send(
                Message::Request {
                    req_id: op_id,
                    addr,
                    op,
                },
                &self.leader_addr,
            )
            .await?;

        Ok(())
    }

    async fn handle_log(&mut self, _connection: &mut Connection, op_id: u32, new_op: Operation) {
        // Guardamos el log y commiteamos la operación anterior.
        self.operations.insert(op_id, new_op);
        if op_id == 0 {
            return; // si es la primera no hay op previamente loggeada
        }

        self.commit_operation(op_id - 1).await;
    }

    async fn handle_ack(&mut self, _connection: &mut Connection, _id: u32) {
        // Replicas no deberían recibir ACKs.
        todo!();
    }

    async fn handle_election(
        &mut self,
        connection: &mut Connection,
        candidate_id: u64,
        candidate_addr: SocketAddr,
    ) {
        // If we have higher id, reply ElectionOk and start our own election.
        let should_reply = {
            let b = self.bully.lock().await;
            b.should_reply_ok(candidate_id)
        };
        if should_reply {
            let reply = Message::ElectionOk {
                responder_id: self.id,
            };
            let _ = connection.send(reply, &candidate_addr).await;

            // Start own election immediately.
            self.start_election(connection).await;
        }
    }

    async fn handle_election_ok(&mut self, _connection: &mut Connection, responder_id: u64) {
        let mut b = self.bully.lock().await;
        b.on_election_ok(responder_id);
    }

    async fn handle_coordinator(
        &mut self,
        _connection: &mut Connection,
        leader_id: u64,
        leader_addr: SocketAddr,
    ) {
        let mut b = self.bully.lock().await;
        b.on_coordinator(leader_id, leader_addr);
        // Update our local "current leader" pointer as well.
        self.leader_addr = leader_addr;

        // TODO: promoción a líder si corresponde.
    }

    async fn start_election(&mut self, connection: &mut Connection) {
        // Build peer_ids map from the current membership (excluding self).
        let mut peer_ids = HashMap::new();
        for (peer_id, addr) in &self.members {
            if *peer_id == self.id {
                continue;
            }

            peer_ids.insert(*peer_id, *addr);
        }

        crate::node::election::bully::conduct_election(
            &self.bully,
            connection,
            peer_ids,
            self.id,
            self.address,
        )
        .await;
    }

    async fn handle_join(&mut self, connection: &mut Connection, addr: SocketAddr) {
        // No-op for replicas.
    }

    async fn handle_cluster_update(
        &mut self,
        _connection: &mut Connection,
        new_member: (u64, SocketAddr),
    ) {
        self.members.insert(new_member.0, new_member.1);
    }

    async fn handle_cluster_view(&mut self, members: Vec<(u64, SocketAddr)>) {
        // Si llega el cluster_view es porque nosotros mandamos el join, así que está ok.
        self.members.clear();
        for (id, addr) in members {
            self.members.insert(id, addr);
        }

        // Ensure we are present with the correct address.
        self.members.insert(self.id, self.address);
    }
}

impl Replica {
    async fn commit_operation(&mut self, op_id: u32) -> AppResult<()> {
        let Some(op) = self.operations.remove(&op_id) else {
            todo!();
        };
        // Igual que en Leader: fire-and-forget al router.
        self.router.do_send(RouterCmd::Execute {
            op_id,
            operation: op,
        });

        Ok(())
    }

    /// - boots the ConnectionManager (TCP),
    /// - boots the ActorRouter in a dedicated Actix system thread,
    /// - starts the Station abstraction (which internally spawns the simulator with `pumps` pumps on stdin),
    /// - seeds the cluster membership (self + leader),
    /// - sends an initial Join message to the leader,
    /// - then enters the main async event loop.
    pub async fn start(
        address: SocketAddr,
        leader_addr: SocketAddr,
        coords: (f64, f64),
        max_conns: usize,
        pumps: usize,
    ) -> AppResult<()> {
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();

        // Spawns a thread for Actix that runs inside a Tokio runtime.
        std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                let router = ActorRouter::new(actor_tx).start();
                if router_tx.send(router.clone()).is_err() {
                    // println!("[ERROR] Failed to deliver ActorRouter addr to Replica");
                }

                pending::<()>().await; // Keep the Actix system running indefinitely.
            });
        });

        // Start the reusable Station abstraction (stdin-based simulator runs inside it).
        let station = Station::start(pumps).await?;

        // Start network connection.
        let mut connection = Connection::start(address, max_conns).await?;

        // Seed membership: self + leader.
        let id = get_id_given_addr(address);
        let mut members: HashMap<u64, SocketAddr> = HashMap::new();
        members.insert(id, address);

        let leader_id = get_id_given_addr(leader_addr);
        members.insert(leader_id, leader_addr);

        let mut replica = Self {
            id,
            coords,
            address,
            leader_addr,
            members,
            bully: Arc::new(Mutex::new(Bully::new(id, address))),
            operations: HashMap::new(),
            is_offline: false,
            offline_queue: VecDeque::new(),
            station_requests: HashMap::new(),
            router: router_rx.await.map_err(|e| AppError::ActorSystem {
                details: format!("failed to receive ActorRouter address: {e}"),
            })?,
        };

        // Join inicial contra el líder para que rebroadcastée el ClusterView.
        let _ = connection
            .send(
                Message::Join {
                    addr: replica.address,
                },
                &replica.leader_addr,
            )
            .await;

        replica.run(connection, actor_rx, station).await
    }
}

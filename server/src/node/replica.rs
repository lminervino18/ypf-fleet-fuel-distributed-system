use super::{message::Message, network::Connection, node::Node, operation::Operation};
use crate::{
    actors::actor_router::ActorRouter, actors::messages::ActorEvent, errors::AppResult,
    node::station::StationToNodeMsg,
};
use actix::Addr;
use std::{collections::HashMap, net::SocketAddr};
use tokio::sync::mpsc;

/// Replica node.
///
/// Same wiring as the Leader, but intended to be a follower in the
/// distributed system. For station-originated charges it uses the
/// exact same Execute (verify + apply interno) flow as the Leader
/// when ONLINE, and the same offline queueing behavior when OFFLINE.
pub struct Replica {
    id: u64,
    coords: (f64, f64),
    max_conns: usize,
    leader_addr: SocketAddr,
    other_replicas: Vec<SocketAddr>,
    operations: HashMap<u32, Operation>,
    connection: Connection,
    actor_rx: mpsc::Receiver<ActorEvent>,
    is_offline: bool,
    router: Addr<ActorRouter>,
}

impl Node for Replica {
    async fn handle_request(&mut self, op: Operation, addr: SocketAddr) {
        // redirect to leader node
        self.connection
            .send(Message::Request { op, addr }, &self.leader_addr)
            .await;
    }

    async fn handle_log(&mut self, new_op: Operation) {
        let new_op_id = new_op.id;
        self.operations.insert(new_op_id, new_op);
        // self.commit_operation(new_op_id - 1).await; // TODO: this logic should be in actors mod
        self.connection
            .send(Message::Ack { id: new_op_id }, &self.leader_addr)
            .await;
        todo!();
    }

    async fn handle_ack(&mut self, _id: u32) {
        todo!(); // TODO: replicas should not receive any ACK msgs
    }

    async fn recv_node_msg(&mut self) -> AppResult<Message> {
        self.connection.recv().await
    }

    async fn recv_actor_event(&mut self) -> Option<ActorEvent> {
        todo!()
    }

    async fn handle_actor_event(&mut self, event: ActorEvent) {
        todo!()
    }

    async fn handle_station_msg(&mut self, msg: StationToNodeMsg) {
        todo!()
    }
}

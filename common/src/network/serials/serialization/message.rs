use crate::Message::*;
use crate::network::serials::protocol::*;
use crate::operation_result::OperationResult;
use crate::{Message, operation::Operation};
use std::net::{IpAddr, SocketAddr};

impl From<Message> for Vec<u8> {
    fn from(msg: Message) -> Self {
        match msg {
            Request {
                req_id: op_id,
                addr,
                op,
            } => serialize_request_message(op_id, addr, op),
            Log { op_id, op } => serialize_log_message(op_id, op),
            Ack { op_id } => serialize_ack_message(op_id),
            Join { addr } => serialize_join_message(addr),
            ClusterView { members } => serialize_cluster_view_message(members),
            ClusterUpdate { new_member } => serialize_cluster_update_message(new_member),
            Response { req_id, op_result } => serialize_response_message(req_id, op_result),
            _ => todo!(),
        }
    }
}

// importante que las operations vayan a lo último para que el checkeo de los lenghts sea
// independiente del resto del msj y se haga dentro de ese otro try from
fn serialize_response_message(req_id: u32, op_result: OperationResult) -> Vec<u8> {
    let type_srl = MSG_TYPE_RESPONSE;
    let req_id_srl = req_id.to_be_bytes();
    let op_result_srl: Vec<u8> = op_result.into();
    let mut srl = vec![];
    srl.push(type_srl);
    srl.extend(req_id_srl);
    srl.extend(op_result_srl);
    srl
}

fn serialize_request_message(op_id: u32, addr: SocketAddr, op: Operation) -> Vec<u8> {
    let type_srl = [MSG_TYPE_REQUEST];
    let op_id_srl = op_id.to_be_bytes();
    let addr_srl = serialize_socket_address(addr);
    let op_srl: Vec<u8> = op.into();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(op_id_srl);
    srl.extend(addr_srl);
    srl.extend(op_srl);
    srl
}

fn serialize_log_message(op_id: u32, op: Operation) -> Vec<u8> {
    let type_srl = [MSG_TYPE_LOG];
    let op_id_srl = op_id.to_be_bytes();
    let op_srl: Vec<u8> = op.into();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(op_id_srl);
    srl.extend(op_srl);
    srl
}

fn serialize_ack_message(op_id: u32) -> Vec<u8> {
    let type_srl = [MSG_TYPE_ACK];
    let op_id_srl = op_id.to_be_bytes();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(op_id_srl);
    srl
}

fn serialize_join_message(addr: SocketAddr) -> Vec<u8> {
    let type_srl = [MSG_TYPE_JOIN];
    let addr_srl = serialize_socket_address(addr);
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(addr_srl);
    srl
}

fn serialize_cluster_view_message(members: Vec<(u64, SocketAddr)>) -> Vec<u8> {
    let type_srl = [MSG_TYPE_CLUSTER_VIEW];
    let members_len_srl = members.len().to_be_bytes();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(members_len_srl);
    for member in members {
        srl.extend(serialize_member(member));
    }

    srl
}

fn serialize_member(member: (u64, SocketAddr)) -> Vec<u8> {
    let id_srl = member.0.to_be_bytes();
    let addr_srl = serialize_socket_address(member.1);
    let mut member_srl = vec![];
    member_srl.extend(id_srl);
    member_srl.extend(addr_srl);
    member_srl
}

fn serialize_cluster_update_message(new_member: (u64, SocketAddr)) -> Vec<u8> {
    let type_srl = [MSG_TYPE_CLUSTER_UPDATE];
    let new_member_srl = serialize_member(new_member);
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(new_member_srl);
    srl
}

fn serialize_socket_address(addr: SocketAddr) -> [u8; 6] {
    match addr.ip() {
        IpAddr::V4(ip) => {
            let [a, b, c, d] = ip.octets();
            let [p0, p1] = addr.port().to_be_bytes();
            [a, b, c, d, p0, p1]
        }
        _ => panic!("only ipv4 is supported"),
    }
}

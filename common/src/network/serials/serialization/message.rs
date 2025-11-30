use super::serialize_socket_address;
use crate::network::serials::protocol::*;
use crate::operation::DatabaseSnapshot;
use crate::operation_result::OperationResult;
use crate::{Message, operation::Operation};
use crate::{Message::*, NodeRole};
use std::net::SocketAddr;

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
            ClusterView {
                members,
                leader_addr,
                database,
            } => serialize_cluster_view_message(members, leader_addr, database),
            ClusterUpdate { new_member } => serialize_cluster_update_message(new_member),
            Response { req_id, op_result } => serialize_response_message(req_id, op_result),
            RoleQuery { addr } => serialize_role_query_message(addr),
            RoleResponse { node_id, role } => serialize_role_response_message(node_id, role),
            Coordinator {
                leader_id,
                leader_addr,
            } => serialize_coordinator_message(leader_id, leader_addr),
            Election {
                candidate_id,
                candidate_addr,
            } => serialize_election_message(candidate_id, candidate_addr),
            ElectionOk { responder_id } => serialize_election_ok_message(responder_id),
        }
    }
}

fn serialize_coordinator_message(leader_id: u64, leader_addr: SocketAddr) -> Vec<u8> {
    let type_srl = MSG_TYPE_COORDINATOR;
    let leader_id_srl = leader_id.to_be_bytes();
    let leader_addr_srl = serialize_socket_address(leader_addr);
    let mut srl = vec![];
    srl.push(type_srl);
    srl.extend(leader_id_srl);
    srl.extend(leader_addr_srl);
    srl
}

fn serialize_election_message(candidate_id: u64, candidate_addr: SocketAddr) -> Vec<u8> {
    let type_srl = MSG_TYPE_ELECTION;
    let candidate_id_srl = candidate_id.to_be_bytes();
    let candidate_addr_srl = serialize_socket_address(candidate_addr);
    let mut srl = vec![];
    srl.push(type_srl);
    srl.extend(candidate_id_srl);
    srl.extend(candidate_addr_srl);
    srl
}

fn serialize_election_ok_message(responder_id: u64) -> Vec<u8> {
    let type_srl = MSG_TYPE_ELECTION_OK;
    let responder_id_srl = responder_id.to_be_bytes();
    let mut srl = vec![];
    srl.push(type_srl);
    srl.extend(responder_id_srl);
    srl
}

fn serialize_role_response_message(node_id: u64, role: NodeRole) -> Vec<u8> {
    let type_srl = MSG_TYPE_ROLE_RESPONSE;
    let node_id_srl = node_id.to_be_bytes();
    let role_srl = match role {
        NodeRole::Leader => NODE_ROLE_LEADER,
        NodeRole::Replica => NODE_ROLE_REPLICA,
        NodeRole::Station => NODE_ROLE_CLIENT,
    };
    let mut srl = vec![];
    srl.push(type_srl);
    srl.extend(node_id_srl);
    srl.push(role_srl);
    srl
}

fn serialize_role_query_message(addr: SocketAddr) -> Vec<u8> {
    let type_srl = MSG_TYPE_ROLE_QUERY;
    let addr_srl = serialize_socket_address(addr);
    let mut srl = vec![type_srl];
    srl.extend(addr_srl);
    srl
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

fn serialize_cluster_view_message(
    members: Vec<(u64, SocketAddr)>,
    leader_addr: SocketAddr,
    database: DatabaseSnapshot,
) -> Vec<u8> {
    let type_srl = [MSG_TYPE_CLUSTER_VIEW];
    let members_len_srl = members.len().to_be_bytes();
    let leader_addr_srl = serialize_socket_address(leader_addr);
    let database_srl: Vec<u8> = database.into();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(members_len_srl);
    for member in members {
        srl.extend(serialize_member(member));
    }

    srl.extend(leader_addr_srl);
    srl.extend(database_srl);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::Operation;
    use crate::operation_result::{ChargeResult, OperationResult};
    use crate::NodeRole;
    use crate::network::serials::protocol::*;
    use std::net::SocketAddr;

    fn addr_localhost() -> SocketAddr {
        "127.0.0.1:8080".parse().unwrap()
    }

    fn sample_op() -> Operation {
        Operation::Charge {
            account_id: 1,
            card_id: 2,
            amount: 3.5,
            from_offline_station: false,
        }
    }

    fn sample_op_result() -> OperationResult {
        OperationResult::Charge(ChargeResult::Ok)
    }

    // ================
    // Helpers puros
    // ================

    #[test]
    fn test_serialize_ack_message() {
        let op_id = 42u32;
        let bytes = super::serialize_ack_message(op_id);

        assert_eq!(bytes[0], MSG_TYPE_ACK);
        assert_eq!(&bytes[1..5], &op_id.to_be_bytes());
        assert_eq!(bytes.len(), 1 + 4);
    }

    #[test]
    fn test_serialize_join_message() {
        let addr = addr_localhost();
        let addr_srl = super::serialize_socket_address(addr);
        let bytes = super::serialize_join_message(addr);

        assert_eq!(bytes[0], MSG_TYPE_JOIN);
        assert_eq!(&bytes[1..], &addr_srl[..]);
    }

    #[test]
    fn test_serialize_role_query_message() {
        let addr = addr_localhost();
        let addr_srl = super::serialize_socket_address(addr);
        let bytes = super::serialize_role_query_message(addr);

        assert_eq!(bytes[0], MSG_TYPE_ROLE_QUERY);
        assert_eq!(&bytes[1..], &addr_srl[..]);
    }

    #[test]
    fn test_serialize_role_response_message_leader() {
        let node_id = 123u64;
        let bytes = super::serialize_role_response_message(node_id, NodeRole::Leader);

        assert_eq!(bytes[0], MSG_TYPE_ROLE_RESPONSE);
        assert_eq!(&bytes[1..9], &node_id.to_be_bytes());
        assert_eq!(bytes[9], NODE_ROLE_LEADER);
    }

    #[test]
    fn test_serialize_role_response_message_replica() {
        let node_id = 999u64;
        let bytes = super::serialize_role_response_message(node_id, NodeRole::Replica);

        assert_eq!(bytes[0], MSG_TYPE_ROLE_RESPONSE);
        assert_eq!(&bytes[1..9], &node_id.to_be_bytes());
        assert_eq!(bytes[9], NODE_ROLE_REPLICA);
    }

    #[test]
    fn test_serialize_election_message() {
        let candidate_id = 7u64;
        let addr = addr_localhost();
        let addr_srl = super::serialize_socket_address(addr);

        let bytes = super::serialize_election_message(candidate_id, addr);

        assert_eq!(bytes[0], MSG_TYPE_ELECTION);
        assert_eq!(&bytes[1..9], &candidate_id.to_be_bytes());
        assert_eq!(&bytes[9..], &addr_srl[..]);
    }

    #[test]
    fn test_serialize_election_ok_message() {
        let responder_id = 55u64;
        let bytes = super::serialize_election_ok_message(responder_id);

        assert_eq!(bytes[0], MSG_TYPE_ELECTION_OK);
        assert_eq!(&bytes[1..9], &responder_id.to_be_bytes());
        assert_eq!(bytes.len(), 1 + 8);
    }

    #[test]
    fn test_serialize_coordinator_message() {
        let leader_id = 100u64;
        let addr = addr_localhost();
        let addr_srl = super::serialize_socket_address(addr);

        let bytes = super::serialize_coordinator_message(leader_id, addr);

        assert_eq!(bytes[0], MSG_TYPE_COORDINATOR);
        assert_eq!(&bytes[1..9], &leader_id.to_be_bytes());
        assert_eq!(&bytes[9..], &addr_srl[..]);
    }

    #[test]
    fn test_serialize_cluster_view_message_single_member() {
        let member_id = 1u64;
        let addr = addr_localhost();

        let bytes = super::serialize_cluster_view_message(vec![(member_id, addr)]);
        let members_len = 1usize.to_be_bytes();

        assert_eq!(bytes[0], MSG_TYPE_CLUSTER_VIEW);
        assert_eq!(&bytes[1..1 + members_len.len()], &members_len);

        // Después de length viene el member: id (8 bytes) + socket
        let member_bytes = super::serialize_member((member_id, addr));
        assert_eq!(
            &bytes[1 + members_len.len()..],
            &member_bytes[..]
        );
    }

    #[test]
    fn test_serialize_cluster_update_message() {
        let member_id = 5u64;
        let addr = addr_localhost();
        let member_bytes = super::serialize_member((member_id, addr));

        let bytes = super::serialize_cluster_update_message((member_id, addr));

        assert_eq!(bytes[0], MSG_TYPE_CLUSTER_UPDATE);
        assert_eq!(&bytes[1..], &member_bytes[..]);
    }

    #[test]
    fn test_serialize_request_message_layout() {
        let op_id = 10u32;
        let addr = addr_localhost();
        let addr_srl = super::serialize_socket_address(addr);
        let op = sample_op();
        let op_srl: Vec<u8> = op.clone().into();

        let bytes = super::serialize_request_message(op_id, addr, op);

        assert_eq!(bytes[0], MSG_TYPE_REQUEST);
        assert_eq!(&bytes[1..5], &op_id.to_be_bytes());
        assert_eq!(&bytes[5..5 + addr_srl.len()], &addr_srl[..]);
        assert_eq!(&bytes[5 + addr_srl.len()..], &op_srl[..]);
    }

    #[test]
    fn test_serialize_log_message_layout() {
        let op_id = 99u32;
        let op = sample_op();
        let op_srl: Vec<u8> = op.clone().into();

        let bytes = super::serialize_log_message(op_id, op);

        assert_eq!(bytes[0], MSG_TYPE_LOG);
        assert_eq!(&bytes[1..5], &op_id.to_be_bytes());
        assert_eq!(&bytes[5..], &op_srl[..]);
    }

    #[test]
    fn test_serialize_response_message_layout() {
        let req_id = 1234u32;
        let op_result = sample_op_result();
        let op_res_srl: Vec<u8> = op_result.clone().into();

        let bytes = super::serialize_response_message(req_id, op_result);

        assert_eq!(bytes[0], MSG_TYPE_RESPONSE);
        assert_eq!(&bytes[1..5], &req_id.to_be_bytes());
        assert_eq!(&bytes[5..], &op_res_srl[..]);
    }

    // ================
    // Tests sobre From<Message> for Vec<u8>
    // ================

    #[test]
    fn test_from_message_ack() {
        let op_id = 77u32;
        let msg = Message::Ack { op_id };
        let bytes: Vec<u8> = msg.into();

        let expected = super::serialize_ack_message(op_id);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_from_message_join() {
        let addr = addr_localhost();
        let msg = Message::Join { addr };
        let bytes: Vec<u8> = msg.into();

        let expected = super::serialize_join_message(addr);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_from_message_election() {
        let candidate_id = 3u64;
        let addr = addr_localhost();
        let msg = Message::Election {
            candidate_id,
            candidate_addr: addr,
        };
        let bytes: Vec<u8> = msg.into();

        let expected = super::serialize_election_message(candidate_id, addr);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_from_message_election_ok() {
        let responder_id = 9u64;
        let msg = Message::ElectionOk { responder_id };
        let bytes: Vec<u8> = msg.into();

        let expected = super::serialize_election_ok_message(responder_id);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_from_message_coordinator() {
        let leader_id = 11u64;
        let addr = addr_localhost();
        let msg = Message::Coordinator {
            leader_id,
            leader_addr: addr,
        };
        let bytes: Vec<u8> = msg.into();

        let expected = super::serialize_coordinator_message(leader_id, addr);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_from_message_role_query() {
        let addr = addr_localhost();
        let msg = Message::RoleQuery { addr };
        let bytes: Vec<u8> = msg.into();

        let expected = super::serialize_role_query_message(addr);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_from_message_role_response() {
        let node_id = 42u64;
        let role = NodeRole::Replica;
        let msg = Message::RoleResponse { node_id, role: role.clone() };
        let bytes: Vec<u8> = msg.into();

        let expected = super::serialize_role_response_message(node_id, role);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_from_message_request() {
        let req_id = 1u32;
        let addr = addr_localhost();
        let op = sample_op();
        let msg = Message::Request {
            req_id,
            addr,
            op: op.clone(),
        };

        let bytes: Vec<u8> = msg.into();
        let expected = super::serialize_request_message(req_id, addr, op);

        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_from_message_response() {
        let req_id = 33u32;
        let op_result = sample_op_result();
        let msg = Message::Response {
            req_id,
            op_result: op_result.clone(),
        };

        let bytes: Vec<u8> = msg.into();
        let expected = super::serialize_response_message(req_id, op_result);

        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_from_message_cluster_view() {
        let addr = addr_localhost();
        let members = vec![(1u64, addr)];
        let msg = Message::ClusterView {
            members: members.clone(),
        };
        let bytes: Vec<u8> = msg.into();

        let expected = super::serialize_cluster_view_message(members);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn test_from_message_cluster_update() {
        let addr = addr_localhost();
        let new_member = (5u64, addr);
        let msg = Message::ClusterUpdate {
            new_member: new_member.clone(),
        };
        let bytes: Vec<u8> = msg.into();

        let expected = super::serialize_cluster_update_message(new_member);
        assert_eq!(bytes, expected);
    }
}

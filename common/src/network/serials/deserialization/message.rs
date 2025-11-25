use crate::errors::AppError;
use crate::errors::AppResult;
use crate::network::serials::protocol::*;
use crate::Message;
use crate::Message::*;
use std::net::SocketAddr;

impl TryFrom<Vec<u8>> for Message {
    type Error = AppError;

    fn try_from(payload: Vec<u8>) -> AppResult<Self> {
        match payload[0] {
            MSG_TYPE_REQUEST => deserialize_request_message(&payload[1..]),
            MSG_TYPE_LOG => deserialize_log_message(&payload[1..]),
            MSG_TYPE_ACK => deserialize_ack_message(&payload[1..]),
            MSG_TYPE_JOIN => deserialize_join_message(&payload[1..]),
            MSG_TYPE_CLUSTER_VIEW => deserialize_cluster_view_message(&payload[1..]),
            MSG_TYPE_CLUSTER_UPDATE => deserialize_cluster_update_message(&payload[1..]),
            MSG_TYPE_RESPONSE => deserialize_response_message(&payload[1..]),
            MSG_TYPE_ROLE_QUERY => deserialize_role_query_message(&payload[1..]),
            MSG_TYPE_COORDINATOR => deserialize_coordinator_message(&payload[1..]),
            MSG_TYPE_ELECTION => deserialize_election_message(&payload[1..]),
            MSG_TYPE_ELECTION_OK => deserialize_election_ok_message(&payload[1..]),
            _ => Err(AppError::InvalidData {
                details: format!(
                    "unknown node message type {}, with contents {:?}",
                    payload[0], payload
                ),
            }),
        }
    }
}

fn deserialize_coordinator_message(payload: &[u8]) -> AppResult<Message> {
    let mut ptr = 0;
    let leader_id = u64::from_be_bytes(payload[ptr..ptr + NODE_ID_SRL_LEN].try_into().map_err(
        |e| AppError::InvalidProtocol {
            details: format!("failed to deserialize leader_id in coordinator message: {e}"),
        },
    )?);
    ptr += NODE_ID_SRL_LEN;
    let leader_addr = deserialize_socket_address_srl(&payload[ptr..])?;
    Ok(Message::Coordinator {
        leader_id,
        leader_addr,
    })
}

fn deserialize_election_message(payload: &[u8]) -> AppResult<Message> {
    let mut ptr = 0;
    let candidate_id = u64::from_be_bytes(payload[ptr..ptr + NODE_ID_SRL_LEN].try_into().map_err(
        |e| AppError::InvalidProtocol {
            details: format!("failed to deserialize candidate_id in election message: {e}"),
        },
    )?);
    ptr += NODE_ID_SRL_LEN;
    let candidate_addr = deserialize_socket_address_srl(&payload[ptr..])?;
    Ok(Message::Election {
        candidate_id,
        candidate_addr,
    })
}

fn deserialize_election_ok_message(payload: &[u8]) -> AppResult<Message> {
    let responder_id = u64::from_be_bytes(payload[0..NODE_ID_SRL_LEN].try_into().map_err(|e| {
        AppError::InvalidProtocol {
            details: format!("failed to deserialize responder_id in election_ok message: {e}"),
        }
    })?);
    Ok(Message::ElectionOk { responder_id })
}

fn deserialize_role_query_message(payload: &[u8]) -> AppResult<Message> {
    let addr = deserialize_socket_address_srl(payload)?;
    Ok(Message::RoleQuery { addr })
}

fn deserialize_response_message(payload: &[u8]) -> AppResult<Message> {
    let mut ptr = 0;
    let req_id = deserialize_op_id(&payload[ptr..])?;
    ptr += OP_ID_SRL_LEN;
    let op_result = payload[ptr..].try_into()?;
    Ok(Message::Response { req_id, op_result })
}

fn deserialize_request_message(payload: &[u8]) -> AppResult<Message> {
    let mut ptr = 0;
    let op_id = deserialize_op_id(&payload[ptr..])?;
    ptr += OP_ID_SRL_LEN;
    let addr = deserialize_socket_address_srl(&payload[ptr..])?;
    ptr += SOCKET_ADDR_LEN;
    let op = payload[ptr..].try_into()?;
    Ok(Request {
        req_id: op_id,
        addr,
        op,
    })
}

fn deserialize_log_message(payload: &[u8]) -> AppResult<Message> {
    let mut ptr = 0;
    let op_id = deserialize_op_id(&payload[ptr..])?;
    ptr += OP_ID_SRL_LEN;
    let op = payload[ptr..].try_into()?;
    Ok(Log { op_id, op })
}

fn deserialize_ack_message(payload: &[u8]) -> AppResult<Message> {
    let op_id = deserialize_op_id(&payload[0..])?;
    Ok(Ack { op_id })
}

fn deserialize_member(payload: &[u8]) -> AppResult<(u64, SocketAddr)> {
    let mut ptr = 0;
    let id = u64::from_be_bytes(payload[ptr..NODE_ID_SRL_LEN].try_into().map_err(|e| {
        AppError::InvalidProtocol {
            details: format!("failed to deserialize member's node_id: {e}"),
        }
    })?);
    ptr += NODE_ID_SRL_LEN;
    let addr = deserialize_socket_address_srl(&payload[ptr..])?;
    Ok((id, addr))
}

fn deserialize_cluster_update_message(payload: &[u8]) -> AppResult<Message> {
    if payload.len() < MEMBER_SRL_LEN {
        return Err(AppError::InvalidProtocol {
            details: "not enough bytes to deserialize op_id".to_string(),
        });
    }

    let new_member = deserialize_member(payload)?;
    Ok(ClusterUpdate { new_member })
}

fn deserialize_join_message(payload: &[u8]) -> AppResult<Message> {
    let addr = deserialize_socket_address_srl(payload)?;
    Ok(Join { addr })
}

fn deserialize_cluster_view_message(payload: &[u8]) -> AppResult<Message> {
    let mut ptr = 0;
    let members_len =
        usize::from_be_bytes(payload[ptr..MEMBERS_LEN_SRL_LEN].try_into().map_err(|e| {
            AppError::InvalidProtocol {
                details: format!("failed to deserialize members_len in cluster_view message: {e}"),
            }
        })?);

    if payload.len() < MEMBER_SRL_LEN * members_len {
        return Err(AppError::InvalidProtocol {
            details: "not enough bytes to deserialize all members in cluster_view message"
                .to_string(),
        });
    }

    ptr += MEMBERS_LEN_SRL_LEN;
    let mut members = vec![];
    for _ in 0..members_len {
        members.push(deserialize_member(&payload[ptr..])?);
        ptr += MEMBER_SRL_LEN;
    }

    Ok(ClusterView { members })
}

fn deserialize_op_id(payload: &[u8]) -> AppResult<u32> {
    if payload.len() < OP_ID_SRL_LEN {
        return Err(AppError::InvalidProtocol {
            details: "not enough bytes to deserialize op_id".to_string(),
        });
    }

    Ok(u32::from_be_bytes(payload[0..4].try_into().map_err(
        |e| AppError::InvalidProtocol {
            details: format!("failed to deserialize op_id in `Request` message: {e}"),
        },
    )?))
}

fn deserialize_socket_address_srl(payload: &[u8]) -> AppResult<SocketAddr> {
    if payload.len() < SOCKET_ADDR_LEN {
        return Err(AppError::InvalidProtocol {
            details: "not enough bytes to deserialize socket_address_srl".to_string(),
        });
    }

    let ip: [u8; 4] = payload[0..4]
        .try_into()
        .map_err(|e| AppError::InvalidProtocol {
            details: format!("failed to read address ip bytes in request message: {e}"),
        })?;
    let port =
        u16::from_be_bytes(
            payload[4..6]
                .try_into()
                .map_err(|e| AppError::InvalidProtocol {
                    details: format!("failed to read address port bytes in request message: {e}"),
                })?,
        );
    Ok(SocketAddr::from((ip, port)))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        operation::Operation,
        operation_result::{ChargeResult, OperationResult},
    };

    #[test]
    fn test_deserialize_valid_op_id_srl() {
        let op_id_srl = 15389u32.to_be_bytes();
        let op_id = deserialize_op_id(&op_id_srl);
        let expected = Ok(15389u32);
        assert_eq!(op_id, expected);
    }

    #[test]
    fn test_deserialize_socket_address_srl() {
        let ip = [127, 0, 0, 1];
        let port: u16 = 12345;
        let mut socket_addr_srl = ip.to_vec();
        socket_addr_srl.extend(port.to_be_bytes());
        let expected = Ok(SocketAddr::from((ip, port)));
        let socket_addr = deserialize_socket_address_srl(&socket_addr_srl);
        assert_eq!(socket_addr, expected);
    }

    #[test]
    fn test_deserialize_request_message() {
        let msg = Message::Request {
            req_id: 15936,
            op: Operation::Charge {
                account_id: 15000,
                card_id: 8934,
                amount: 346389.5,
                from_offline_station: true,
            },
            addr: SocketAddr::from(([127, 0, 0, 1], 12345)),
        };
        let msg_srl: Vec<u8> = msg.clone().into();
        let expected = Ok(msg);
        let msg = msg_srl.try_into();
        assert_eq!(msg, expected);
    }

    #[test]
    fn test_deserialize_log_message() {
        let msg = Message::Log {
            op_id: 15936,
            op: Operation::Charge {
                account_id: 15000,
                card_id: 8934,
                amount: 346389.5,
                from_offline_station: true,
            },
        };
        let msg_srl: Vec<u8> = msg.clone().into();
        let expected = Ok(msg);
        let msg = msg_srl.try_into();
        assert_eq!(msg, expected);
    }

    #[test]
    fn test_deserialize_ack_message() {
        let msg = Message::Ack { op_id: 15936 };
        let msg_srl: Vec<u8> = msg.clone().into();
        let expected = Ok(msg);
        let msg = msg_srl.try_into();
        assert_eq!(msg, expected);
    }

    #[test]
    fn test_deserialize_join_message() {
        let msg = Message::Join {
            addr: SocketAddr::from(([127, 0, 0, 1], 12345)),
        };
        let msg_srl: Vec<u8> = msg.clone().into();
        let expected = Ok(msg);
        let msg = msg_srl.try_into();
        assert_eq!(msg, expected);
    }

    #[test]
    fn test_deserialize_cluster_view_message() {
        let msg = Message::ClusterView {
            members: vec![
                (132, SocketAddr::from(([127, 0, 0, 1], 12345))),
                (8534, SocketAddr::from(([127, 0, 0, 1], 12346))),
                (12, SocketAddr::from(([127, 0, 0, 1], 12347))),
            ],
        };
        let msg_srl: Vec<u8> = msg.clone().into();
        let expected = Ok(msg);
        let msg = msg_srl.try_into();
        assert_eq!(msg, expected);
    }

    #[test]
    fn test_deserialize_cluster_update_message() {
        let msg = Message::ClusterUpdate {
            new_member: (13248, SocketAddr::from(([127, 0, 0, 1], 12346))),
        };
        let msg_srl: Vec<u8> = msg.clone().into();
        let expected = Ok(msg);
        let msg = msg_srl.try_into();
        assert_eq!(msg, expected);
    }

    #[test]
    fn test_deserialize_response_message() {
        let msg = Message::Response {
            req_id: 12039,
            op_result: OperationResult::Charge(ChargeResult::Ok),
        };
        let msg_srl: Vec<u8> = msg.clone().into();
        let expected = Ok(msg);
        let msg = msg_srl.try_into();
        assert_eq!(msg, expected);
    }

    /* MSG_TYPE_ELECTION => deserialize_election_message(&payload[1..]),
    MSG_TYPE_ELECTION_OK => deserialize_election_ok_message(&payload[1..]), */
    #[test]
    fn test_deserialize_election_message() {
        let msg = Message::Election {
            candidate_id: 234,
            candidate_addr: SocketAddr::from(([127, 0, 0, 1], 12346)),
        };
        let msg_srl: Vec<u8> = msg.clone().into();
        let expected = Ok(msg);
        let msg = msg_srl.try_into();
        assert_eq!(msg, expected);
    }

    #[test]
    fn test_deserialize_election_ok_message() {
        let msg = Message::ElectionOk {
            responder_id: 15388,
        };
        let msg_srl: Vec<u8> = msg.clone().into();
        let expected = Ok(msg);
        let msg = msg_srl.try_into();
        assert_eq!(msg, expected);
    }
}

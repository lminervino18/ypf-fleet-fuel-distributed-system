use super::operation::Operation;
use crate::{
    errors::AppError,
    node::serial_helpers::{deserialize_socket_addr, read_bytes, serialize_socket_addr},
};
use NodeMessage::*;

/// Messages sent between nodes.
#[derive(Debug, PartialEq)]
pub enum NodeMessage {
    /// Operation request sent by the client server (station).
    ///
    /// This message is only handled by the coordinator. In case a replica receives it, it resends
    /// it to the coordinator.
    Request { op: Operation, addr: SocketAddr },

    /// Log message sent by coordinator to the replicas.
    ///
    /// The optional `u32` is a placeholder for a commit order. Replicas commit the operation on
    /// their logs with the `commit: u32` id.
    Log { op: Operation },

    /// Acknowledgement reply sent by a replica after receiving a valid `Log` message from the
    /// coordinator.
    Ack { id: u32 },
}

use std::net::SocketAddr;

impl TryFrom<Vec<u8>> for NodeMessage {
    type Error = AppError;

    fn try_from(payload: Vec<u8>) -> Result<Self, AppError> {
        match payload[0] {
            0u8 => Ok(Request {
                op: payload[1..25].try_into()?,
                addr: deserialize_socket_addr(read_bytes(&payload, 25..31)?)?,
            }),
            1u8 => Ok(Log {
                op: payload[5..].try_into()?,
            }),
            2u8 => Ok(Ack {
                id: u32::from_be_bytes(payload[1..5].try_into().map_err(|e| {
                    AppError::InvalidData {
                        details: format!("not enough bytes to deserialize ACK: {e}"),
                    }
                })?),
            }),
            _ => Err(AppError::InvalidData {
                details: ("unknown node message type".to_string()),
            }),
        }
    }
}

impl From<NodeMessage> for Vec<u8> {
    fn from(msg: NodeMessage) -> Self {
        match msg {
            NodeMessage::Request { op, addr } => {
                let mut srl: Vec<u8> = vec![0u8];
                let op_srl: Vec<u8> = op.into();
                let addr_srl = serialize_socket_addr(addr).unwrap(); // TODO: this unwrap has to be
                                                                     // avoided by specifying that
                                                                     // the socket addr HAS to be
                                                                     // IPv4
                srl.extend(op_srl);
                srl.extend(addr_srl);
                srl
            }
            NodeMessage::Log { op } => {
                let mut srl = vec![1u8];
                let op_srl: Vec<u8> = op.into();
                srl.extend(op_srl);
                srl
            }
            NodeMessage::Ack { id } => {
                let mut srl = vec![2u8];
                let id_srl = id.to_be_bytes().to_vec();
                srl.extend(id_srl);
                srl
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn deserialize_valid_operation_request_node_msg() {
        let op = Operation {
            id: 1,
            account_id: 348,
            card_id: 34821,
            amount: 80500.53,
        };
        let op_srl: Vec<u8> = op.clone().into();
        let addr = SocketAddr::new(IpAddr::V4([127, 0, 0, 1].into()), 12345);
        let addr_srl = serialize_socket_addr(addr).unwrap();
        let mut msg_bytes = [0x00].to_vec();
        msg_bytes.extend(op_srl);
        msg_bytes.extend(addr_srl);
        let node_msg: NodeMessage = msg_bytes.try_into().unwrap();
        let expected = NodeMessage::Request { op, addr };
        assert_eq!(node_msg, expected);
    }

    #[test]
    fn serialize_operation_request_node_msg() {
        let op = Operation {
            id: 1,
            account_id: 348,
            card_id: 34821,
            amount: 80500.53,
        };
        let addr = SocketAddr::new(IpAddr::V4([127, 0, 0, 1].into()), 12345);
        let node_msg = NodeMessage::Request {
            op: op.clone(),
            addr,
        };
        let msg_bytes: Vec<u8> = node_msg.into();
        let mut expected = [0x00].to_vec();
        let op_srl: Vec<u8> = op.into();
        let addr_srl = serialize_socket_addr(addr).unwrap();
        expected.extend(op_srl);
        expected.extend(addr_srl);
        assert_eq!(msg_bytes, expected);
    }
}

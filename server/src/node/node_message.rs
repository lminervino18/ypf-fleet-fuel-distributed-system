use super::operation::Operation;
use crate::errors::AppError;

/// Messages sent between nodes.
#[derive(Debug, PartialEq)]
pub enum NodeMessage {
    /// Operation request sent by the client server (station).
    ///
    /// This message is only handled by the coordinator. In case a replica receives it, it resends
    /// it to the coordinator.
    Request { op: Operation },

    /// Log message sent by coordinator to the replicas.
    ///
    /// The optional `u32` is a placeholder for a commit order. Replicas commit the operation on
    /// their logs with the `commit: u32` id.
    Log { op: Operation },

    /// Acknowledgement reply sent by a replica after receiving a valid `Log` message from the
    /// coordinator.
    Ack { id: u32 },
}

use NodeMessage::*;

impl TryFrom<Vec<u8>> for NodeMessage {
    type Error = AppError;

    fn try_from(payload: Vec<u8>) -> Result<Self, AppError> {
        match payload[0] {
            0u8 => Ok(Request {
                op: payload[1..].try_into()?,
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
            NodeMessage::Request { op } => {
                let mut srl: Vec<u8> = vec![0u8];
                let op_srl: Vec<u8> = op.into();
                srl.extend(op_srl);
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

    #[test]
    fn deserialize_valid_operation_request_node_msg() {
        let msg_bytes = [0x00, 0x00, 0x00, 0x00, 0x01].to_vec();
        let node_msg: NodeMessage = msg_bytes.try_into().unwrap();
        let expected = NodeMessage::Request {
            op: Operation { id: 1 },
        };
        assert_eq!(node_msg, expected);
    }

    #[test]
    fn serialize_operation_request_node_msg() {
        let node_msg = NodeMessage::Request {
            op: Operation { id: 1 },
        };
        let msg_bytes: Vec<u8> = node_msg.into();
        let expected = [0x00, 0x00, 0x00, 0x00, 0x01].to_vec();
        assert_eq!(msg_bytes, expected);
    }
}

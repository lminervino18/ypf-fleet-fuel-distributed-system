use crate::operation::Operation;
use crate::serial_error::SerialError;

#[derive(Debug, PartialEq)]
pub enum NodeMessage {
    Error,
    Accept(Operation),
    /* estos tipos de mensaje están para mostrar el
     * esqueleto, no deberían llevar todos Operation
     * adentro
     ***/
    Learn(Operation),
    Commit(Operation),
    Finished(Operation),
}

// 1. coord -> replicas: accept
// 2. replicas -> coord: learn
// 3. coord -> replicas: commit
// 4. replicas -> coord: finished

impl TryFrom<Vec<u8>> for NodeMessage {
    type Error = SerialError;

    fn try_from(payload: Vec<u8>) -> Result<Self, SerialError> {
        match payload[0] {
            0x01 => Ok(NodeMessage::Accept(Operation::try_from(&payload[1..])?)),
            _ => Err(SerialError::InvalidBytes),
        }
    }
}

impl From<NodeMessage> for Vec<u8> {
    fn from(msg: NodeMessage) -> Self {
        match msg {
            NodeMessage::Accept(op) => {
                let mut srl: Vec<u8> = [0x01].to_vec();
                let op_srl: Vec<u8> = op.into();
                srl.extend(op_srl);
                srl
            }
            _ => [].to_vec(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn deserialize_valid_accept_operation_node_msg() {
        let msg_bytes = [0x01, 0x01].to_vec();
        let node_msg: NodeMessage = msg_bytes.try_into().unwrap();
        let expected = NodeMessage::Accept(Operation { id: 1 });
        assert_eq!(node_msg, expected);
    }

    #[test]
    fn serialize_accept_operation_node_msg() {
        let node_msg = NodeMessage::Accept(Operation { id: 1 });
        let msg_bytes: Vec<u8> = node_msg.into();
        let expected = [0x01, 0x01].to_vec();
        assert_eq!(msg_bytes, expected);
    }
}

use super::helpers::serialize_socket_addr;
use crate::node::message::Message;
use crate::node::message::Message::*;

impl From<Message> for &[u8] {
    fn from(msg: Message) -> Self {
        todo!();
    }
}

impl From<Message> for Vec<u8> {
    fn from(msg: Message) -> Self {
        match msg {
            Request { op, addr } => {
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
            Log { op } => {
                let mut srl = vec![1u8];
                let op_srl: Vec<u8> = op.into();
                srl.extend(op_srl);
                srl
            }
            Ack { id } => {
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
    use crate::node::operation::Operation;
    use std::net::{IpAddr, SocketAddr};

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
        let node_msg: Message = msg_bytes.try_into().unwrap();
        let expected = Message::Request { op, addr };
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
        let node_msg = Message::Request {
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

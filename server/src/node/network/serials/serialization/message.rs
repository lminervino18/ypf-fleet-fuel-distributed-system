use crate::node::message::Message;
use crate::node::message::Message::*;
use crate::node::network::serials::protocol::*;
use crate::node::operation::Operation;
use std::net::{IpAddr, SocketAddr};

impl From<Message> for Vec<u8> {
    fn from(msg: Message) -> Self {
        match msg {
            Request { op_id, addr, op } => serialize_request_message(op_id, addr, op),
            Log { op_id, op } => serialize_log_message(op_id, op),
            Ack { op_id } => serialize_ack_message(op_id),
        }
    }
}

// importante que las operations vayan a lo último para que el checkeo de los lenghts sea
// independiente del resto del msj y se haga dentro de ese otro try from
fn serialize_request_message(op_id: u32, addr: SocketAddr, op: Operation) -> Vec<u8> {
    let type_srl = [MSG_TYPE_REQUEST];
    let op_id_srl = op_id.to_be_bytes();
    let addr_srl: [u8; 6] = match addr.ip() {
        IpAddr::V4(ip) => {
            let [a, b, c, d] = ip.octets();
            let [p0, p1] = addr.port().to_be_bytes();
            [a, b, c, d, p0, p1]
        }
        _ => panic!("only ipv4 is supported"),
    };
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_serialize_charge_operation() {
        let op = Operation::Charge {
            account_id: 348,
            card_id: 34821,
            amount: 80500.53,
            from_offline_station: true,
        };

        let op_srl: Vec<u8> = op.into();
        let expected = [
            // 0x00, 0x00, 0x00, 0x7E, // 126
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x5C, // 348
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88, 0x05, // 34721
            0x47, 0x9d, 0x3a, 0x44, // 80500.53 IEEE 754 simple precision
            0x01,
        ];
        assert_eq!(op_srl, expected);
    }

    /* #[test]
    fn deserialize_opeartion() {
        let op_srl = [
            0x00, 0x00, 0x00, 0x7E, // 126
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x5C, // 348
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88, 0x05, // 34721
            0x47, 0x9d, 0x3a, 0x44, // 80500.53 IEEE simple precision
        ];
        let expected = Operation {
            id: 126,
            account_id: 348,
            card_id: 34821,
            amount: 80500.53,
        };
        let op: Operation = op_srl[..].try_into().unwrap();
        assert_eq!(op, expected); */

    // TODO: arreglar estos tests q están usando métodos anteriores al refac
    /* #[test]
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
    } */
}

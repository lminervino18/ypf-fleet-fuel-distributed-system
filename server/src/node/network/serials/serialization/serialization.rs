use super::protocol::*;
use crate::node::message::Message;
use crate::node::message::Message::*;
use crate::node::operation::Operation;
use crate::node::operation::Operation::*;
use std::net::IpAddr;

impl From<Operation> for Vec<u8> {
    fn from(op: Operation) -> Self {
        match op {
            Charge {
                account_id,
                card_id,
                amount,
                from_offline_station,
            } => {
                let type_srl = [OP_TYPE_CHARGE];
                let acc_id_srl = account_id.to_be_bytes();
                let card_id_srl = card_id.to_be_bytes();
                let amount_srl = amount.to_be_bytes();
                let offline_srl = if from_offline_station {
                    TRUE.to_be_bytes()
                } else {
                    FALSE.to_be_bytes()
                };
                let mut srl = vec![];
                srl.extend(type_srl);
                srl.extend(acc_id_srl);
                srl.extend(card_id_srl);
                srl.extend(amount_srl);
                srl.extend(offline_srl);
                srl
            }
            LimitAccount {
                account_id,
                new_limit,
            } => {
                let type_srl = [OP_TYPE_LIMIT_ACCOUNT];
                let acc_id_srl = account_id.to_be_bytes();
                let new_limit_srl = new_limit.unwrap_or(NO_LIMIT).to_be_bytes();
                let mut srl = vec![];
                srl.extend(type_srl);
                srl.extend(acc_id_srl);
                srl.extend(new_limit_srl);
                srl
            }
            LimitCard {
                account_id,
                card_id,
                new_limit,
            } => {
                let type_srl = [OP_TYPE_LIMIT_CARD];
                let acc_id_srl = account_id.to_be_bytes();
                let card_id_srl = card_id.to_be_bytes();
                let new_limit_srl = new_limit.unwrap_or(NO_LIMIT).to_be_bytes();
                let mut srl = vec![];
                srl.extend(type_srl);
                srl.extend(acc_id_srl);
                srl.extend(card_id_srl);
                srl.extend(new_limit_srl);
                srl
            }
        }
    }
}

impl From<Message> for Vec<u8> {
    fn from(msg: Message) -> Self {
        match msg {
            Request { op_id, op, addr } => {
                let type_srl = [MSG_TYPE_REQUEST];
                let op_id_srl = op_id.to_be_bytes();
                let op_srl: Vec<u8> = op.into();
                let addr_srl: [u8; 6] = match addr.ip() {
                    IpAddr::V4(ip) => {
                        let [a, b, c, d] = ip.octets();
                        let [p0, p1] = addr.port().to_be_bytes();
                        [a, b, c, d, p0, p1]
                    }
                    _ => panic!("only ipv4 is supported"),
                };
                let mut srl = vec![];
                srl.extend(type_srl);
                srl.extend(op_id_srl);
                srl.extend(op_srl);
                srl.extend(addr_srl);
                srl
            }
            Log { op_id, op } => {
                let type_srl = [MSG_TYPE_LOG];
                let op_id_srl = op_id.to_be_bytes();
                let op_srl: Vec<u8> = op.into();
                let mut srl = vec![];
                srl.extend(type_srl);
                srl.extend(op_id_srl);
                srl.extend(op_srl);
                srl
            }
            Ack { op_id } => {
                let type_srl = [MSG_TYPE_ACK];
                let op_id_srl = op_id.to_be_bytes();
                let mut srl = vec![];
                srl.extend(type_srl);
                srl.extend(op_id_srl);
                srl
            }
        }
    }
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

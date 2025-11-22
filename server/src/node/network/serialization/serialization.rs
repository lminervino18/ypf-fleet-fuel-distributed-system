use super::protocol::*;
use crate::node::message::Message;
use crate::node::message::Message::*;
use crate::node::operation::Operation;
use std::net::IpAddr;

impl From<Operation> for Vec<u8> {
    fn from(msg: Operation) -> Self {
        let id_srl = msg.id.to_be_bytes();
        let acc_id_srl = msg.account_id.to_be_bytes();
        let card_id_srl = msg.card_id.to_be_bytes();
        let amount = msg.amount.to_be_bytes();
        let mut srl = vec![];
        srl.extend(id_srl);
        srl.extend(acc_id_srl);
        srl.extend(card_id_srl);
        srl.extend(amount);
        srl
    }
}

impl From<Message> for Vec<u8> {
    fn from(msg: Message) -> Self {
        match msg {
            Request { op, addr } => {
                let type_srl = [TYPE_REQUEST];
                let op_srl: Vec<u8> = op.into();
                let addr_srl: [u8; 6] = match addr.ip() {
                    IpAddr::V4(ip) => {
                        let [a, b, c, d] = ip.octets();
                        let [p0, p1] = addr.port().to_be_bytes();
                        [a, b, c, d, p0, p1]
                    }
                    _ => panic!(), // sólo ipv4
                };
                let mut srl = vec![];
                srl.extend(type_srl);
                srl.extend(op_srl);
                srl.extend(addr_srl);
                srl
            }
            Log { op } => {
                let type_srl = [TYPE_LOG];
                let op_srl: Vec<u8> = op.into();
                let mut srl = vec![];
                srl.extend(type_srl);
                srl.extend(op_srl);
                srl
            }
            Ack { id } => {
                let type_srl = [TYPE_ACK];
                let id_srl = id.to_be_bytes();
                let mut srl = vec![];
                srl.extend(type_srl);
                srl.extend(id_srl);
                srl
            }
            RequestVote { term, candidate_id, candidate_addr } => {
                let type_srl = [TYPE_REQUEST_VOTE];
                let term_srl = term.to_be_bytes();
                let cand_id_srl = candidate_id.to_be_bytes();
                let addr_srl: [u8; 6] = match candidate_addr.ip() {
                    std::net::IpAddr::V4(ip) => {
                        let [a, b, c, d] = ip.octets();
                        let [p0, p1] = candidate_addr.port().to_be_bytes();
                        [a, b, c, d, p0, p1]
                    }
                    _ => panic!(),
                };

                let mut srl = vec![];
                srl.extend(type_srl);
                srl.extend(term_srl);
                srl.extend(cand_id_srl);
                srl.extend(addr_srl);
                srl
            }
            Vote { term, voter_id, voter_addr, granted } => {
                let type_srl = [TYPE_VOTE];
                let term_srl = term.to_be_bytes();
                let voter_id_srl = voter_id.to_be_bytes();
                let addr_srl: [u8; 6] = match voter_addr.ip() {
                    std::net::IpAddr::V4(ip) => {
                        let [a, b, c, d] = ip.octets();
                        let [p0, p1] = voter_addr.port().to_be_bytes();
                        [a, b, c, d, p0, p1]
                    }
                    _ => panic!(),
                };
                let granted_srl = [if granted { 1u8 } else { 0u8 }];

                let mut srl = vec![];
                srl.extend(type_srl);
                srl.extend(term_srl);
                srl.extend(voter_id_srl);
                srl.extend(addr_srl);
                srl.extend(granted_srl);
                srl
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_serialize_operation() {
        let op = Operation {
            id: 126,
            account_id: 348,
            card_id: 34821,
            amount: 80500.53,
        };

        let op_srl: Vec<u8> = op.into();
        let expected = [
            0x00, 0x00, 0x00, 0x7E, // 126
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x5C, // 348
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88, 0x05, // 34721
            0x47, 0x9d, 0x3a, 0x44, // 80500.53 IEEE 754 simple precision
        ];
        assert_eq!(op_srl, expected);
    }

    #[test]
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
        assert_eq!(op, expected);

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
}

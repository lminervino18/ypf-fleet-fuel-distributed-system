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
            _ => {
                todo!()
            }
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

use crate::Message;
use crate::Message::*;
use crate::errors::AppError;
use crate::errors::AppResult;
use crate::network::serials::protocol::*;
use std::net::SocketAddr;

impl TryFrom<Vec<u8>> for Message {
    type Error = AppError;

    fn try_from(payload: Vec<u8>) -> AppResult<Self> {
        match payload[0] {
            MSG_TYPE_REQUEST => deserialize_request_message(&payload[1..]),
            MSG_TYPE_LOG => deserialize_log_message(&payload[1..]),
            MSG_TYPE_ACK => deserialize_ack_message(&payload[1..]),
            _ => Err(AppError::InvalidData {
                details: format!(
                    "unknown node message type {}, with contents {:?}",
                    payload[0], payload
                ),
            }),
        }
    }
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::operation::Operation;

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
}

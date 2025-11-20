use super::helpers::{deserialize_socket_addr, read_bytes};
use crate::node::message::Message::*;
use crate::{errors::AppError, node::message::Message};

impl TryFrom<Vec<u8>> for Message {
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

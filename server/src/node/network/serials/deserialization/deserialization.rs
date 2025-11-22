use super::protocol::*;
use crate::errors::AppResult;
use crate::node::message::Message::*;
use crate::node::operation::Operation;
use crate::{errors::AppError, node::message::Message};
use std::net::SocketAddr;

impl TryFrom<Vec<u8>> for Message {
    type Error = AppError;

    fn try_from(payload: Vec<u8>) -> AppResult<Self> {
        match payload[0] {
            MSG_TYPE_REQUEST => {
                let op_srl = &payload[1..];
                let ip_srl: [u8; 4] =
                    payload[25..29]
                        .try_into()
                        .map_err(|e| AppError::InvalidProtocol {
                            details: format!(
                                "failed to read address ip bytes in request message: {e}"
                            ),
                        })?;
                let port_srl: [u8; 2] =
                    payload[29..31]
                        .try_into()
                        .map_err(|e| AppError::InvalidProtocol {
                            details: format!(
                                "failed to read address port bytes in request message: {e}"
                            ),
                        })?;
                let port = u16::from_be_bytes(port_srl);
                Ok(Request {
                    op: op_srl.try_into()?,
                    addr: SocketAddr::from((ip_srl, port)),
                })
            }
            MSG_TYPE_LOG => {
                let op_srl = &payload[1..];
                Ok(Log {
                    op: op_srl.try_into()?,
                })
            }
            MSG_TYPE_ACK => {
                let id_srl: [u8; 4] =
                    payload[1..5]
                        .try_into()
                        .map_err(|e| AppError::InvalidProtocol {
                            details: format!("failed to read id bytes in request message: {e}"),
                        })?;
                let id = u32::from_be_bytes(id_srl);
                Ok(Ack { op_id: id })
            }
            _ => Err(AppError::InvalidData {
                details: format!(
                    "unknown node message type {}, with contents {:?}",
                    payload[0], payload
                ),
            }),
        }
    }
}

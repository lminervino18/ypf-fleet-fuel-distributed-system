use super::protocol::*;
use crate::node::message::Message::*;
use crate::node::operation::Operation;
use crate::{errors::AppError, node::message::Message};
use std::net::SocketAddr;

// TODO: para no tener que slicear el payload a mano se podría agregar al protocolo
// que cada campo venga prefijado con su longitud en bytes
impl TryFrom<&[u8]> for Operation {
    type Error = AppError;

    fn try_from(payload: &[u8]) -> Result<Self, AppError> {
        let id_srl: [u8; 4] = payload[0..4]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to read id bytes in operation: {e}"),
            })?;
        let account_id_srl: [u8; 8] =
            payload[4..12]
                .try_into()
                .map_err(|e| AppError::InvalidProtocol {
                    details: format!("failed to read account id bytes in operation: {e}"),
                })?;
        let card_id_srl: [u8; 8] =
            payload[12..20]
                .try_into()
                .map_err(|e| AppError::InvalidProtocol {
                    details: format!("failed to read card id bytes in operation: {e}"),
                })?;
        let amount_srl: [u8; 4] =
            payload[20..24]
                .try_into()
                .map_err(|e| AppError::InvalidProtocol {
                    details: format!("failed to read amount bytes in operation: {e}"),
                })?;
        Ok(Operation {
            id: u32::from_be_bytes(id_srl),
            account_id: u64::from_be_bytes(account_id_srl),
            card_id: u64::from_be_bytes(card_id_srl),
            amount: f32::from_be_bytes(amount_srl),
        })
    }
}

impl TryFrom<Vec<u8>> for Message {
    type Error = AppError;

    fn try_from(payload: Vec<u8>) -> Result<Self, AppError> {
        match payload[0] {
            TYPE_REQUEST => {
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
            TYPE_LOG => {
                let op_srl = &payload[1..];
                Ok(Log {
                    op: op_srl.try_into()?,
                })
            }
            TYPE_ACK => {
                let id_srl: [u8; 4] =
                    payload[1..5]
                        .try_into()
                        .map_err(|e| AppError::InvalidProtocol {
                            details: format!("failed to read id bytes in request message: {e}"),
                        })?;
                let id = u32::from_be_bytes(id_srl);
                Ok(Ack { id })
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

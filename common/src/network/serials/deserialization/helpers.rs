use crate::{AppError, AppResult, network::serials::protocol::*};
use std::net::SocketAddr;

pub fn deserialize_limit(payload: &[u8]) -> AppResult<Option<f32>> {
    let limit = deserialize_amount(payload)?;
    Ok(match limit {
        NO_LIMIT => None,
        _ => Some(limit),
    })
}

pub fn deserialize_vec_len(payload: &[u8]) -> AppResult<usize> {
    Ok(usize::from_be_bytes(
        payload[0..VEC_LEN_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to deserialize vec_len: {e}"),
            })?,
    ))
}

pub fn deserialize_account_id(payload: &[u8]) -> AppResult<u64> {
    Ok(u64::from_be_bytes(
        payload[0..ACC_ID_SRL_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to deserialize account id: {e}"),
            })?,
    ))
}

pub fn deserialize_card_id(payload: &[u8]) -> AppResult<u64> {
    Ok(u64::from_be_bytes(
        payload[0..CARD_ID_SRL_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to deserialize card id: {e}"),
            })?,
    ))
}

pub fn deserialize_amount(payload: &[u8]) -> AppResult<f32> {
    Ok(f32::from_be_bytes(
        payload[0..AMOUNT_SRL_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to deserialize amount: {e}"),
            })?,
    ))
}

pub fn deserialize_socket_address_srl(payload: &[u8]) -> AppResult<SocketAddr> {
    if payload.len() < SOCKET_ADDR_LEN {
        return Err(AppError::InvalidProtocol {
            details: "not enough bytes to deserialize socket_address_srl".to_string(),
        });
    }

    let ip: [u8; 4] = payload[0..4]
        .try_into()
        .map_err(|e| AppError::InvalidProtocol {
            details: format!("failed to read address ip bytes: {e}"),
        })?;
    let port =
        u16::from_be_bytes(
            payload[4..6]
                .try_into()
                .map_err(|e| AppError::InvalidProtocol {
                    details: format!("failed to read address port bytes: {e}"),
                })?,
        );
    Ok(SocketAddr::from((ip, port)))
}

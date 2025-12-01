//! Deserialization helper utilities for the binary protocol.
//!
//! This module provides small, focused functions used by higher-level
//! TryFrom implementations to extract typed values from serialized byte
//! slices. Each helper validates input length and maps parsing errors into
//! `AppError::InvalidProtocol` where appropriate.

use crate::{AppError, AppResult, network::serials::protocol::*};
use std::net::SocketAddr;

/// Deserialize an optional limit value.
///
/// The protocol represents the absence of a limit with the sentinel `NO_LIMIT`.
/// Return `Ok(None)` when the sentinel is present, otherwise return the f32 value.
pub fn deserialize_limit(payload: &[u8]) -> AppResult<Option<f32>> {
    let limit = deserialize_amount(payload)?;
    Ok(match limit {
        NO_LIMIT => None,
        _ => Some(limit),
    })
}

/// Deserialize a vector length field.
///
/// The vector length is encoded as `VEC_LEN_LEN` bytes (big-endian).
/// Return the decoded length as `usize` or an `InvalidProtocol` error.
pub fn deserialize_vec_len(payload: &[u8]) -> AppResult<usize> {
    Ok(usize::from_be_bytes(
        payload[0..VEC_LEN_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to deserialize vec_len: {e}"),
            })?,
    ))
}

/// Deserialize an account identifier.
///
/// Account ids are encoded in `ACC_ID_SRL_LEN` bytes (big-endian u64).
pub fn deserialize_account_id(payload: &[u8]) -> AppResult<u64> {
    Ok(u64::from_be_bytes(
        payload[0..ACC_ID_SRL_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to deserialize account id: {e}"),
            })?,
    ))
}

/// Deserialize a card identifier.
///
/// Card ids are encoded in `CARD_ID_SRL_LEN` bytes (big-endian u64).
pub fn deserialize_card_id(payload: &[u8]) -> AppResult<u64> {
    Ok(u64::from_be_bytes(
        payload[0..CARD_ID_SRL_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to deserialize card id: {e}"),
            })?,
    ))
}

/// Deserialize a 32-bit floating point amount.
///
/// Amounts are encoded as big-endian `f32` occupying `AMOUNT_SRL_LEN` bytes.
pub fn deserialize_amount(payload: &[u8]) -> AppResult<f32> {
    Ok(f32::from_be_bytes(
        payload[0..AMOUNT_SRL_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to deserialize amount: {e}"),
            })?,
    ))
}

/// Deserialize a socket address from its serialized representation.
///
/// The function expects at least `SOCKET_ADDR_LEN` bytes. On success it
/// returns a `SocketAddr` constructed from the first 4 bytes (IPv4) and the
/// subsequent 2 bytes (port) in big-endian order.
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

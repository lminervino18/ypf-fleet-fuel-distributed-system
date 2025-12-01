//! Deserialization helpers for database snapshots.
//!
//! This module implements `TryFrom` conversions from serialized byte payloads
//! into the in-memory snapshot types used by the protocol:
//! - `DatabaseSnapshot` (owned `Vec<u8>` -> `DatabaseSnapshot`)
//! - `AccountSnapshot` (`&[u8]` -> `AccountSnapshot`)
//! - `CardSnapshot` (`&[u8]` -> `CardSnapshot`)
//!
//! Frame layout and helper functions are provided by the `serials::helpers` and
//! `serials::protocol` modules. Each `TryFrom` implementation reads fields in
//! the defined order and advances a local pointer (`ptr`) over the payload.
use super::{deserialize_socket_address_srl, helpers::deserialize_card_id};
use crate::{
    AppError, AppResult,
    network::serials::{
        deserialization::helpers::{
            deserialize_account_id, deserialize_amount, deserialize_limit, deserialize_vec_len,
        },
        protocol::{
            ACC_ID_SRL_LEN, ACC_SNAPSHOT_SRL_LEN, AMOUNT_SRL_LEN, CARD_ID_SRL_LEN,
            CARD_SNAPSHOT_SRL_LEN, SOCKET_ADDR_LEN, VEC_LEN_LEN,
        },
    },
    operation::{AccountSnapshot, CardSnapshot, DatabaseSnapshot},
};

/// Deserialize a `DatabaseSnapshot` from an owned byte vector.
///
/// The expected serialized layout is:
/// - socket address (fixed length),
/// - vector length for accounts,
/// - sequence of account snapshots (each fixed-length),
/// - vector length for cards,
/// - sequence of card snapshots (each fixed-length).
impl TryFrom<Vec<u8>> for DatabaseSnapshot {
    type Error = AppError;

    fn try_from(payload: Vec<u8>) -> AppResult<Self> {
        let mut ptr = 0;
        let addr = deserialize_socket_address_srl(&payload[ptr..])?;
        ptr += SOCKET_ADDR_LEN;
        let accounts_len = deserialize_vec_len(&payload[ptr..])?;
        ptr += VEC_LEN_LEN;
        let mut accounts = vec![];
        for _ in 0..accounts_len {
            let account: AccountSnapshot = payload[ptr..].try_into()?;
            ptr += ACC_SNAPSHOT_SRL_LEN;
            accounts.push(account);
        }

        let cards_len = deserialize_vec_len(&payload[ptr..])?;
        ptr += VEC_LEN_LEN;
        let mut cards = vec![];
        for _ in 0..cards_len {
            let card: CardSnapshot = payload[ptr..].try_into()?;
            ptr += CARD_SNAPSHOT_SRL_LEN;
            cards.push(card);
        }

        Ok(DatabaseSnapshot {
            addr,
            accounts,
            cards,
        })
    }
}

/// Deserialize an `AccountSnapshot` from a byte slice.
///
/// Expected layout for an account snapshot:
/// - account id (fixed length),
/// - optional limit (serialized as AMOUNT_SRL_LEN),
/// - consumed amount.
impl TryFrom<&[u8]> for AccountSnapshot {
    type Error = AppError;

    fn try_from(payload: &[u8]) -> AppResult<Self> {
        let mut ptr = 0;
        let account_id = deserialize_account_id(&payload[ptr..])?;
        ptr += ACC_ID_SRL_LEN;
        let limit = deserialize_limit(&payload[ptr..])?;
        ptr += AMOUNT_SRL_LEN;
        let consumed = deserialize_amount(&payload[ptr..])?;
        Ok(AccountSnapshot {
            account_id,
            limit,
            consumed,
        })
    }
}

/// Deserialize a `CardSnapshot` from a byte slice.
///
/// Expected layout for a card snapshot:
/// - account id (fixed length),
/// - card id (fixed length),
/// - optional limit (serialized as AMOUNT_SRL_LEN),
/// - consumed amount.
impl TryFrom<&[u8]> for CardSnapshot {
    type Error = AppError;

    fn try_from(payload: &[u8]) -> AppResult<Self> {
        let mut ptr = 0;
        let account_id = deserialize_account_id(&payload[ptr..])?;
        ptr += ACC_ID_SRL_LEN;
        let card_id = deserialize_card_id(&payload[ptr..])?;
        ptr += CARD_ID_SRL_LEN;
        let limit = deserialize_limit(&payload[ptr..])?;
        ptr += AMOUNT_SRL_LEN;
        let consumed = deserialize_amount(&payload[ptr..])?;
        Ok(CardSnapshot {
            account_id,
            card_id,
            limit,
            consumed,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::net::SocketAddr;

    #[test]
    fn serialize_and_deserialize_database_snapshot() {
        let database = DatabaseSnapshot {
            addr: SocketAddr::from(([127, 0, 0, 1], 12346)),
            accounts: vec![
                AccountSnapshot {
                    account_id: 235,
                    limit: Some(13249998f32),
                    consumed: 9500345f32,
                },
                AccountSnapshot {
                    account_id: 864,
                    limit: Some(1000f32),
                    consumed: 500f32,
                },
                AccountSnapshot {
                    account_id: 1,
                    limit: Some(123457f32),
                    consumed: 123456f32,
                },
            ],
            cards: vec![
                CardSnapshot {
                    account_id: 1,
                    card_id: 1523,
                    limit: Some(13249998f32),
                    consumed: 9500345f32,
                },
                CardSnapshot {
                    account_id: 864,
                    card_id: 999,
                    limit: Some(1000f32),
                    consumed: 500f32,
                },
                CardSnapshot {
                    account_id: 1000,
                    card_id: 3945,
                    limit: Some(123457f32),
                    consumed: 123456f32,
                },
            ],
        };
        let database_srl: Vec<u8> = database.clone().into();
        let expected = Ok(database);
        let database = database_srl.try_into();
        assert_eq!(database, expected);
    }
}

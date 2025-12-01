//! Operation deserialization utilities.
//!
//! Implement `TryFrom<&[u8]> for Operation` by dispatching on the leading
//! operation type byte and delegating to per-operation deserializers.
//!
//! Each deserializer reads fixed-size fields in order, advancing a local
//! pointer (`ptr`) over the payload and using helper functions for complex
//! substructures (socket addresses, ids, amounts, limits).
use super::{
    deserialize_socket_address_srl,
    helpers::{deserialize_account_id, deserialize_amount, deserialize_card_id, deserialize_limit},
};
use crate::{
    errors::{AppError, AppResult},
    network::serials::protocol::*,
    operation::Operation,
};

impl TryFrom<&[u8]> for Operation {
    type Error = AppError;

    fn try_from(payload: &[u8]) -> AppResult<Self> {
        match payload[0] {
            OP_TYPE_CHARGE => deserialize_charge_operation(&payload[1..]),
            OP_TYPE_LIMIT_ACCOUNT => deserialize_limit_account_operation(&payload[1..]),
            OP_TYPE_LIMIT_CARD => deserialize_limit_card_operation(&payload[1..]),
            OP_TYPE_QUERY_ACCOUNT => deserialize_query_account_operation(&payload[1..]),
            OP_TYPE_BILL => deserialize_bill_operation(&payload[1..]),
            OP_GET_DATABASE => deserialize_get_database_operation(&payload[1..]),
            OP_REPLACE_DATABASE => deserialize_replace_database_operation(&payload[1..]),
            _ => Err(AppError::InvalidProtocol {
                details: format!("unknown operation type {:?}", payload[0]),
            }),
        }
    }
}

fn deserialize_replace_database_operation(payload: &[u8]) -> AppResult<Operation> {
    // Convert remaining bytes into an owned snapshot for ReplaceDatabase.
    let snapshot = payload.to_vec().try_into()?;
    Ok(Operation::ReplaceDatabase { snapshot })
}

fn deserialize_get_database_operation(payload: &[u8]) -> AppResult<Operation> {
    // Deserialize a socket address that identifies the requested database owner.
    let addr = deserialize_socket_address_srl(payload)?;
    Ok(Operation::GetDatabase { addr })
}

/// Deserialize the `from_offline_station` boolean attribute.
///
/// The attribute is encoded using a single byte with values `TRUE` or `FALSE`.
fn deserialize_from_offline_station(payload: &[u8]) -> AppResult<bool> {
    Ok(match payload[0..OFFLINE_SRL_LEN] {
        [TRUE] => true,
        [FALSE] => false,
        _ => {
            return Err(AppError::InvalidProtocol {
                details: "invalid bytes for `from_offline_station` attribute in charge operation"
                    .to_string(),
            });
        }
    })
}

/// Deserialize a Charge operation.
///
/// Expected serialized layout (CHARGE_SRL_LEN bytes):
/// - account_id (ACC_ID_SRL_LEN)
/// - card_id (CARD_ID_SRL_LEN)
/// - amount (AMOUNT_SRL_LEN)
/// - from_offline_station (OFFLINE_SRL_LEN)
fn deserialize_charge_operation(payload: &[u8]) -> AppResult<Operation> {
    if payload.len() != CHARGE_SRL_LEN {
        return Err(AppError::InvalidProtocol {
            details: "not enough bytes to deserialize charge operation".to_string(),
        });
    }

    let mut ptr = 0;
    let account_id = deserialize_account_id(&payload[ptr..])?;
    ptr += ACC_ID_SRL_LEN;
    let card_id = deserialize_card_id(&payload[ptr..])?;
    ptr += CARD_ID_SRL_LEN;
    let amount = deserialize_amount(&payload[ptr..])?;
    ptr += AMOUNT_SRL_LEN;
    let from_offline_station = deserialize_from_offline_station(&payload[ptr..])?;
    Ok(Operation::Charge {
        account_id,
        card_id,
        amount,
        from_offline_station,
    })
}

/// Deserialize a LimitAccount operation.
///
/// Expected serialized layout (LIMIT_ACC_SRL_LEN bytes):
/// - account_id (ACC_ID_SRL_LEN)
/// - new_limit (AMOUNT_SRL_LEN or sentinel for None)
fn deserialize_limit_account_operation(payload: &[u8]) -> AppResult<Operation> {
    if payload.len() != LIMIT_ACC_SRL_LEN {
        return Err(AppError::InvalidProtocol {
            details: "not enough bytes to deserialize limit account operation".to_string(),
        });
    }

    let mut ptr = 0;
    let account_id = deserialize_account_id(&payload[ptr..])?;
    ptr += ACC_ID_SRL_LEN;
    let new_limit = deserialize_limit(&payload[ptr..])?;
    Ok(Operation::LimitAccount {
        account_id,
        new_limit,
    })
}

/// Deserialize a LimitCard operation.
///
/// Expected serialized layout (LIMIT_CARD_SRL_LEN bytes):
/// - account_id (ACC_ID_SRL_LEN)
/// - card_id (CARD_ID_SRL_LEN)
/// - new_limit (AMOUNT_SRL_LEN or sentinel)
fn deserialize_limit_card_operation(payload: &[u8]) -> AppResult<Operation> {
    if payload.len() != LIMIT_CARD_SRL_LEN {
        return Err(AppError::InvalidProtocol {
            details: "not enough bytes to deserialize limit card operation".to_string(),
        });
    }

    let mut ptr = 0;
    let account_id = deserialize_account_id(&payload[ptr..])?;
    ptr += ACC_ID_SRL_LEN;
    let card_id = deserialize_card_id(&payload[ptr..])?;
    ptr += CARD_ID_SRL_LEN;
    let new_limit = deserialize_limit(&payload[ptr..])?;
    Ok(Operation::LimitCard {
        account_id,
        card_id,
        new_limit,
    })
}

/// Deserialize an AccountQuery operation.
///
/// The payload contains only the account id.
fn deserialize_query_account_operation(payload: &[u8]) -> AppResult<Operation> {
    if payload.len() != ACC_ID_SRL_LEN {
        return Err(AppError::InvalidProtocol {
            details: "not enough bytes to deserialize query account operation".to_string(),
        });
    }

    let account_id = deserialize_account_id(&payload[0..])?;
    Ok(Operation::AccountQuery { account_id })
}

/// Deserialize a Bill operation.
///
/// Layout:
/// - account_id (ACC_ID_SRL_LEN)
/// - optional period string (UTF-8). If present, extract and return its substring.
fn deserialize_bill_operation(payload: &[u8]) -> AppResult<Operation> {
    if payload.len() < ACC_ID_SRL_LEN {
        return Err(AppError::InvalidProtocol {
            details: "not enough bytes to deserialize bill operation".to_string(),
        });
    }

    let mut ptr = 0;
    let account_id = deserialize_account_id(&payload[ptr..])?;
    ptr += ACC_ID_SRL_LEN;
    let period = if payload.len() > ptr {
        let period_bytes = &payload[ptr..];
        let byte_of_len_size = 1;
        match std::str::from_utf8(period_bytes) {
            Ok(period_str) => match period_str.to_string() {
                ref s if s == "\0" => None,
                s => Some(s[byte_of_len_size..].to_string()),
            },
            Err(e) => {
                return Err(AppError::InvalidProtocol {
                    details: format!("failed to deserialize period string in bill operation: {e}"),
                });
            }
        }
    } else {
        None
    };

    Ok(Operation::Bill { account_id, period })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_deserialize_valid_account_id() {
        let account_id_srl = 34586u64.to_be_bytes();
        let expected = Ok(34586);
        let account_id = deserialize_account_id(&account_id_srl);
        assert_eq!(account_id, expected);
    }

    #[test]
    fn test_deserialize_valid_card_id() {
        let card_id_srl = 34586u64.to_be_bytes();
        let expected = Ok(34586);
        let account_id = deserialize_card_id(&card_id_srl);
        assert_eq!(account_id, expected);
    }

    #[test]
    fn test_deserialize_valid_amount() {
        let amount_srl = 493583.5f32.to_be_bytes();
        let expected = Ok(493583.5f32);
        let amount = deserialize_amount(&amount_srl);
        assert_eq!(amount, expected);
    }

    #[test]
    fn test_deserialize_valid_some_limit() {
        let limit_srl = 543000.8f32.to_be_bytes();
        let expected = Ok(Some(543000.8f32));
        let limit = deserialize_limit(&limit_srl);
        assert_eq!(limit, expected);
    }

    #[test]
    fn test_deserialize_valid_none_limit() {
        let limit_srl = NO_LIMIT.to_be_bytes();
        let expected = Ok(None);
        let limit = deserialize_limit(&limit_srl);
        assert_eq!(limit, expected);
    }

    #[test]
    fn test_deserialize_valid_charge_operation() {
        let op = Operation::Charge {
            account_id: 10012,
            card_id: 15333,
            amount: 15864.63,
            from_offline_station: true,
        };
        let op_srl: Vec<u8> = op.clone().into();
        let expected = Ok(op);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }

    #[test]
    fn test_deserialize_valid_limit_account_operation() {
        let op = Operation::LimitAccount {
            account_id: 15388,
            new_limit: Some(942000.8),
        };
        let op_srl: Vec<u8> = op.clone().into();
        let expected = Ok(op);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }

    #[test]
    fn test_deserialize_valid_limit_card_operation() {
        let op = Operation::LimitCard {
            account_id: 15388,
            card_id: 2,
            new_limit: None,
        };
        let op_srl: Vec<u8> = op.clone().into();
        let expected = Ok(op);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }

    #[test]
    fn test_deserialize_valid_query_account_operation() {
        let op = Operation::AccountQuery { account_id: 5000 };
        let op_srl: Vec<u8> = op.clone().into();
        let expected = Ok(op);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }

    #[test]
    fn test_deserialize_valid_bill_operation_with_period() {
        let op = Operation::Bill {
            account_id: 7000,
            period: Some("2025-10".to_string()),
        };
        let op_srl: Vec<u8> = op.clone().into();
        let expected = Ok(op);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }

    #[test]
    fn test_deserialize_valid_bill_operation_without_period() {
        let op = Operation::Bill {
            account_id: 7000,
            period: None,
        };
        let op_srl: Vec<u8> = op.clone().into();
        let expected = Ok(op);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }
}

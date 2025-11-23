use super::helpers::{deserialize_account_id, deserialize_amount, deserialize_card_id};
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
            _ => Err(AppError::InvalidProtocol {
                details: format!("unknown operation type {:?}", payload[0]),
            }),
        }
    }
}

// attribute deserialization
fn deserialize_limit(payload: &[u8]) -> AppResult<Option<f32>> {
    let limit = deserialize_amount(payload)?;
    Ok(match limit {
        NO_LIMIT => None,
        _ => Some(limit),
    })
}

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

// operations deserialization
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

fn deserialize_query_account_operation(payload: &[u8]) -> AppResult<Operation> {
    if payload.len() != ACC_ID_SRL_LEN {
        return Err(AppError::InvalidProtocol {
            details: "not enough bytes to deserialize query account operation".to_string(),
        });
    }

    let account_id = deserialize_account_id(&payload[0..])?;
    Ok(Operation::AccountQuery { account_id })
}

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

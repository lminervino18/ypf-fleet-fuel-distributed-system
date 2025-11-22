use crate::node::network::serials::protocol::*;

impl TryFrom<&[u8]> for Operation {
    type Error = AppError;

    fn try_from(payload: &[u8]) -> AppResult<Self> {
        match payload[0] {
            OP_TYPE_CHARGE => deserialize_charge_operation(&payload[1..]),
            OP_TYPE_LIMIT_ACCOUNT => deserialize_limit_account_operation(&payload[1..]),
            OP_TYPE_LIMIT_CARD => deserialize_limit_card_operation(&payload[1..]),
            _ => Err(AppError::InvalidProtocol {
                details: format!("unknown operation type {:?}", payload),
            }),
        }
    }
}

// attribute deserialization
fn deserialize_account_id(payload: &[u8]) -> AppResult<u64> {
    Ok(u64::from_be_bytes(
        payload[0..ACC_ID_SRL_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to deserialize account id in charge operation: {e}"),
            })?,
    ))
}

fn deserialize_card_id(payload: &[u8]) -> AppResult<u64> {
    Ok(u64::from_be_bytes(
        payload[0..CARD_ID_SRL_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to deserialize card id in charge operation: {e}"),
            })?,
    ))
}

fn deserialize_amount(payload: &[u8]) -> AppResult<f32> {
    Ok(f32::from_be_bytes(
        payload[0..AMOUNT_SRL_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!("failed to deserialize amount in charge operation: {e}"),
            })?,
    ))
}

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
            })
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

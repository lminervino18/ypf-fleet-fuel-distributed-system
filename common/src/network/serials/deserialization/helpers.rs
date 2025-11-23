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

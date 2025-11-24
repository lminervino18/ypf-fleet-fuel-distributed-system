use super::helpers::{deserialize_account_id, deserialize_amount, deserialize_card_id};
use crate::{
    AppError, AppResult, LimitCheckError, LimitUpdateError, VerifyError,
    network::serials::protocol::*,
    operation_result::{
        AccountQueryResult, ChargeResult, LimitResult,
        OperationResult::{self, *},
    },
};

/* pub const CHARGE_RESULT: u8 = 0x00;
pub const LIMIT_ACCOUNT_RESULT: u8 = 0x02;
pub const LIMIT_CARD_RESULT: u8 = 0x03;
pub const ACC_QUERY_RESULT: u8 = 0x04; */
impl TryFrom<&[u8]> for OperationResult {
    type Error = AppError;

    fn try_from(payload: &[u8]) -> AppResult<Self> {
        match payload[0] {
            CHARGE_RESULT => deserialize_charge_result(&payload[1..]),
            LIMIT_ACCOUNT_RESULT => deserialize_limit_account_result(&payload[1..]),
            LIMIT_CARD_RESULT => deserialize_limit_card_result(&payload[1..]),
            ACC_QUERY_RESULT => deserialize_account_query_result(&payload[1..]),
            _ => Err(AppError::InvalidProtocol {
                details: format!("unknown operation result type {:?}", payload[0]),
            }),
        }
    }
}

fn deserialize_card_spent(payload: &[u8]) -> AppResult<(u64, f32)> {
    let mut ptr = 0;
    let card_id = deserialize_card_id(&payload[ptr..])?;
    ptr += CARD_ID_SRL_LEN;
    let amount = deserialize_amount(&payload[ptr..])?;
    Ok((card_id, amount))
}

fn deserialize_account_query_result(payload: &[u8]) -> AppResult<OperationResult> {
    let mut ptr = 0;
    let account_id = deserialize_account_id(&payload[ptr..])?;
    ptr += ACC_ID_SRL_LEN;
    let total_spent = deserialize_amount(&payload[ptr..])?;
    ptr += AMOUNT_SRL_LEN;
    let per_card_spent_len = usize::from_be_bytes(
        payload[ptr..ptr + PER_CARD_SPENT_LEN]
            .try_into()
            .map_err(|e| AppError::InvalidProtocol {
                details: format!(
                    "failed to deserialize len of per_card_spent in account_query_result: {e}"
                ),
            })?,
    );

    ptr += PER_CARD_SPENT_LEN;
    let mut per_card_spent = vec![];
    for _ in 0..per_card_spent_len {
        per_card_spent.push(deserialize_card_spent(&payload[ptr..])?);
        ptr += CARD_SPENT_LEN;
    }

    Ok(AccountQuery(AccountQueryResult {
        account_id,
        total_spent,
        per_card_spent,
    }))
}

fn deserialize_limit_card_result(payload: &[u8]) -> AppResult<OperationResult> {
    match payload[0] {
        OK => Ok(LimitCard(LimitResult::Ok)),
        ERR => todo!(),
        _ => Err(AppError::InvalidProtocol {
            details: format!("unknown limit card result {}", payload[0]),
        }),
    }
}

fn deserialize_limit_check_error(payload: &[u8]) -> AppResult<LimitCheckError> {
    if payload.len() < LIMIT_CHECK_ERR_LEN {
        return Err(AppError::InvalidProtocol {
            details: format!(
                "not enough bytes to deserialize limit_check_error: {:?}",
                payload
            ),
        });
    }

    match payload[0] {
        ACC_LIMIT_EXCEEDED => Ok(LimitCheckError::AccountLimitExceeded),
        CARD_LIMIT_EXCEEDED => Ok(LimitCheckError::CardLimitExceeded),
        _ => Err(AppError::InvalidProtocol {
            details: format!("unknown limit_check_error: {}", payload[0]),
        }),
    }
}

fn deserialize_limit_update_error(payload: &[u8]) -> AppResult<LimitUpdateError> {
    match payload[0] {
        BELOW_CURRENT_USAGE => Ok(LimitUpdateError::BelowCurrentUsage),
        _ => Err(AppError::InvalidProtocol {
            details: format!("unknown limit_update_error: {}", payload[2]),
        }),
    }
}

fn deserialize_verify_error(payload: &[u8]) -> AppResult<VerifyError> {
    match payload[0] {
        CHARGE_LIMIT_ERR => Ok(VerifyError::ChargeLimit(deserialize_limit_check_error(
            &payload[1..],
        )?)),
        LIMIT_UPDATE_ERR => Ok(VerifyError::LimitUpdate(deserialize_limit_update_error(
            &payload[1..],
        )?)),
        _ => Err(AppError::InvalidProtocol {
            details: format!("unknown limit card err {}", payload[1]),
        }),
    }
}

fn deserialize_limit_account_result(payload: &[u8]) -> AppResult<OperationResult> {
    match payload[0] {
        OK => Ok(LimitAccount(LimitResult::Ok)),
        ERR => Ok(LimitAccount(LimitResult::Failed(deserialize_verify_error(
            &payload[1..],
        )?))),
        _ => Err(AppError::InvalidProtocol {
            details: format!("unknown limit card result {}", payload[0]),
        }),
    }
}

fn deserialize_charge_result(payload: &[u8]) -> AppResult<OperationResult> {
    match payload[0] {
        OK => Ok(Charge(ChargeResult::Ok)),
        ERR => Ok(Charge(ChargeResult::Failed(deserialize_verify_error(
            &payload[1..],
        )?))),
        _ => Err(AppError::InvalidProtocol {
            details: format!("unknown charge result {}", payload[0]),
        }),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_deserialize_account_query_result() {
        let op_result = OperationResult::AccountQuery(AccountQueryResult {
            account_id: 128,
            total_spent: 234592.3,
            per_card_spent: vec![(15300, 8503.4), (6003, 18443.9), (203, 503.0)], // y el resto?!
        });
        let op_srl: Vec<u8> = op_result.clone().into();
        let expected = Ok(op_result);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }

    #[test]
    fn test_deserialize_limit_account_result_ok() {
        let op_result = OperationResult::LimitAccount(LimitResult::Ok);
        let op_srl: Vec<u8> = op_result.clone().into();
        let expected = Ok(op_result);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }

    #[test]
    fn test_deserialize_limit_account_result_below_current_usage() {
        let op_result = OperationResult::LimitAccount(LimitResult::Failed(
            VerifyError::LimitUpdate(LimitUpdateError::BelowCurrentUsage),
        ));
        let op_srl: Vec<u8> = op_result.clone().into();
        let expected = Ok(op_result);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }

    #[test]
    fn test_deserialize_limit_account_result_card_limit_exceeded() {
        let op_result = OperationResult::LimitAccount(LimitResult::Failed(
            VerifyError::ChargeLimit(LimitCheckError::CardLimitExceeded),
        ));
        let op_srl: Vec<u8> = op_result.clone().into();
        let expected = Ok(op_result);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }

    #[test]
    fn test_deserialize_charge_result_ok() {
        let op_result = OperationResult::Charge(ChargeResult::Ok);
        let op_srl: Vec<u8> = op_result.clone().into();
        let expected = Ok(op_result);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }

    #[test]
    fn test_deserialize_charge_result_account_limit_exceeded() {
        let op_result = OperationResult::Charge(ChargeResult::Failed(VerifyError::ChargeLimit(
            LimitCheckError::AccountLimitExceeded,
        )));
        let op_srl: Vec<u8> = op_result.clone().into();
        let expected = Ok(op_result);
        let op = op_srl[..].try_into();
        assert_eq!(op, expected);
    }
}

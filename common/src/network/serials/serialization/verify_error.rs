use crate::LimitCheckError::*;
use crate::LimitUpdateError::*;
use crate::VerifyError;
use crate::VerifyError::*;
use crate::network::serials::protocol::*;

impl From<VerifyError> for Vec<u8> {
    fn from(error: VerifyError) -> Self {
        match error {
            ChargeLimit(error) => serialize_charge_limit_error(error),
            LimitUpdate(error) => serialize_limit_update_error(error),
        }
    }
}

fn serialize_limit_update_error(error: crate::LimitUpdateError) -> Vec<u8> {
    let type_srl = LIMIT_UPDATE_ERR;
    let mut srl = vec![];
    srl.push(type_srl);
    match error {
        BelowCurrentUsage => srl.push(BELOW_CURRENT_USAGE),
    }

    srl
}

fn serialize_charge_limit_error(error: crate::LimitCheckError) -> Vec<u8> {
    let type_srl = CHARGE_LIMIT_ERR;
    let mut srl = vec![];
    srl.push(type_srl);
    match error {
        CardLimitExceeded => srl.push(CARD_LIMIT_EXCEEDED),
        AccountLimitExceeded => srl.push(ACC_LIMIT_EXCEEDED),
    }

    srl
}

use crate::network::serials::protocol::*;
use crate::operation_result::OperationResult::*;
use crate::operation_result::*;

impl From<OperationResult> for Vec<u8> {
    fn from(op_result: OperationResult) -> Self {
        match op_result {
            Charge(result) => serialize_charge_result(result),
            LimitAccount(result) => serialize_limit_account_result(result),
            LimitCard(result) => serialize_limit_card_result(result),
            AccountQuery(result) => serialize_account_query_result(result),
        }
    }
}

fn serialize_card_spent(card_spent: (u64, f32)) -> Vec<u8> {
    let card_id_srl = card_spent.0.to_be_bytes();
    let spent_srl = card_spent.1.to_be_bytes();
    let mut srl = vec![];
    srl.extend(card_id_srl);
    srl.extend(spent_srl);
    srl
}

fn serialize_account_query_result(result: AccountQueryResult) -> Vec<u8> {
    let type_srl = [CHARGE_RESULT];
    let account_id_srl = result.account_id.to_be_bytes();
    let total_spent_srl = result.total_spent.to_be_bytes();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(account_id_srl);
    srl.extend(total_spent_srl);
    for card_spent in result.per_card_spent {
        srl.extend(serialize_card_spent(card_spent));
    }

    srl
}

fn serialize_limit_card_result(result: LimitResult) -> Vec<u8> {
    let type_srl = LIMIT_CARD_RESULT;
    let mut srl = vec![];
    srl.push(type_srl);
    srl.extend(serialize_limit_result(result));
    srl
}

fn serialize_limit_account_result(result: LimitResult) -> Vec<u8> {
    let type_srl = LIMIT_ACCOUNT_RESULT;
    let mut srl = vec![];
    srl.push(type_srl);
    srl.extend(serialize_limit_result(result));
    srl
}

fn serialize_charge_result(result: ChargeResult) -> Vec<u8> {
    let type_srl = LIMIT_ACCOUNT_RESULT;
    let mut srl = vec![];
    srl.push(type_srl);
    match result {
        ChargeResult::Ok => srl.push(OK),
        ChargeResult::Failed(error) => {
            srl.push(ERR);
            let error_srl: Vec<u8> = error.into();
            srl.extend(error_srl);
        }
    }

    srl
}

fn serialize_limit_result(result: LimitResult) -> Vec<u8> {
    let mut limit_result_srl = vec![];
    match result {
        LimitResult::Ok => limit_result_srl.push(OK),
        LimitResult::Failed(error) => {
            limit_result_srl.push(ERR);
            let error_srl: Vec<u8> = error.into();
            limit_result_srl.extend(error_srl);
        }
    }

    limit_result_srl
}

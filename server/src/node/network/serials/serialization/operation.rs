use crate::node::network::serials::protocol::*;
use crate::node::operation::Operation;
use crate::node::operation::Operation::*;

impl From<Operation> for Vec<u8> {
    fn from(op: Operation) -> Self {
        match op {
            Charge {
                account_id,
                card_id,
                amount,
                from_offline_station,
            } => serialize_charge_operation(account_id, card_id, amount, from_offline_station),
            LimitAccount {
                account_id,
                new_limit,
            } => serialize_limit_account_operation(account_id, new_limit),
            LimitCard {
                account_id,
                card_id,
                new_limit,
            } => serialize_limit_card_operation(account_id, card_id, new_limit),
        }
    }
}

fn serialize_charge_operation(
    account_id: u64,
    card_id: u64,
    amount: f32,
    from_offline_station: bool,
) -> Vec<u8> {
    let type_srl = [OP_TYPE_CHARGE];
    let acc_id_srl = account_id.to_be_bytes();
    let card_id_srl = card_id.to_be_bytes();
    let amount_srl = amount.to_be_bytes();
    let offline_srl = if from_offline_station {
        TRUE.to_be_bytes()
    } else {
        FALSE.to_be_bytes()
    };
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(acc_id_srl);
    srl.extend(card_id_srl);
    srl.extend(amount_srl);
    srl.extend(offline_srl);
    srl
}

fn serialize_limit_account_operation(account_id: u64, new_limit: Option<f32>) -> Vec<u8> {
    let type_srl = [OP_TYPE_LIMIT_ACCOUNT];
    let acc_id_srl = account_id.to_be_bytes();
    let new_limit_srl = new_limit.unwrap_or(NO_LIMIT).to_be_bytes();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(acc_id_srl);
    srl.extend(new_limit_srl);
    srl
}

fn serialize_limit_card_operation(
    account_id: u64,
    card_id: u64,
    new_limit: Option<f32>,
) -> Vec<u8> {
    let type_srl = [OP_TYPE_LIMIT_CARD];
    let acc_id_srl = account_id.to_be_bytes();
    let card_id_srl = card_id.to_be_bytes();
    let new_limit_srl = new_limit.unwrap_or(NO_LIMIT).to_be_bytes();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(acc_id_srl);
    srl.extend(card_id_srl);
    srl.extend(new_limit_srl);
    srl
}

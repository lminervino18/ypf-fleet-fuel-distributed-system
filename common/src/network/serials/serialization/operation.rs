use crate::network::serials::protocol::*;
use crate::operation::Operation;
use crate::operation::Operation::*;

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
            QueryAccount { account_id } => {
                serialize_query_account_operation(account_id)
            }
            QueryCards { account_id } => {
                serialize_query_cards_operation(account_id)
            }
            QueryCard { account_id, card_id } => {
                serialize_query_card_operation(account_id, card_id)
            }
            Bill { account_id, period } => {
                serialize_bill_operation(account_id, period)
            }
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

fn serialize_query_account_operation(account_id: u64) -> Vec<u8> {
    let type_srl = [OP_TYPE_QUERY_ACCOUNT];
    let acc_id_srl = account_id.to_be_bytes();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(acc_id_srl);
    srl
}

fn serialize_query_cards_operation(account_id: u64) -> Vec<u8> {
    let type_srl = [OP_TYPE_QUERY_CARDS];
    let acc_id_srl = account_id.to_be_bytes();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(acc_id_srl);
    srl
}

fn serialize_query_card_operation(account_id: u64, card_id: u64) -> Vec<u8> {
    let type_srl = [OP_TYPE_QUERY_CARD];
    let acc_id_srl = account_id.to_be_bytes();
    let card_id_srl = card_id.to_be_bytes();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(acc_id_srl);
    srl.extend(card_id_srl);
    srl
}

fn serialize_bill_operation(account_id: u64, period: Option<String>) -> Vec<u8> {
    let type_srl = [OP_TYPE_BILL];
    let acc_id_srl = account_id.to_be_bytes();
    let mut srl = vec![];
    srl.extend(type_srl);
    srl.extend(acc_id_srl);
    if let Some(period_str) = period {
        let period_bytes = period_str.as_bytes();
        let period_len = period_bytes.len() as u8;
        srl.push(period_len);
        srl.extend(period_bytes);
    } else {
        srl.push(0); // length 0 indicates no period
    }
    srl
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_charge_operation() {
        let account_id = 123456789u64;
        let card_id = 987654321u64;
        let amount = 50.0f32;
        let from_offline_station = true;
        let serialized =
            serialize_charge_operation(account_id, card_id, amount, from_offline_station);
        let expected: Vec<u8> = vec![
            OP_TYPE_CHARGE,
            0, 0, 0, 0, 7, 91, 205, 21,
            0, 0, 0, 0, 58, 222, 104, 177,
            66, 72, 0, 0,
            TRUE,
        ];
        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_serialize_limit_account_operation() {
        let account_id = 123456789u64;
        let new_limit = Some(100.0f32);
        let serialized = serialize_limit_account_operation(account_id, new_limit);
        let expected: Vec<u8> = vec![
            OP_TYPE_LIMIT_ACCOUNT,
            0, 0, 0, 0, 7, 91, 205, 21,
            66, 200, 0, 0,
        ];
        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_serialize_limit_card_operation() {
        let account_id = 123456789u64;
        let card_id = 987654321u64;
        let new_limit = Some(200.0f32);
        let serialized = serialize_limit_card_operation(account_id, card_id, new_limit);
        let expected: Vec<u8> = vec![
            OP_TYPE_LIMIT_CARD,
            0, 0, 0, 0, 7, 91, 205, 21,
            0, 0, 0, 0, 58, 222, 104, 177,
            67, 72, 0, 0,
        ];
        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_serialize_query_account_operation() {
        let account_id = 123456789u64;
        let serialized = serialize_query_account_operation(account_id);
        let expected: Vec<u8> = vec![
            OP_TYPE_QUERY_ACCOUNT,
            0, 0, 0, 0, 7, 91, 205, 21,
        ];
        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_serialize_query_cards_operation() {
        let account_id = 123456789u64;
        let serialized = serialize_query_cards_operation(account_id);
        let expected: Vec<u8> = vec![
            OP_TYPE_QUERY_CARDS,
            0, 0, 0, 0, 7, 91, 205, 21,
        ];
        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_serialize_query_card_operation() {
        let account_id = 123456789u64;
        let card_id = 987654321u64;
        let serialized = serialize_query_card_operation(account_id, card_id);
        let expected: Vec<u8> = vec![
            OP_TYPE_QUERY_CARD,
            0, 0, 0, 0, 7, 91, 205, 21,
            0, 0, 0, 0, 58, 222, 104, 177,
        ];
        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_serialize_bill_operation_with_period() {
        let account_id = 123456789u64;
        let period = Some("2025-10".to_string());
        let serialized = serialize_bill_operation(account_id, period);
        let expected: Vec<u8> = vec![
            OP_TYPE_BILL,
            0, 0, 0, 0, 7, 91, 205, 21,
            7, // length of period string
            50, 48, 50, 53, 45, 49, 48, // "2025-10"
        ];
        assert_eq!(serialized, expected);
    }

    #[test]
    fn test_serialize_bill_operation_without_period() {
        let account_id = 123456789u64;
        let period = None;
        let serialized = serialize_bill_operation(account_id, period);
        let expected: Vec<u8> = vec![
            OP_TYPE_BILL,
            0, 0, 0, 0, 7, 91, 205, 21,
            0, // length 0 indicates no period
        ];
        assert_eq!(serialized, expected);
    }
}

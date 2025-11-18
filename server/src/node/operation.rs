use crate::errors::AppError;

#[derive(Debug, PartialEq, Clone)]
pub struct Operation {
    pub id: u32,
    pub account_id: u64,
    pub card_id: u64,
    pub amount: f32,
}

fn read_bytes<const N: usize>(
    payload: &[u8],
    range: std::ops::Range<usize>,
) -> Result<[u8; N], AppError> {
    payload
        .get(range.clone())
        .ok_or_else(|| AppError::InvalidData {
            details: format!("not enough bytes to deserialize operation: {:?}", range),
        })?
        .try_into()
        .map_err(|e| AppError::InvalidData {
            details: format!("not enough bytes to deserialize operation: {e}"),
        })
}

impl TryFrom<&[u8]> for Operation {
    type Error = AppError;

    fn try_from(payload: &[u8]) -> Result<Self, AppError> {
        Ok(Operation {
            id: u32::from_be_bytes(read_bytes(payload, 0..4)?),
            account_id: u64::from_be_bytes(read_bytes(payload, 4..12)?),
            card_id: u64::from_be_bytes(read_bytes(payload, 12..20)?),
            amount: f32::from_be_bytes(read_bytes(payload, 20..24)?),
        })
    }
}

impl From<Operation> for Vec<u8> {
    fn from(op: Operation) -> Self {
        let mut srl = vec![];
        srl.extend(op.id.to_be_bytes().to_vec());
        srl.extend(op.account_id.to_be_bytes().to_vec());
        srl.extend(op.card_id.to_be_bytes().to_vec());
        srl.extend(op.amount.to_be_bytes().to_vec());
        srl
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_serialize_operation() {
        let op = Operation {
            id: 126,
            account_id: 348,
            card_id: 34821,
            amount: 80500.53,
        };

        let op_srl: Vec<u8> = op.into();
        let expected = [
            0x00, 0x00, 0x00, 0x7E, // 126
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x5C, // 348
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88, 0x05, // 34721
            0x47, 0x9d, 0x3a, 0x44, // 80500.53 IEEE simple precision
        ];
        assert_eq!(op_srl, expected);
    }

    #[test]
    fn deserialize_opeartion() {
        let op_srl = [
            0x00, 0x00, 0x00, 0x7E, // 126
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x5C, // 348
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88, 0x05, // 34721
            0x47, 0x9d, 0x3a, 0x44, // 80500.53 IEEE simple precision
        ];
        let expected = Operation {
            id: 126,
            account_id: 348,
            card_id: 34821,
            amount: 80500.53,
        };
        let op: Operation = op_srl[..].try_into().unwrap();
        assert_eq!(op, expected);
    }
}

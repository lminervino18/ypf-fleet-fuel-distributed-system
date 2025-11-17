use crate::errors::AppError;

#[derive(Debug, PartialEq, Clone)]
pub struct Operation {
    pub id: u32,
}

impl TryFrom<&[u8]> for Operation {
    type Error = AppError;

    fn try_from(payload: &[u8]) -> Result<Self, AppError> {
        Ok(Operation {
            id: u32::from_ne_bytes(payload[0..5].try_into().map_err(|e| {
                AppError::InvalidData {
                    details: format!("not enough bytes to deserialize operation: {e}"),
                }
            })?),
        })
    }
}

impl From<Operation> for Vec<u8> {
    fn from(op: Operation) -> Self {
        op.id.to_be_bytes().to_vec()
    }
}

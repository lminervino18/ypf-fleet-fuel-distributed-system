use crate::serial_error::SerialError;

#[derive(Debug, PartialEq, Clone)]
pub struct Operation {
    pub id: u8,
}

impl TryFrom<&[u8]> for Operation {
    type Error = SerialError;

    fn try_from(payload: &[u8]) -> Result<Self, SerialError> {
        // TODO: check validity of payload
        Ok(Operation { id: payload[0] })
    }
}

impl From<Operation> for Vec<u8> {
    fn from(op: Operation) -> Self {
        vec![op.id]
    }
}

use crate::VerifyError;

#[derive(Debug, Clone, PartialEq)]
pub enum OperationResult {
    Ok,
    Err(VerifyError),
}

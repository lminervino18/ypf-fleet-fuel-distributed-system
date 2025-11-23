use crate::VerifyError;

impl From<VerifyError> for Vec<u8> {
    fn from(error: VerifyError) -> Self {
        todo!();
    }
}

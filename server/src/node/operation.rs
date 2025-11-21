#[derive(Debug, PartialEq, Clone)]
pub struct Operation {
    pub id: u32,
    pub account_id: u64,
    pub card_id: u64,
    pub amount: f32,
}

// msg types
pub const MSG_TYPE_REQUEST: u8 = 0x00;
pub const MSG_TYPE_LOG: u8 = 0x01;
pub const MSG_TYPE_ACK: u8 = 0x02;

// operation types
pub const OP_TYPE_CHARGE: u8 = 0x00;
pub const OP_TYPE_LIMIT_CARD: u8 = 0x01;
pub const OP_TYPE_LIMIT_ACCOUNT: u8 = 0x02;

// attribute variants
pub const NO_LIMIT: f32 = -1.0;
pub const TRUE: u8 = 0x01;
pub const FALSE: u8 = 0x00;

// lenghts of attribute serials
pub const OP_ID_SRL_LEN: usize = size_of::<u32>();
pub const SOCKET_ADDR_LEN: usize = 6; // 4 bytes para la ip + 2 para el puerto

pub const ACC_ID_SRL_LEN: usize = size_of::<u64>();
pub const CARD_ID_SRL_LEN: usize = size_of::<u64>();
pub const AMOUNT_SRL_LEN: usize = size_of::<f32>();
pub const OFFLINE_SRL_LEN: usize = size_of::<u8>();

// lenghts of operation variants
pub const CHARGE_SRL_LEN: usize =
    ACC_ID_SRL_LEN + CARD_ID_SRL_LEN + AMOUNT_SRL_LEN + OFFLINE_SRL_LEN;
pub const LIMIT_ACC_SRL_LEN: usize = ACC_ID_SRL_LEN + AMOUNT_SRL_LEN;
pub const LIMIT_CARD_SRL_LEN: usize = ACC_ID_SRL_LEN + CARD_ID_SRL_LEN + AMOUNT_SRL_LEN;

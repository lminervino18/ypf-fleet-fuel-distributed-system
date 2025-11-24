// heartbeat protocol
pub const HEARTBEAT_REQUEST: u8 = 0xFE;
pub const HEARBEAT_REPLY: u8 = 0xFF;

// msg types
pub const MSG_TYPE_REQUEST: u8 = 0x00;
pub const MSG_TYPE_LOG: u8 = 0x01;
pub const MSG_TYPE_ACK: u8 = 0x02;
pub const MSG_TYPE_JOIN: u8 = 0x03;
pub const MSG_TYPE_CLUSTER_VIEW: u8 = 0x04;
pub const MSG_TYPE_CLUSTER_UPDATE: u8 = 0x05;
pub const MSG_TYPE_RESPONSE: u8 = 0x06;

// debug msgs
pub const MSG_TYPE_ROLE_QUERY: u8 = 0x07;
pub const MSG_TYPE_ROLE_RESPONSE: u8 = 0x08;
pub const NODE_ROLE_LEADER: u8 = 0x00;
pub const NODE_ROLE_REPLICA: u8 = 0x01;
pub const NODE_ROLE_CLIENT: u8 = 0x02;

// operation types
pub const OP_TYPE_CHARGE: u8 = 0x00;
pub const OP_TYPE_LIMIT_CARD: u8 = 0x01;
pub const OP_TYPE_LIMIT_ACCOUNT: u8 = 0x02;
pub const OP_TYPE_QUERY_ACCOUNT: u8 = 0x03;
pub const OP_TYPE_QUERY_CARDS: u8 = 0x04;
pub const OP_TYPE_QUERY_CARD: u8 = 0x05;
pub const OP_TYPE_BILL: u8 = 0x06;

// operation result types
pub const CHARGE_RESULT: u8 = 0x00;
pub const LIMIT_ACCOUNT_RESULT: u8 = 0x02;
pub const LIMIT_CARD_RESULT: u8 = 0x03;
pub const ACC_QUERY_RESULT: u8 = 0x04;

// verify error types
pub const LIMIT_UPDATE_ERR: u8 = 0x00;
pub const CHARGE_LIMIT_ERR: u8 = 0x01;
pub const LIMIT_CHECK_ERR: u8 = 0x01;
pub const BELOW_CURRENT_USAGE: u8 = 0x00;
pub const CARD_LIMIT_EXCEEDED: u8 = 0x01;
pub const ACC_LIMIT_EXCEEDED: u8 = 0x02;

// lengths
pub const LIMIT_CHECK_ERR_LEN: usize = size_of::<u8>();

// attribute variants
pub const NO_LIMIT: f32 = -1.0;
pub const TRUE: u8 = 0x01;
pub const FALSE: u8 = 0x00;
pub const OK: u8 = 0x02;
pub const ERR: u8 = 0x03;

// lenghts of attribute serials
pub const OP_ID_SRL_LEN: usize = size_of::<u32>();
pub const SOCKET_ADDR_LEN: usize = 6; // 4 bytes para la ip + 2 para el puerto

pub const ACC_ID_SRL_LEN: usize = size_of::<u64>();
pub const CARD_ID_SRL_LEN: usize = size_of::<u64>();
pub const AMOUNT_SRL_LEN: usize = size_of::<f32>();
pub const OFFLINE_SRL_LEN: usize = size_of::<u8>();

pub const NODE_ID_SRL_LEN: usize = size_of::<u64>(); // está así en todos lados, pero 64 bytes???
pub const MEMBER_SRL_LEN: usize = NODE_ID_SRL_LEN + SOCKET_ADDR_LEN;
pub const MEMBERS_LEN_SRL_LEN: usize = size_of::<usize>();

pub const PER_CARD_SPENT_LEN: usize = size_of::<usize>();
pub const CARD_SPENT_LEN: usize = CARD_ID_SRL_LEN + AMOUNT_SRL_LEN;

// lenghts of operation variants
pub const CHARGE_SRL_LEN: usize =
    ACC_ID_SRL_LEN + CARD_ID_SRL_LEN + AMOUNT_SRL_LEN + OFFLINE_SRL_LEN;
pub const LIMIT_ACC_SRL_LEN: usize = ACC_ID_SRL_LEN + AMOUNT_SRL_LEN;
pub const LIMIT_CARD_SRL_LEN: usize = ACC_ID_SRL_LEN + CARD_ID_SRL_LEN + AMOUNT_SRL_LEN;

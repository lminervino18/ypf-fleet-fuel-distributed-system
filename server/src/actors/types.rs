use actix::prelude::*;
use std::net::SocketAddr;

/// Globally-addressable actor identity (transport-agnostic).
///
/// - `Account` and `LeaderCard` are globally unique by (account_id) and (card_id).
/// - `Subscriber` can live on many nodes for the same `card_id`. Global identity = (node_id, card_id).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ActorAddr {
    Account    { account_id: u64 },
    LeaderCard { card_id: u64 },
    Subscriber { node_id: u64, card_id: u64 },
}

/// Business-level messages (placeholder; extend with your real domain model).
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum ActorMsg {
    Placeholder(String),
    // e.g. Charge{ tx_id: u64, amount: f64 }, Update{ card_id: u64, delta: f64 }, etc.
}

/// Commands to the ActorRouter (creation, routing, inbound-from-network).
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum RouterCmd {
    /// Fire-and-forget delivery to a logical address (local or remote).
    Send { to: ActorAddr, msg: ActorMsg },

    /// Create local actors on demand (local only).
    CreateAccount    { account_id: u64 },
    CreateLeaderCard { card_id: u64 },
    CreateSubscriber { card_id: u64 }, // local subscriber; node_id = self_node

    /// Raw inbound frame from network; the router will parse & route.
    NetIn { from: SocketAddr, bytes: Vec<u8> },
}

/// Minimal wire-level placeholder.
/// The router should translate ProtocolMessage <-> ActorMsg.
#[derive(Debug, Clone)]
pub enum ProtocolMessage {
    Placeholder(String),
}

/* ====== Queries to ActorRouter (ask pattern) ====== */

/// Returns `Some(Addr<Account>)` if a local Account exists for `account_id`.
#[derive(Debug, Clone, Message)]
#[rtype(result = "Option<Addr<crate::actors::Account>>")]
pub struct GetAccount {
    pub account_id: u64,
}

/// Returns `Some(Addr<LeaderCard>)` if a local Leader exists for `card_id`.
#[derive(Debug, Clone, Message)]
#[rtype(result = "Option<Addr<crate::actors::LeaderCard>>")]
pub struct GetLeader {
    pub card_id: u64,
}

/// Returns `Some(Addr<Subscriber>)` if a local Subscriber exists for `card_id`.
#[derive(Debug, Clone, Message)]
#[rtype(result = "Option<Addr<crate::actors::Subscriber>>")]
pub struct GetLocalSubscriber {
    pub card_id: u64,
}

/// Generic yes/no: do we host this logical actor locally?
#[derive(Debug, Clone, Message)]
#[rtype(result = "bool")]
pub struct HasLocal {
    pub addr: ActorAddr,
}

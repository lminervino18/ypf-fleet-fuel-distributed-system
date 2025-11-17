use actix::prelude::*;
use serde::{Deserialize, Serialize};

// ===========================
//  Actor-level messages
// ===========================

/// High-level messages exchanged between actors.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum ActorMsg {
    /// Free-form debug message.
    Placeholder(String),

    /// Forward a message to a card inside an account.
    CardMessage { card_id: u64, msg: Box<ActorMsg> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    CheckLimit {
        account_id: u64,
        card_id: u64,
        amount: f64,
        request_id: u64,
        station_id: u64,
    },

    Charge {
        account_id: u64,
        card_id: u64,
        amount: f64,
        station_id: u64,
        timestamp: u64,
        op_id: u64,
    },
}

// ===========================
//  Actor → Node events
// ===========================

/// Messages emitted by actors to the Node (over the dedicated channel).
#[derive(Debug, Clone)]
pub enum ActorEvent {
    /// An operation is ready to be processed by the Node.
    /// - `CheckLimit` → processed locally by the Node
    /// - `Charge` → sent through consensus
    OperationReady { operation: Operation },

    /// Optional: local updates caused by actor logic.
    /// The Node may ignore this, unless you want to forward it back to stations.
    CardUpdated { card_id: u64, delta: f64 },

    /// Generic debug/info event.
    Debug(String),
}

// ===========================
//  Commands to the ActorRouter
// ===========================

/// Commands the Node can send to the ActorRouter.
#[derive(Debug, Clone, Message)]
#[rtype(result = "()")]
pub enum RouterCmd {
    /// Send a message to an account (creates it if missing).
    SendToAccount { account_id: u64, msg: ActorMsg },

    /// Send a message to a specific card within an account.
    SendToCard {
        account_id: u64,
        card_id: u64,
        msg: Box<ActorMsg>,
    },

    /// Debug: print the list of active local accounts.
    ListAccounts,
}

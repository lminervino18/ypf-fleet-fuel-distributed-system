//! Actor router module.
//!
//! ActorRouter → Node via channel
//! AccountActor → ActorRouter via Actix messages
//! CardActor → AccountActor → ActorRouter via Actix messages

use actix::prelude::*;
use std::collections::HashMap;

use tokio::sync::mpsc;

use super::account::AccountActor;
use super::types::{ActorEvent, ActorMsg, Operation, RouterCmd};

/// Internal messages coming from AccountActor or CardActor back to ActorRouter.
/// These are NOT seen by the Node directly.
#[derive(Debug, Message)]
#[rtype(result = "()")]
pub enum RouterInternalMsg {
    OperationReady(Operation),
    CardUpdated { card_id: u64, delta: f64 },
    Debug(String),
}

/// Router actor that owns and routes to AccountActor instances.
pub struct ActorRouter {
    /// account_id -> AccountActor address
    pub accounts: HashMap<u64, Addr<AccountActor>>,

    /// Channel ActorRouter → Node
    pub event_tx: mpsc::Sender<ActorEvent>,
}

impl ActorRouter {
    pub fn new(event_tx: mpsc::Sender<ActorEvent>) -> Self {
        Self {
            accounts: HashMap::new(),
            event_tx,
        }
    }

    /// Helper to emit an event to the Node.
    fn emit(&self, ev: ActorEvent) {
        let _ = self.event_tx.try_send(ev);
    }

    /// Return or create an AccountActor.
    fn get_or_create_account(
        &mut self,
        account_id: u64,
        ctx: &mut Context<Self>,
    ) -> Addr<AccountActor> {
        if let Some(addr) = self.accounts.get(&account_id) {
            return addr.clone();
        }

        println!("[Router] Creating local account {}", account_id);

        // AccountActor receives the router address (NOT the channel)
        let addr = AccountActor::new(account_id, ctx.address()).start();

        self.accounts.insert(account_id, addr.clone());
        addr
    }
}

impl Actor for ActorRouter {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        println!("[Router] Started with {} accounts", self.accounts.len());
    }
}

/// Handle commands sent from Node → Router
impl Handler<RouterCmd> for ActorRouter {
    type Result = ();

    fn handle(&mut self, msg: RouterCmd, ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            RouterCmd::SendToAccount { account_id, msg } => {
                let acc = self.get_or_create_account(account_id, ctx);
                acc.do_send(msg);
            }

            RouterCmd::SendToCard {
                account_id,
                card_id,
                msg,
            } => {
                let acc = self.get_or_create_account(account_id, ctx);
                acc.do_send(ActorMsg::CardMessage { card_id, msg });
            }

            RouterCmd::ListAccounts => {
                println!("[Router] Accounts: {:?}", self.accounts.keys());
            }
        }
    }
}

/// Handle messages from AccountActor / CardActor → ActorRouter
impl Handler<RouterInternalMsg> for ActorRouter {
    type Result = ();

    fn handle(&mut self, msg: RouterInternalMsg, _ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            RouterInternalMsg::OperationReady(op) => {
                self.emit(ActorEvent::OperationReady { operation: op });
            }

            RouterInternalMsg::CardUpdated { card_id, delta } => {
                self.emit(ActorEvent::CardUpdated { card_id, delta });
            }

            RouterInternalMsg::Debug(s) => {
                self.emit(ActorEvent::Debug(s));
            }
        }
    }
}

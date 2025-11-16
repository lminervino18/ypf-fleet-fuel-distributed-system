//! Actor router module.
//!
//! The ActorRouter manages local account actors and forwards relevant
//! messages to the node it's running on. Actor hierarchy (conceptually):
//! ```text
//! ActorRouter (node root)
//!  └── AccountActor (one per account)
//!       └── CardActor (one per card)
//! ```

use actix::prelude::*;
use std::collections::HashMap;

use super::account::AccountActor;
use super::types::{ActorMsg, RouterCmd};

/// Router actor that owns and routes to AccountActor instances.
///
/// Responsibilities:
/// - create or lookup local AccountActor instances,
/// - route messages to accounts/cards,
/// - communicate business logic to the node
pub struct ActorRouter {
    /// Map: account_id -> AccountActor address
    pub accounts: HashMap<u64, Addr<AccountActor>>,
}

impl ActorRouter {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
        }
    }

    /// Return an existing AccountActor or create a new one.
    fn get_or_create_account(
        &mut self,
        account_id: u64,
        ctx: &mut Context<Self>,
    ) -> Addr<AccountActor> {
        if let Some(addr) = self.accounts.get(&account_id) {
            return addr.clone();
        }

        println!("[Router] Creating new local account id={}", account_id);
        let addr = AccountActor::new(account_id, ctx.address()).start();
        self.accounts.insert(account_id, addr.clone());
        addr
    }
}

impl Actor for ActorRouter {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        println!(
            "[Router] ActorRouter started with {} accounts",
            self.accounts.len(),
        );
    }
}

impl Handler<RouterCmd> for ActorRouter {
    type Result = ();

    fn handle(&mut self, msg: RouterCmd, ctx: &mut Context<Self>) -> Self::Result {
        match msg {
            // Create or forward a message to an account
            RouterCmd::SendToAccount { account_id, msg } => {
                let acc = self.get_or_create_account(account_id, ctx);
                acc.do_send(msg);
            }

            // Send a message to a specific card within an account
            RouterCmd::SendToCard {
                account_id,
                card_id,
                msg,
            } => {
                let acc = self.get_or_create_account(account_id, ctx);
                acc.do_send(ActorMsg::CardMessage { card_id, msg });
            }

            // List active local accounts
            RouterCmd::ListAccounts => {
                println!("[Router] Local accounts: {:?}", self.accounts.keys());
            }
        }
    }
}

//! Actor router module.
//!
//! The ActorRouter manages local account actors and forwards or replicates
//! messages to known replicas. Actor hierarchy (conceptually):
//! ```text
//! ActorRouter (node root)
//!  └── AccountActor (one per account)
//!       └── CardActor (one per card)
//! ```
//!
//! The router bridges network events (via ConnectionManager) and local actors.

use actix::prelude::*;
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::account::AccountActor;
use super::types::{ActorMsg, RouterCmd};
use crate::connection_manager::ManagerCmd;

/// Router actor that owns and routes to AccountActor instances.
///
/// Responsibilities:
/// - create or lookup local AccountActor instances,
/// - route messages to accounts/cards,
/// - replicate payloads to configured replica addresses via the ConnectionManager.
pub struct ActorRouter {
    /// Map: account_id -> AccountActor address
    pub accounts: HashMap<u64, Addr<AccountActor>>,

    /// Channel to the ConnectionManager to send network messages (to replicas).
    pub manager_cmd: mpsc::Sender<ManagerCmd>,

    /// Replica addresses as "IP:PORT" strings.
    pub replicas: Vec<String>,
}

impl ActorRouter {
    pub fn new(manager_cmd: mpsc::Sender<ManagerCmd>, replicas: Vec<String>) -> Self {
        Self {
            accounts: HashMap::new(),
            manager_cmd,
            replicas,
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
            "[Router] ActorRouter started with {} accounts and {} replicas",
            self.accounts.len(),
            self.replicas.len()
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

            // Handle incoming network bytes forwarded from ConnectionManager
            RouterCmd::NetIn { from, bytes } => {
                // Attempt to decode payload as UTF-8
                let payload = String::from_utf8_lossy(&bytes);
                println!(
                    "[Router][NetIn] Message received from {}: {}",
                    from, payload
                );

                // Simple example processing: classify replication or generic messages.
                if payload.starts_with("REPL:") {
                    println!("[Router] Replication message from {}", from);
                    // TODO: parse and apply replication payload to local state.
                } else if payload.starts_with("MSG") {
                    println!("[Router] Generic MSG payload: {}", payload);
                } else {
                    println!("[Router] Unknown payload: {}", payload);
                }
            }

            // List active local accounts
            RouterCmd::ListAccounts => {
                println!("[Router] Local accounts: {:?}", self.accounts.keys());
            }
        }
    }
}

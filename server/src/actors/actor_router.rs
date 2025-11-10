use actix::prelude::*;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;

use crate::connection::ManagerCmd;
use crate::errors::{AppError, AppResult};
use tokio::sync::mpsc;

use super::types::*;
use super::{Account, LeaderCard, Subscriber};

/// ActorRouter:
/// - Keeps local registries (actors hosted by this node)
/// - Routes messages locally or to remote nodes via TCP (ManagerCmd)
/// - Maintains routing table: node_id -> SocketAddr
pub struct ActorRouter {
    pub accounts: HashMap<u64, Addr<Account>>,
    pub leaders: HashMap<u64, Addr<LeaderCard>>,
    pub subs: HashMap<u64, Addr<Subscriber>>,
    pub remote_subs: HashMap<u64, HashSet<u64>>,
    pub manager_cmd_tx: mpsc::Sender<ManagerCmd>,
    pub self_node: u64,
    pub routing: HashMap<u64, SocketAddr>,
}

impl ActorRouter {
    pub fn new(self_node: u64, manager_cmd_tx: mpsc::Sender<ManagerCmd>) -> Self {
        Self {
            accounts: HashMap::new(),
            leaders: HashMap::new(),
            subs: HashMap::new(),
            remote_subs: HashMap::new(),
            manager_cmd_tx,
            self_node,
            routing: HashMap::new(),
        }
    }

    /// Helper: asynchronously send a string message to a remote node via TCP.
    fn send_tcp(&self, addr: SocketAddr, msg: &str) -> AppResult<()> {
        let tx = self.manager_cmd_tx.clone();
        let msg_owned = msg.to_string();
        actix::spawn(async move {
            if let Err(e) = tx.send(ManagerCmd::SendTo(addr, msg_owned)).await {
                eprintln!(
                    "[Router][WARN] Failed to enqueue TCP send to {}: {}",
                    addr, e
                );
            }
        });
        Ok(())
    }

    /// Parses a simple wire message like: `MSG <from_id> PING` or `MSG <from_id> PONG`
    fn parse_wire_message(msg: &str) -> AppResult<(u64, String)> {
        let parts: Vec<&str> = msg.split_whitespace().collect();
        if parts.len() < 3 || parts[0] != "MSG" {
            return Err(AppError::ProtocolParse {
                details: format!("Invalid wire message: '{}'", msg),
            });
        }

        let from_id = parts[1].parse::<u64>().map_err(|_| AppError::ProtocolParse {
            details: format!("Invalid sender ID in message: '{}'", msg),
        })?;

        let payload = parts[2].to_string();
        Ok((from_id, payload))
    }

    /// Handles a raw inbound frame coming from the network.
    fn handle_net_in(&self, from: SocketAddr, bytes: Vec<u8>) -> AppResult<()> {
        let msg = String::from_utf8(bytes.clone()).map_err(|_| AppError::ProtocolParse {
            details: "Invalid UTF-8 in inbound message".to_string(),
        })?;

        println!("[Router] NetIn from {}: '{}'", from, msg.trim());

        // Parse and react to protocol messages
        if msg.starts_with("MSG") {
            let (from_id, payload) = Self::parse_wire_message(&msg)?;

            println!("[Router] Got '{}' from {}", payload, from_id);

            if payload == "PING" {
                // Reply with a PONG
                let reply = format!("MSG {} PONG\n", self.self_node);
                self.send_tcp(from, &reply)?;
            } else if payload == "PONG" {
                println!("[Router] Received PONG from {}", from_id);
            } else {
                println!("[Router] Unknown payload '{}' from {}", payload, from_id);
            }
        } else {
            return Err(AppError::ProtocolParse {
                details: format!("Unsupported message format: '{}'", msg.trim()),
            });
        }

        Ok(())
    }
}

impl Actor for ActorRouter {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        println!(
            "[Router] Started (node_id={}, local: acc={}, leaders={}, subs={})",
            self.self_node,
            self.accounts.len(),
            self.leaders.len(),
            self.subs.len()
        );
    }
}

impl Handler<RouterCmd> for ActorRouter {
    type Result = ();

    fn handle(&mut self, msg: RouterCmd, ctx: &mut Self::Context) -> Self::Result {
        if let Err(e) = self.try_handle(msg, ctx) {
            eprintln!("[Router][ERROR] {}", e);
        }
    }
}

impl ActorRouter {
    /// Inner version of handler that returns structured AppError instead of swallowing them.
    fn try_handle(&mut self, msg: RouterCmd, ctx: &mut Context<Self>) -> AppResult<()> {
        match msg {
            // --- Create local subscriber ---
            RouterCmd::CreateSubscriber { card_id } => {
                if self.subs.contains_key(&card_id) {
                    println!("[Router] Subscriber({}) already exists", card_id);
                    return Ok(());
                }
                let addr = Subscriber::new(card_id, ctx.address()).start();
                self.subs.insert(card_id, addr);
                println!("[Router] Created local Subscriber({})", card_id);
            }

            // --- Handle inbound TCP frame ---
            RouterCmd::NetIn { from, bytes } => {
                self.handle_net_in(from, bytes)?;
            }

            // --- Other commands (placeholders) ---
            RouterCmd::CreateAccount { account_id } => {
                let addr = Account::new(account_id, ctx.address()).start();
                self.accounts.insert(account_id, addr);
                println!("[Router] Created local Account({})", account_id);
            }

            RouterCmd::CreateLeaderCard { card_id } => {
                let addr = LeaderCard::new(card_id, ctx.address()).start();
                self.leaders.insert(card_id, addr);
                println!("[Router] Created local LeaderCard({})", card_id);
            }

            RouterCmd::Send { to, msg } => {
                println!("[Router] Route send → {:?} with {:?}", to, msg);
                // Future: route between nodes
            }
        }
        Ok(())
    }
}

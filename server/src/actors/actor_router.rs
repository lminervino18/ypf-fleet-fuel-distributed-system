use actix::prelude::*;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use crate::connection::ManagerCmd;
use tokio::sync::mpsc;

use super::types::*;
use super::{Account, LeaderCard, Subscriber};

pub struct ActorRouter {
    pub accounts: HashMap<u64, Addr<Account>>,
    pub leaders:  HashMap<u64, Addr<LeaderCard>>,
    pub subs:     HashMap<u64, Addr<Subscriber>>,
    pub remote_subs: HashMap<u64, HashSet<u64>>,
    pub manager_cmd_tx: mpsc::Sender<ManagerCmd>,
    pub self_node: u64,
    pub routing: HashMap<u64, SocketAddr>,
}

impl ActorRouter {
    pub fn new(self_node: u64, manager_cmd_tx: mpsc::Sender<ManagerCmd>) -> Self {
        Self {
            accounts: HashMap::new(),
            leaders:  HashMap::new(),
            subs:     HashMap::new(),
            remote_subs: HashMap::new(),
            manager_cmd_tx,
            self_node,
            routing: HashMap::new(),
        }
    }

    fn send_tcp(&self, addr: SocketAddr, msg: &str) {
        let tx = self.manager_cmd_tx.clone();
        let msg_owned = msg.to_string();
        actix::spawn(async move {
            let _ = tx.send(ManagerCmd::SendTo(addr, msg_owned)).await;
        });
    }
}

impl Actor for ActorRouter {
    type Context = Context<Self>;
}

impl Handler<RouterCmd> for ActorRouter {
    type Result = ();

    fn handle(&mut self, msg: RouterCmd, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            RouterCmd::CreateSubscriber { card_id } => {
                let addr = Subscriber::new(card_id, ctx.address()).start();
                self.subs.insert(card_id, addr);
                println!("[Router] Created local Subscriber({})", card_id);
            }

            RouterCmd::NetIn { from, bytes } => {
                let msg = String::from_utf8_lossy(&bytes);
                println!("[Router] NetIn from {}: '{}'", from, msg);

                if msg.starts_with("MSG") {
                    // Parse simple message: "MSG <from_id> PING"
                    let parts: Vec<_> = msg.split_whitespace().collect();
                    if parts.len() >= 3 {
                        let from_id: u64 = parts[1].parse().unwrap_or(0);
                        let payload = parts[2];
                        println!("[Router] Got '{}' from {}", payload, from_id);

                        if payload == "PING" {
                            // respond
                            let reply = format!("MSG {} PONG\n", self.self_node);
                            self.send_tcp(from, &reply);
                        }
                    }
                } else if msg.starts_with("MSG") && msg.contains("PONG") {
                    println!("[Router] Received PONG!");
                }
            }

            _ => {}
        }
    }
}

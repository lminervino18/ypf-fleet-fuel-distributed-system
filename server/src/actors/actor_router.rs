use actix::prelude::*;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;

use super::types::{
    ActorAddr, ActorMsg, RouterCmd, ProtocolMessage,
    GetAccount, GetLeader, GetLocalSubscriber, HasLocal,
};
use crate::connection::ManagerCmd; // bridge to your TCP connection manager

use tokio::sync::mpsc;

use super::{Account, LeaderCard, Subscriber};

/// ActorRouter:
/// - Keeps local registries (who lives in this process)
/// - Decides local vs remote delivery for ActorAddr
/// - Talks to ConnectionManager via `manager_cmd_tx` for remote sends
/// - Maintains a routing table node_id -> SocketAddr
pub struct ActorRouter {
    /// Local actors (only those hosted by this node)
    pub accounts: HashMap<u64, Addr<Account>>,     // account_id -> Addr
    pub leaders:  HashMap<u64, Addr<LeaderCard>>,  // card_id    -> Addr
    pub subs:     HashMap<u64, Addr<Subscriber>>,  // card_id    -> Addr (local subscriber)

    /// Optional: for each card, which remote nodes also host a subscriber
    pub remote_subs: HashMap<u64, HashSet<u64>>,   // card_id -> {node_id,...}

    /// Bridge to TCP communicator
    pub manager_cmd_tx: mpsc::Sender<ManagerCmd>,

    /// This node id (used to decide if ActorAddr::Subscriber is local or remote)
    pub self_node: u64,

    /// Node routing: node_id -> socket address
    pub routing: HashMap<u64, SocketAddr>,
}

impl ActorRouter {
    pub fn new(
        self_node: u64,
        manager_cmd_tx: mpsc::Sender<ManagerCmd>,
    ) -> Self {
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

    /// Helper: is a Subscriber address local to this node?
    pub fn is_local_subscriber(&self, node_id: u64) -> bool {
        node_id == self.self_node
    }

    /// Helper: (placeholder) map ActorMsg -> ProtocolMessage for remote sends.
    fn to_protocol(msg: ActorMsg) -> ProtocolMessage {
        // TODO: build a real mapping from domain ActorMsg to wire-level ProtocolMessage
        match msg {
            ActorMsg::Placeholder(s) => ProtocolMessage::Placeholder(s),
        }
    }

    /// Helper: (placeholder) encode a `ProtocolMessage` to a **String** for the wire,
    /// because ManagerCmd::SendTo currently expects `String`.
    fn encode_wire(msg: ProtocolMessage) -> String {
        // TODO: replace with real serialization (JSON, bincode, prost...)
        match msg {
            ProtocolMessage::Placeholder(s) => format!("[wire-placeholder] {}\n", s),
        }
    }
}

impl Actor for ActorRouter {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        log::info!(
            "ActorRouter started (self_node={}, local: acc={}, leaders={}, subs={})",
            self.self_node,
            self.accounts.len(),
            self.leaders.len(),
            self.subs.len()
        );
    }
}

/* =======================
   Handlers: Create / Send / NetIn
   ======================= */

impl Handler<RouterCmd> for ActorRouter {
    type Result = ();

    fn handle(&mut self, msg: RouterCmd, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            /* ---- Create local actors ---- */
            RouterCmd::CreateAccount { account_id } => {
                if self.accounts.contains_key(&account_id) {
                    log::debug!("Account {} already exists locally", account_id);
                    return;
                }
                let addr = Account::new(account_id, ctx.address()).start();
                self.accounts.insert(account_id, addr);
                log::info!("Created local Account({})", account_id);
            }

            RouterCmd::CreateLeaderCard { card_id } => {
                if self.leaders.contains_key(&card_id) {
                    log::debug!("LeaderCard {} already exists locally", card_id);
                    return;
                }
                let addr = LeaderCard::new(card_id, ctx.address()).start();
                self.leaders.insert(card_id, addr);
                log::info!("Created local LeaderCard({})", card_id);
            }

            RouterCmd::CreateSubscriber { card_id } => {
                if self.subs.contains_key(&card_id) {
                    log::debug!("Subscriber for card {} already exists locally", card_id);
                    return;
                }
                let addr = Subscriber::new(card_id, ctx.address()).start();
                self.subs.insert(card_id, addr);
                log::info!("Created local Subscriber(card_id={})", card_id);
            }

            /* ---- Route a domain message ---- */
            RouterCmd::Send { to, msg } => {
                match to {
                    ActorAddr::Account { account_id } => {
                        if let Some(a) = self.accounts.get(&account_id) {
                            a.do_send(msg);
                        } else {
                            // Remote path (not implemented yet): find where account lives, serialize + send via TCP.
                            log::warn!("Account({}) not local; remote send not implemented yet", account_id);
                        }
                    }
                    ActorAddr::LeaderCard { card_id } => {
                        if let Some(l) = self.leaders.get(&card_id) {
                            l.do_send(msg);
                        } else {
                            log::warn!("LeaderCard({}) not local; remote send not implemented yet", card_id);
                        }
                    }
                    ActorAddr::Subscriber { node_id, card_id } => {
                        if self.is_local_subscriber(node_id) {
                            if let Some(s) = self.subs.get(&card_id) {
                                s.do_send(msg);
                            } else {
                                log::warn!("Local Subscriber(card_id={}) not found", card_id);
                            }
                        } else {
                            // Remote: resolve node address & hand over to ConnectionManager
                            if let Some(sock) = self.routing.get(&node_id).copied() {
                                let proto = Self::to_protocol(msg);
                                let wire = Self::encode_wire(proto); // <- String now
                                let tx = self.manager_cmd_tx.clone();
                                // Fire-and-forget: schedule a TCP send
                                actix::spawn(async move {
                                    let _ = tx.send(ManagerCmd::SendTo(sock, wire)).await;
                                });
                            } else {
                                log::warn!(
                                    "No routing entry for node_id={} to reach Subscriber(card_id={})",
                                    node_id, card_id
                                );
                            }
                        }
                    }
                }
            }

            /* ---- Raw inbound frames from TCP ---- */
            RouterCmd::NetIn { from, bytes } => {
                // TODO: parse `bytes` -> ProtocolMessage -> ActorMsg + resolve ActorAddr destination.
                // For now, just log that we received a frame.
                log::debug!("ActorRouter NetIn from {} ({} bytes): {:?}", from, bytes.len(), &bytes);
            }
        }
    }
}

/* =======================
   Handlers: Queries (ask)
   ======================= */

impl Handler<GetAccount> for ActorRouter {
    type Result = Option<Addr<Account>>;

    fn handle(&mut self, msg: GetAccount, _ctx: &mut Self::Context) -> Self::Result {
        self.accounts.get(&msg.account_id).cloned()
    }
}

impl Handler<GetLeader> for ActorRouter {
    type Result = Option<Addr<LeaderCard>>;

    fn handle(&mut self, msg: GetLeader, _ctx: &mut Self::Context) -> Self::Result {
        self.leaders.get(&msg.card_id).cloned()
    }
}

impl Handler<GetLocalSubscriber> for ActorRouter {
    type Result = Option<Addr<Subscriber>>;

    fn handle(&mut self, msg: GetLocalSubscriber, _ctx: &mut Self::Context) -> Self::Result {
        self.subs.get(&msg.card_id).cloned()
    }
}

impl Handler<HasLocal> for ActorRouter {
    type Result = bool;

    fn handle(&mut self, msg: HasLocal, _ctx: &mut Self::Context) -> Self::Result {
        match msg.addr {
            ActorAddr::Account { account_id } => self.accounts.contains_key(&account_id),
            ActorAddr::LeaderCard { card_id } => self.leaders.contains_key(&card_id),
            ActorAddr::Subscriber { node_id, card_id } => {
                self.is_local_subscriber(node_id) && self.subs.contains_key(&card_id)
            }
        }
    }
}

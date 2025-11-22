use crate::node::message::Message;
use std::net::SocketAddr;

/// Simple, modular Bully algorithm helper.
///
/// This struct contains minimal state and helpers to send/receive
/// bully messages. It is intentionally lightweight; the surrounding
/// node (Leader/Replica) drives when to call these methods and provides
/// the peer list and connection instance.
pub struct Bully {
    pub id: u64,
    pub address: SocketAddr,
    pub leader_id: Option<u64>,
    pub leader_addr: Option<SocketAddr>,
    pub election_in_progress: bool,
    pub received_ok: bool,
}

use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;

/// Conduct a Bully election: owns the coordination flow
/// (send Election, wait for replies, promote to coordinator). It receives an
/// `Arc<Mutex<Bully>>` so it can be spawned as a background task without the
/// caller holding the lock across await points.
pub async fn conduct_election(
    bully: Arc<Mutex<Bully>>,
    outgoing: Sender<(Message, SocketAddr)>,
    peers: Vec<SocketAddr>,
    id: u64,
    address: SocketAddr,
) {
    // attempt to start election; return early if another election is active
    if !bully.lock().await.mark_start_election() {
        return;
    }

    let msg = Message::Election {
        candidate_id: id,
        candidate_addr: address,
    };

    // broadcast Election
    for p in &peers {
        let _ = outgoing.send((msg.clone(), *p)).await;
    }

    // wait for responses window
    let window_ms: u64 = 400;
    tokio::time::sleep(std::time::Duration::from_millis(window_ms)).await;

    let got_ok = { let b = bully.lock().await; b.received_ok };
    if !got_ok {
        // become coordinator/leader
        let mut b = bully.lock().await;
        b.mark_coordinator();

        let coordinator_msg = Message::Coordinator { leader_id: id, leader_addr: address };
        for p in &peers {
            let _ = outgoing.send((coordinator_msg.clone(), *p)).await;
        }
    }
}

impl Bully {
    pub fn new(id: u64, address: SocketAddr) -> Self {
        Self {
            id,
            address,
            leader_id: None,
            leader_addr: None,
            election_in_progress: false,
            received_ok: false,
        }
    }

    /// Mark that an election has started (state only).
    pub fn mark_start_election(&mut self) -> bool {
        if self.election_in_progress {
            return false;
        }
        self.election_in_progress = true;
        self.received_ok = false;
        true
    }

    /// Handle an incoming Election message from a candidate.
    /// If this node has higher id, reply `ElectionOk` to candidate.
    /// Decide whether we should reply OK to a candidate (higher id wins).
    pub fn should_reply_ok(&self, candidate_id: u64) -> bool {
        self.id > candidate_id
    }

    /// Handle an OK reply to our election request.
    pub fn on_election_ok(&mut self) {
        // Record that at least one higher process is alive.
        self.received_ok = true;
    }

    /// Broadcast that this node is the coordinator/leader.
    /// Mark coordinator in state
    pub fn mark_coordinator(&mut self) {
        self.leader_id = Some(self.id);
        self.leader_addr = Some(self.address);
        self.election_in_progress = false;
        self.received_ok = false;
    }

    /// Handle an incoming Coordinator announcement.
    pub fn on_coordinator(&mut self, leader_id: u64, leader_addr: SocketAddr) {
        self.leader_id = Some(leader_id);
        self.leader_addr = Some(leader_addr);
        self.election_in_progress = false;
        self.received_ok = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::network::Connection;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn bully_new_sets_fields() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40000);
        let b = Bully::new(42, addr);
        assert_eq!(b.id, 42);
        assert_eq!(b.address, addr);
        assert!(!b.election_in_progress);
        assert!(!b.received_ok);
        assert!(b.leader_id.is_none());
    }

    #[test]
    fn on_coordinator_updates_state() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40001);
        let mut b = Bully::new(7, addr);
        let leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 50000);
        b.on_coordinator(99, leader_addr);
        assert_eq!(b.leader_id, Some(99));
        assert_eq!(b.leader_addr, Some(leader_addr));
        assert!(!b.election_in_progress);
    }

    // This test only checks that start_election flips internal flags when called
    // with an empty peers list (so no actual network sends are attempted).
    #[tokio::test]
    async fn start_election_sets_state() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        // start a Connection bound to an ephemeral port; we won't send to peers
        let _conn = Connection::start(addr, 1).await.expect("start connection");
        let mut b = Bully::new(3, addr);

        // Call mark_start_election with empty peer list: should set election_in_progress
        b.mark_start_election();
        assert!(b.election_in_progress);
        assert!(!b.received_ok);
    }
}

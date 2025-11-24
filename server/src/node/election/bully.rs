use common::Message;
use std::net::SocketAddr;

/// Bully algorithm helper
pub struct Bully {
    pub id: u64,
    pub address: SocketAddr,
    pub leader_id: Option<u64>,
    pub leader_addr: Option<SocketAddr>,
    pub election_in_progress: bool,
    pub received_ok: bool,
}

use common::Connection;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Conduct a Bully election: sends Election to higher-ID peers,
/// waits for replies, and promotes to coordinator if no ElectionOk received.
/// peer_ids: map of peer_id -> SocketAddr for all other nodes
pub async fn conduct_election(
    bully: &Arc<Mutex<Bully>>,
    connection: &mut Connection,
    peer_ids: HashMap<u64, SocketAddr>,
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

    // Send Election ONLY to nodes with higher ID (Bully algorithm rule)
    let higher_peers: Vec<SocketAddr> = peer_ids
        .iter()
        .filter(|(peer_id, _)| **peer_id > id)
        .map(|(_, addr)| *addr)
        .collect();

    // println!("[Bully ID={}] Starting election, sending to {} higher peers", id, higher_peers.len());
    // println!("Higher peers: {:?}", higher_peers);
    for p in &higher_peers {
        let _ = connection.send(msg.clone(), p).await;
    }

    // wait for responses
    const WINDOW_MS: u64 = 200;
    tokio::time::sleep(std::time::Duration::from_millis(WINDOW_MS)).await;

    let got_ok = {
        let b = bully.lock().await;
        b.received_ok
    };
    if !got_ok {
        // become coordinator/leader (no higher process responded)
        // println!("[Bully ID={}] No ElectionOk received, becoming coordinator", id);
        let mut b = bully.lock().await;
        b.mark_coordinator();

        // Announce to ALL peers
        let all_peers: Vec<SocketAddr> = peer_ids.values().copied().collect();
        let coordinator_msg = Message::Coordinator {
            leader_id: id,
            leader_addr: address,
        };
        for p in &all_peers {
            
            let _ = connection.send(coordinator_msg.clone(), p).await;
        }
    } else {
        // println!("[Bully ID={}] Received ElectionOk, stepping down", id);
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
    
    pub fn is_election_in_progress(&self) -> bool {
        // NOTE: Ahora mismo si ya hay una elecciòn en proceso y se llamò a Election se ignora
        self.election_in_progress
    }

    /// Handle an incoming Election message from a candidate.
    /// If this node has higher id, reply `ElectionOk` to candidate.
    /// Decide whether we should reply OK to a candidate (higher id wins).
    pub fn should_reply_ok(&self, candidate_id: u64) -> bool {
        self.id > candidate_id
    }

    /// Handle an OK reply to our election request.
    pub fn on_election_ok(&mut self, _responder_id: u64) {
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
    use common::Connection;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn bully_new_sets_fields() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40000);
        let b = Bully::new(42, addr);
        assert_eq!(b.id, 42);
        assert_eq!(b.address, addr);
        assert!(!b.is_election_in_progress());
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
        assert!(!b.is_election_in_progress());
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
        assert!(b.is_election_in_progress());
        assert!(!b.received_ok);
    }
}

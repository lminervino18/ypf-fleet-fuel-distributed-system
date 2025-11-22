use crate::node::message::Message;
use crate::node::network::Connection;
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

    /// Start an election: broadcast `Election` to all peers.
    pub fn start_election(&mut self, conn: &mut Connection, peers: &[SocketAddr]) {
        if self.election_in_progress {
            return;
        }
        self.election_in_progress = true;
        self.received_ok = false;

        let msg = Message::Election {
            candidate_id: self.id,
            candidate_addr: self.address,
        };

        // send to all peers (they will reply OK if they have higher id)
        for peer in peers {
            let _ = conn.send(msg.clone(), peer);
        }
    }

    /// Handle an incoming Election message from a candidate.
    /// If this node has higher id, reply `ElectionOk` to candidate.
    pub fn on_election_received(&mut self, conn: &mut Connection, candidate_id: u64, candidate_addr: SocketAddr) {
        if self.id > candidate_id {
            let reply = Message::ElectionOk {
                responder_id: self.id,
                responder_addr: self.address,
            };

            let _ = conn.send(reply, &candidate_addr);
            // Optionally start our own election to take over
            // (the caller can decide to trigger start_election as well)
        }
    }

    /// Handle an OK reply to our election request.
    pub fn on_election_ok(&mut self, _responder_id: u64, _responder_addr: SocketAddr) {
        // Record that at least one higher process is alive.
        self.received_ok = true;
    }

    /// Broadcast that this node is the coordinator/leader.
    pub fn announce_coordinator(&mut self, conn: &mut Connection, peers: &[SocketAddr]) {
        self.leader_id = Some(self.id);
        self.leader_addr = Some(self.address);
        self.election_in_progress = false;

        let msg = Message::Coordinator {
            leader_id: self.id,
            leader_addr: self.address,
        };

        for peer in peers {
            let _ = conn.send(msg.clone(), peer);
        }
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
        let mut conn = Connection::start(addr, 1).await.expect("start connection");
        let mut b = Bully::new(3, addr);

        // Call start_election with empty peer list: should set election_in_progress
        b.start_election(&mut conn, &[]);
        assert!(b.election_in_progress);
        assert!(!b.received_ok);
    }
}

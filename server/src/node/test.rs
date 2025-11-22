#[cfg(test)]
mod node_test {
    use crate::node::{message::Message, operation::Operation, Leader, Replica};
    use std::net::{IpAddr, SocketAddr};
    use std::thread;
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    use tokio::task;
    use tokio::{io::AsyncWriteExt, net::TcpStream};

    /* #[test]
    fn test_syncronization_between_one_leader_and_one_replica() {
        let leader_addr = "127.0.0.1:12345";
        let replica_addr = "127.0.0.1:12346";
        let mut leader = std::process::Command::cargo_bin("server").unwrap()

        let mut replica = std::process::Command::new(format!(
            "cargo run --bin server -- replica --leader-addr=\"{leader_addr}\""
        ))
        .spawn()
        .unwrap();

        leader.wait().unwrap();
        replica.wait().unwrap();
    } */

    /* #[tokio::test]
    async fn test_syncronization_between_one_leader_and_one_replica() {
        let dummy_coords = (0.0, 0.0);
        let leader_addr = SocketAddr::new(IpAddr::V4([127, 0, 0, 1].into()), 12346);
        let replica_addr = SocketAddr::new(IpAddr::V4([127, 0, 0, 1].into()), 12345);
        let leader = tokio::spawn(async move {
            Leader::start(leader_addr, dummy_coords, vec![replica_addr], 10)
                .await
                .unwrap();
        });

        let replica = tokio::spawn(async move {
            Replica::start(replica_addr, dummy_coords, leader_addr, vec![], 10)
                .await
                .unwrap();
        });

        tokio::time::sleep(Duration::from_secs(1)).await; // wait for listener
        let mut skt = TcpStream::connect(leader_addr).await.unwrap();
        let op = Operation {
            id: 1,
            account_id: 1,
            card_id: 1,
            amount: 1.0,
        };
        let request: Vec<u8> = Message::Request {
            op,
            addr: skt.local_addr().unwrap(),
        }
        .into();
        skt.write_all(&request).await.unwrap();
        let mut buf = [0; 64];
        let _ = skt.read(&mut buf).await.unwrap();
        println!("buf read: {:?}", buf);
        leader.abort();
        replica.abort();
    } */
}

#[cfg(test)]
mod bully_election_test {
    use crate::node::election::bully::Bully;
    use crate::node::network::Connection;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio::time::{sleep, Duration};

    /// Test Bully election with 1 leader (low ID) and 3 replicas.
    /// Simulates leader failure and verifies the replica with highest ID becomes coordinator.
    #[tokio::test]
    async fn test_bully_election_with_leader_and_three_replicas() {
        // Setup: 1 leader + 3 replicas with distinct IDs
        // Leader has lowest ID (will lose election), replicas have higher IDs
        let leader_id = 1u64;
        let leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 13000);
        
        let replica1_id = 10u64;
        let replica1_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 13001);
        
        let replica2_id = 20u64;
        let replica2_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 13002);
        
        let replica3_id = 30u64; // highest ID, should become coordinator
        let replica3_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 13003);

        // Create connections for each node
        let _leader_conn = Arc::new(Mutex::new(
            Connection::start(leader_addr, 10).await.expect("leader connection")
        ));
        let replica1_conn = Arc::new(Mutex::new(
            Connection::start(replica1_addr, 10).await.expect("replica1 connection")
        ));
        let replica2_conn = Arc::new(Mutex::new(
            Connection::start(replica2_addr, 10).await.expect("replica2 connection")
        ));
        let replica3_conn = Arc::new(Mutex::new(
            Connection::start(replica3_addr, 10).await.expect("replica3 connection")
        ));

        // Create Bully instances
        let _leader_bully = Arc::new(Mutex::new(Bully::new(leader_id, leader_addr)));
        let replica1_bully = Arc::new(Mutex::new(Bully::new(replica1_id, replica1_addr)));
        let replica2_bully = Arc::new(Mutex::new(Bully::new(replica2_id, replica2_addr)));
        let replica3_bully = Arc::new(Mutex::new(Bully::new(replica3_id, replica3_addr)));

        // Wait for connections to be ready
        sleep(Duration::from_millis(100)).await;

        // Simulate leader failure by starting election from replica1
        // replica1 knows about all other nodes
        let replica1_peers = vec![leader_addr, replica2_addr, replica3_addr];
        
        // Spawn election from replica1
        let r1_bully = replica1_bully.clone();
        let r1_conn = replica1_conn.clone();
        let r1_peers = replica1_peers.clone();
        tokio::spawn(async move {
            crate::node::election::bully::conduct_election(
                r1_bully,
                r1_conn,
                r1_peers,
                replica1_id,
                replica1_addr,
            ).await;
        });

        // Simulate replicas 2 and 3 receiving Election messages and responding
        sleep(Duration::from_millis(50)).await;

        // Replica2 and Replica3 should receive Election from Replica1
        // Since they have higher IDs, they should reply with ElectionOk and start their own elections
        
        // Replica2 receives Election from Replica1, sends OK and starts election
        // TODO: Cambiar esto por el envio de mensajes reales
        {
            let mut r1_b = replica1_bully.lock().await;
            let r2_b = replica2_bully.lock().await;
            let r3_b = replica3_bully.lock().await;
            assert_eq!(r2_b.should_reply_ok(replica1_id), true,
                "Replica2 should reply OK to Replica1's election");
            assert_eq!(r3_b.should_reply_ok(replica1_id), true,
                "Replica3 should reply OK to Replica1's election");
            r1_b.on_election_ok(replica2_id);
        }

        println!("== Replica2 starts election ==");
        let r2_bully = replica2_bully.clone();
        let r2_conn = replica2_conn.clone();
        let replica2_peers = vec![leader_addr, replica1_addr, replica3_addr];
        tokio::spawn(async move {
            crate::node::election::bully::conduct_election(
                r2_bully,
                r2_conn,
                replica2_peers,
                replica2_id,
                replica2_addr,
            ).await;
        });

        // replica2 sent to replica1 and replica3, which should reply OK to replica3
        {
            let mut r2_b = replica2_bully.lock().await;
            let r3_b = replica3_bully.lock().await;
            assert_eq!(r3_b.should_reply_ok(replica1_id), true,
                "Replica3 should reply OK to Replica1's election");
            r2_b.on_election_ok(replica3_id);
        }
        
        sleep(Duration::from_millis(50)).await;

        println!("== Replica3 starts election ==");
        let r3_bully = replica3_bully.clone();
        let r3_conn = replica3_conn.clone();
        let replica3_peers = vec![leader_addr, replica1_addr, replica2_addr];
        tokio::spawn(async move {
            crate::node::election::bully::conduct_election(
                r3_bully,
                r3_conn,
                replica3_peers,
                replica3_id,
                replica3_addr,
            ).await;
        });

        // Wait for election window + coordinator announcement
        sleep(Duration::from_millis(1000)).await;

        // Verify: Replica3 (highest ID) should be the new coordinator
        {
            let r3_b = replica3_bully.lock().await;
            assert_eq!(r3_b.leader_id, Some(replica3_id), 
                "Replica3 should recognize itself as leader");
            assert_eq!(r3_b.leader_addr, Some(replica3_addr),
                "Replica3 should have its own address as leader address");
            assert!(!r3_b.election_in_progress,
                "Election should be finished");
        }

        // Verify: Replica1 should recognize Replica3 as leader (after receiving Coordinator msg)
        {
            let mut r1_b = replica1_bully.lock().await;
            r1_b.on_coordinator(replica3_id, replica3_addr);
            assert_eq!(r1_b.leader_id, Some(replica3_id),
                "Replica1 should recognize Replica3 as new leader");
        }

        // Verify: Replica2 should recognize Replica3 as leader
        {
            let mut r2_b = replica2_bully.lock().await;
            r2_b.on_coordinator(replica3_id, replica3_addr);
            assert_eq!(r2_b.leader_id, Some(replica3_id),
                "Replica2 should recognize Replica3 as new leader");
        }

        println!("Bully election test passed: Replica3 (ID={}) became coordinator", replica3_id);
    }
}

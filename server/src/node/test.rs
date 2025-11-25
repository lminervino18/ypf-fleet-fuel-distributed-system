// #[cfg(test)]
// mod node_test {
//     use crate::node::{Leader, Replica};
//     use std::net::{IpAddr, SocketAddr};
//     use std::thread;
//     use std::time::Duration;
//     use tokio::io::AsyncReadExt;
//     use tokio::task;
//     use tokio::{io::AsyncWriteExt, net::TcpStream};
//
//     /* #[test]
//     fn test_syncronization_between_one_leader_and_one_replica() {
//         let leader_addr = "127.0.0.1:12345";
//         let replica_addr = "127.0.0.1:12346";
//         let mut leader = std::process::Command::cargo_bin("server").unwrap()
//
//         let mut replica = std::process::Command::new(format!(
//             "cargo run --bin server -- replica --leader-addr=\"{leader_addr}\""
//         ))
//         .spawn()
//         .unwrap();
//
//         leader.wait().unwrap();
//         replica.wait().unwrap();
//     } */
//
//     /* #[tokio::test]
//     async fn test_syncronization_between_one_leader_and_one_replica() {
//         let dummy_coords = (0.0, 0.0);
//         let leader_addr = SocketAddr::new(IpAddr::V4([127, 0, 0, 1].into()), 12346);
//         let replica_addr = SocketAddr::new(IpAddr::V4([127, 0, 0, 1].into()), 12345);
//         let leader = tokio::spawn(async move {
//             Leader::start(leader_addr, dummy_coords, vec![replica_addr], 10)
//                 .await
//                 .unwrap();
//         });
//
//         let replica = tokio::spawn(async move {
//             Replica::start(replica_addr, dummy_coords, leader_addr, vec![], 10)
//                 .await
//                 .unwrap();
//         });
//
//         tokio::time::sleep(Duration::from_secs(1)).await; // wait for listener
//         let mut skt = TcpStream::connect(leader_addr).await.unwrap();
//         let op = Operation {
//             id: 1,
//             account_id: 1,
//             card_id: 1,
//             amount: 1.0,
//         };
//         let request: Vec<u8> = Message::Request {
//             op,
//             addr: skt.local_addr().unwrap(),
//         }
//         .into();
//         skt.write_all(&request).await.unwrap();
//         let mut buf = [0; 64];
//         let _ = skt.read(&mut buf).await.unwrap();
//         println!("buf read: {:?}", buf);
//         leader.abort();
//         replica.abort();
//     } */
// }
//
// #[cfg(test)]
// mod bully_election_test {
//     use crate::node::election::bully::Bully;
//     use crate::node::node::Node; // Import the Node trait
//     use crate::node::utils::get_id_given_addr;
//     use crate::node::{Leader, Replica};
//     use common::{Connection, Message};
//     use std::collections::{HashMap, VecDeque};
//     use std::net::{IpAddr, Ipv4Addr, SocketAddr};
//     use std::sync::Arc;
//     use tokio::sync::Mutex;
//     use tokio::time::{sleep, Duration};
//     /// Test Leader → Replica conversion with state preservation.
//     #[tokio::test]
//     async fn test_leader_to_replica_conversion() {
//         let current_leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 14100);
//         let new_leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 14101);
//
//         let current_leader_id = get_id_given_addr(current_leader_addr);
//         let new_leader_id = get_id_given_addr(new_leader_addr);
//
//         // Create a Leader with some state
//         let mut members = HashMap::new();
//         members.insert(current_leader_id, current_leader_addr);
//         members.insert(new_leader_id, new_leader_addr);
//
//         let bully = Arc::new(Mutex::new(Bully::new(
//             current_leader_id,
//             current_leader_addr,
//         )));
//         {
//             let mut b = bully.lock().await;
//             b.mark_coordinator(); // Mark as current coordinator
//         }
//
//         let mut leader = Leader::from_existing(
//             current_leader_id,
//             5, // current_op_id
//             (30.0, 40.0),
//             current_leader_addr,
//             members.clone(),
//             bully.clone(),
//             HashMap::new(), // operations from replica (not used by Leader)
//             false,
//             VecDeque::new(),
//         );
//
//         let replica = leader.into_replica(new_leader_addr);
//
//         assert_eq!(
//             replica.test_get_id(),
//             current_leader_id,
//             "[TEST] Replica ID matches former Leader ID"
//         );
//         assert_eq!(
//             replica.test_get_members().len(),
//             members.len(),
//             "[TEST] Replica members match former Leader members"
//         );
//         assert_eq!(
//             replica.test_get_operations(),
//             HashMap::new(),
//             "[TEST] Replica operations log is empty"
//         );
//     }
//
//     #[tokio::test]
//     async fn test_replica_to_leader_conversion() {
//         let replica_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 14000);
//         let leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 14001);
//
//         let replica_id = get_id_given_addr(replica_addr);
//         let leader_id = get_id_given_addr(leader_addr);
//
//         // Create a Replica with some state
//         let mut members = HashMap::new();
//         members.insert(replica_id, replica_addr);
//         members.insert(leader_id, leader_addr);
//
//         let bully = Arc::new(Mutex::new(Bully::new(replica_id, replica_addr)));
//
//         // Simulate some operations in the replica's log
//         let mut operations = HashMap::new();
//         operations.insert(
//             1,
//             common::operation::Operation::Charge {
//                 account_id: 100,
//                 card_id: 200,
//                 amount: 50.0,
//                 from_offline_station: false,
//             },
//         );
//         operations.insert(
//             2,
//             common::operation::Operation::Charge {
//                 account_id: 101,
//                 card_id: 201,
//                 amount: 75.0,
//                 from_offline_station: false,
//             },
//         );
//
//         let replica = Replica::from_existing(
//             replica_id,
//             (10.0, 20.0),
//             replica_addr,
//             leader_addr,
//             members.clone(),
//             bully.clone(),
//             operations.clone(),
//             false,
//             VecDeque::new(),
//         );
//
//         let leader = replica.into_leader();
//         assert_eq!(
//             leader.test_get_id(),
//             replica_id,
//             "[TEST] Replica ID matches"
//         );
//         assert_eq!(
//             leader.test_get_members().len(),
//             members.len(),
//             "[TEST] Leader members match former Replica members"
//         );
//     }
//
//     /// Test that Replica.handle_coordinator() correctly detects promotion to leader.
//     /// Creates a real Replica, calls handle_coordinator with its own ID, and verifies
//     /// that it returns RoleChange::PromoteToLeader and updates internal state.
//     #[tokio::test]
//     async fn test_bully_election_triggers_role_change() {
//         let replica_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 14200);
//         let old_leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 14201);
//
//         let replica_id = get_id_given_addr(replica_addr);
//         let old_leader_id = get_id_given_addr(old_leader_addr);
//
//         println!("\n[TEST] === Testing Replica promotion via handle_coordinator ===");
//         println!(
//             "[TEST] Replica ID: {}, Old Leader ID: {}",
//             replica_id, old_leader_id
//         );
//
//         // Create a real Replica instance with complete state
//         let mut members = HashMap::new();
//         members.insert(replica_id, replica_addr);
//         members.insert(old_leader_id, old_leader_addr);
//
//         let bully = Arc::new(Mutex::new(Bully::new(replica_id, replica_addr)));
//
//         // Start with election in progress
//         {
//             let mut b = bully.lock().await;
//             b.mark_start_election();
//             println!("[TEST] Election marked as started");
//         }
//
//         let mut replica = Replica::from_existing(
//             replica_id,
//             (10.0, 20.0),
//             replica_addr,
//             old_leader_addr,
//             members.clone(),
//             bully.clone(),
//             HashMap::new(),
//             false,
//             VecDeque::new(),
//         );
//
//         println!("[TEST] Created Replica with ID={}", replica_id);
//
//         // Create a Connection for handle_coordinator (won't be used but needed for signature)
//         let mut connection = Connection::start(replica_addr, 10)
//             .await
//             .expect("Failed to create connection");
//
//         // === PHASE 1: Receive Coordinator message with OWN ID (should promote) ===
//         println!(
//             "\n[TEST] PHASE 1: Calling handle_coordinator with own ID ({})",
//             replica_id
//         );
//
//         let role_change = replica
//             .handle_coordinator(
//                 &mut connection,
//                 replica_id, // Replica won election!
//                 replica_addr,
//             )
//             .await;
//
//         // Verify that PromoteToLeader was returned
//         match role_change {
//             crate::node::node::RoleChange::PromoteToLeader => {
//                 assert!(
//                     true,
//                     "[TEST] handle_coordinator correctly returned RoleChange::PromoteToLeader"
//                 );
//             }
//             other => {
//                 panic!("[TEST] Expected PromoteToLeader, got {:?}", other);
//             }
//         }
//
//         // Verify Bully state was updated correctly
//         {
//             let b = bully.lock().await;
//             assert_eq!(
//                 b.leader_id,
//                 Some(replica_id),
//                 "[TEST] Bully should recognize itself as leader"
//             );
//             assert_eq!(
//                 b.leader_addr,
//                 Some(replica_addr),
//                 "[TEST] Bully should have correct leader address"
//             );
//             assert!(
//                 !b.is_election_in_progress(),
//                 "[TEST] Election should be finished"
//             );
//             println!(
//                 "[TEST] Bully state correctly updated (leader_id={:?}, election_in_progress={})",
//                 b.leader_id,
//                 b.is_election_in_progress()
//             );
//         }
//
//         // === PHASE 2: Receive Coordinator from DIFFERENT node (should NOT promote) ===
//         println!("\n[TEST] PHASE 2: Calling handle_coordinator with different ID");
//
//         // Create a fresh replica to test the "not my ID" case
//         let bully2 = Arc::new(Mutex::new(Bully::new(replica_id, replica_addr)));
//         let mut replica2 = Replica::from_existing(
//             replica_id,
//             (10.0, 20.0),
//             replica_addr,
//             old_leader_addr,
//             members.clone(),
//             bully2.clone(),
//             HashMap::new(),
//             false,
//             VecDeque::new(),
//         );
//
//         let role_change_2 = replica2
//             .handle_coordinator(
//                 &mut connection,
//                 old_leader_id, // Different node won
//                 old_leader_addr,
//             )
//             .await;
//
//         // Verify that None was returned
//         match role_change_2 {
//             crate::node::node::RoleChange::None => {
//                 assert!(
//                     true,
//                     "[TEST] handle_coordinator correctly returned RoleChange::None"
//                 );
//             }
//             other => {
//                 panic!(
//                     "[TEST] Expected None when different node is coordinator, got {:?}",
//                     other
//                 );
//             }
//         }
//
//         // Verify Bully state was updated to new leader
//         {
//             let b = bully2.lock().await;
//             assert_eq!(
//                 b.leader_id,
//                 Some(old_leader_id),
//                 "[TEST] Bully should recognize new leader"
//             );
//             assert_eq!(
//                 b.leader_addr,
//                 Some(old_leader_addr),
//                 "[TEST] Bully should have new leader address"
//             );
//             println!(
//                 "[TEST] Bully state updated to new leader (leader_id={:?})",
//                 b.leader_id
//             );
//         }
//     }
//
//     /// Test that Leader.handle_coordinator() correctly detects demotion to replica.
//     /// Creates a real Leader, calls handle_coordinator with different ID, and verifies
//     /// that it returns RoleChange::DemoteToReplica and updates internal state.
//     #[tokio::test]
//     async fn test_leader_receives_coordinator_triggers_demotion() {
//         let current_leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 14300);
//         let new_leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 14301);
//
//         let current_leader_id = get_id_given_addr(current_leader_addr);
//         let new_leader_id = get_id_given_addr(new_leader_addr);
//
//         println!("\n[TEST] === Testing Leader demotion via handle_coordinator ===");
//         println!(
//             "[TEST] Current Leader ID: {}, New Leader ID: {}",
//             current_leader_id, new_leader_id
//         );
//
//         // Create a real Leader instance with complete state
//         let mut members = HashMap::new();
//         members.insert(current_leader_id, current_leader_addr);
//         members.insert(new_leader_id, new_leader_addr);
//
//         let bully = Arc::new(Mutex::new(Bully::new(
//             current_leader_id,
//             current_leader_addr,
//         )));
//
//         // Mark as current coordinator (this is the active leader)
//         {
//             let mut b = bully.lock().await;
//             b.mark_coordinator();
//             assert_eq!(
//                 b.leader_id,
//                 Some(current_leader_id),
//                 "[TEST] Should be current leader"
//             );
//             println!("[TEST] Leader marked as coordinator");
//         }
//
//         let mut leader = Leader::from_existing(
//             current_leader_id,
//             5, // current_op_id
//             (30.0, 40.0),
//             current_leader_addr,
//             members.clone(),
//             bully.clone(),
//             HashMap::new(),
//             false,
//             VecDeque::new(),
//         );
//
//         println!("[TEST] Created Leader with ID={}", current_leader_id);
//
//         // Create a Connection for handle_coordinator (won't be used but needed for signature)
//         let mut connection = Connection::start(current_leader_addr, 10)
//             .await
//             .expect("Failed to create connection");
//
//         // === PHASE 1: Receive Coordinator from DIFFERENT node (should demote) ===
//         println!(
//             "\n[TEST] PHASE 1: Calling handle_coordinator with different ID ({})",
//             new_leader_id
//         );
//         let role_change = leader
//             .handle_coordinator(
//                 &mut connection,
//                 new_leader_id, // Different node won election!
//                 new_leader_addr,
//             )
//             .await;
//
//         // Verify that DemoteToReplica was returned
//         match role_change {
//             crate::node::node::RoleChange::DemoteToReplica {
//                 new_leader_addr: addr,
//             } => {
//                 assert_eq!(
//                     addr, new_leader_addr,
//                     "[TEST] New leader address should match"
//                 );
//                 println!(
//                     "[TEST] handle_coordinator correctly returned RoleChange::DemoteToReplica"
//                 );
//             }
//             other => {
//                 panic!("[TEST] Expected DemoteToReplica, got {:?}", other);
//             }
//         }
//
//         // Verify Bully state was updated correctly
//         {
//             let b = bully.lock().await;
//             assert_eq!(
//                 b.leader_id,
//                 Some(new_leader_id),
//                 "[TEST] Bully should recognize new leader"
//             );
//             assert_eq!(
//                 b.leader_addr,
//                 Some(new_leader_addr),
//                 "[TEST] Bully should have new leader address"
//             );
//             assert!(
//                 !b.is_election_in_progress(),
//                 "[TEST] Election should be finished"
//             );
//             println!(
//                 "[TEST] Bully state correctly updated (leader_id={:?}, election_in_progress={})",
//                 b.leader_id,
//                 b.is_election_in_progress()
//             );
//         }
//
//         // === PHASE 2: Receive Coordinator with OWN ID (should NOT demote) ===
//         println!("\n[TEST] PHASE 2: Calling handle_coordinator with own ID");
//
//         // Create a fresh leader to test the "my ID" case
//         let bully2 = Arc::new(Mutex::new(Bully::new(
//             current_leader_id,
//             current_leader_addr,
//         )));
//         {
//             let mut b = bully2.lock().await;
//             b.mark_coordinator();
//         }
//
//         let mut leader2 = Leader::from_existing(
//             current_leader_id,
//             5,
//             (30.0, 40.0),
//             current_leader_addr,
//             members.clone(),
//             bully2.clone(),
//             HashMap::new(),
//             false,
//             VecDeque::new(),
//         );
//
//         let role_change_2 = leader2
//             .handle_coordinator(
//                 &mut connection,
//                 current_leader_id, // Same ID - re-announcing self as coordinator
//                 current_leader_addr,
//             )
//             .await;
//
//         // Verify that None was returned
//         match role_change_2 {
//             crate::node::node::RoleChange::None => {
//                 println!("[TEST] handle_coordinator correctly returned RoleChange::None");
//             }
//             other => {
//                 panic!(
//                     "[TEST] Expected None when same node is coordinator, got {:?}",
//                     other
//                 );
//             }
//         }
//
//         // Verify Bully state remains as current leader
//         {
//             let b = bully2.lock().await;
//             assert_eq!(
//                 b.leader_id,
//                 Some(current_leader_id),
//                 "[TEST] Bully should still recognize itself as leader"
//             );
//             assert_eq!(
//                 b.leader_addr,
//                 Some(current_leader_addr),
//                 "[TEST] Bully should keep its own address"
//             );
//             println!("[TEST] Bully state unchanged (leader_id={:?})", b.leader_id);
//         }
//
//         println!("\n[TEST] === All handle_coordinator assertions passed ===");
//     }
//
//     /// Integration test: Full Bully election with multiple replicas.
//     /// Tests the complete election flow including message passing, state updates,
//     /// and coordinator announcement.
//     #[tokio::test]
//     async fn test_full_bully_election_with_leader_and_three_replicas() {
//         println!("\n[TEST] === Full Bully Election Integration Test (Leader/Replica role transitions) ===");
//
//         // Addresses (choose ports so leader likely not highest ID)
//         let leader_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12000);
//         let replica1_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15001);
//         let replica2_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15002);
//         let replica3_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15003);
//
//         // Derive IDs
//         let leader_id = get_id_given_addr(leader_addr);
//         let replica1_id = get_id_given_addr(replica1_addr);
//         let replica2_id = get_id_given_addr(replica2_addr);
//         let replica3_id = get_id_given_addr(replica3_addr);
//
//         println!("[TEST] Initial IDs: leader={leader_id}, r1={replica1_id}, r2={replica2_id}, r3={replica3_id}");
//
//         // Highest ID should win election
//         let highest_id = [leader_id, replica1_id, replica2_id, replica3_id]
//             .iter()
//             .max()
//             .copied()
//             .unwrap();
//
//         // Precondition: leader should not already be highest for role change scenario
//         assert!(leader_id != highest_id, "[TEST] Precondición fallida: el líder inicial tiene el ID más alto, no habrá cambio de rol");
//
//         // Shared membership map
//         let mut members = HashMap::new();
//         members.insert(leader_id, leader_addr);
//         members.insert(replica1_id, replica1_addr);
//         members.insert(replica2_id, replica2_addr);
//         members.insert(replica3_id, replica3_addr);
//
//         // Bully instances
//         let leader_bully = Arc::new(Mutex::new(Bully::new(leader_id, leader_addr)));
//         let replica1_bully = Arc::new(Mutex::new(Bully::new(replica1_id, replica1_addr)));
//         let replica2_bully = Arc::new(Mutex::new(Bully::new(replica2_id, replica2_addr)));
//         let replica3_bully = Arc::new(Mutex::new(Bully::new(replica3_id, replica3_addr)));
//
//         // Construct Leader and Replicas
//         let mut leader = Leader::from_existing(
//             leader_id,
//             0,
//             (0.0, 0.0),
//             leader_addr,
//             members.clone(),
//             leader_bully.clone(),
//             HashMap::new(),
//             false,
//             VecDeque::new(),
//         );
//         let mut replica1 = Replica::from_existing(
//             replica1_id,
//             (0.0, 0.0),
//             replica1_addr,
//             leader_addr,
//             members.clone(),
//             replica1_bully.clone(),
//             HashMap::new(),
//             false,
//             VecDeque::new(),
//         );
//         let mut replica2 = Replica::from_existing(
//             replica2_id,
//             (0.0, 0.0),
//             replica2_addr,
//             leader_addr,
//             members.clone(),
//             replica2_bully.clone(),
//             HashMap::new(),
//             false,
//             VecDeque::new(),
//         );
//         let mut replica3 = Replica::from_existing(
//             replica3_id,
//             (0.0, 0.0),
//             replica3_addr,
//             leader_addr,
//             members.clone(),
//             replica3_bully.clone(),
//             HashMap::new(),
//             false,
//             VecDeque::new(),
//         );
//
//         // Connections for nodes (only needed for election + coordinator handling)
//         let mut replica1_conn = Connection::start(replica1_addr, 10)
//             .await
//             .expect("replica1 conn");
//         let mut replica2_conn = Connection::start(replica2_addr, 10)
//             .await
//             .expect("replica2 conn");
//         let mut replica3_conn = Connection::start(replica3_addr, 10)
//             .await
//             .expect("replica3 conn");
//         let mut leader_conn = Connection::start(leader_addr, 10)
//             .await
//             .expect("leader conn");
//
//         // Pick winner replica (highest id among replicas)
//         let mut replicas_info = vec![
//             (replica1_id, replica1_addr, 1u8),
//             (replica2_id, replica2_addr, 2u8),
//             (replica3_id, replica3_addr, 3u8),
//         ];
//         replicas_info.sort_by_key(|(id, _, _)| *id);
//         let (winner_id, winner_addr, winner_tag) = *replicas_info.last().unwrap();
//         assert_eq!(
//             winner_id, highest_id,
//             "[TEST] El mayor ID debe ganar la elección"
//         );
//         println!("[TEST] Winner candidate replica should be tag={winner_tag} id={winner_id}");
//
//         // Start election from winner replica using its start_election
//         match winner_tag {
//             1 => replica1.start_election(&mut replica1_conn).await,
//             2 => replica2.start_election(&mut replica2_conn).await,
//             3 => replica3.start_election(&mut replica3_conn).await,
//             _ => unreachable!(),
//         }
//         sleep(Duration::from_millis(300)).await; // wait election window
//
//         // Manual propagation of Coordinator message into handlers (simulate network dispatch)
//         // Winner replica processes its own Coordinator (promotion)
//         let promote_change = match winner_tag {
//             1 => {
//                 replica1
//                     .handle_coordinator(&mut replica1_conn, winner_id, winner_addr)
//                     .await
//             }
//             2 => {
//                 replica2
//                     .handle_coordinator(&mut replica2_conn, winner_id, winner_addr)
//                     .await
//             }
//             3 => {
//                 replica3
//                     .handle_coordinator(&mut replica3_conn, winner_id, winner_addr)
//                     .await
//             }
//             _ => unreachable!(),
//         };
//         println!(
//             "[TEST] promote_change={promote_change:?}, corresponding to winner tag={}",
//             winner_tag
//         );
//         assert!(matches!(
//             promote_change,
//             crate::node::node::RoleChange::PromoteToLeader
//         ));
//
//         // Old leader processes Coordinator (demotion)
//         let demote_change = leader
//             .handle_coordinator(&mut leader_conn, winner_id, winner_addr)
//             .await;
//         println!("[TEST] demote_change={demote_change:?}");
//         assert!(matches!(
//             demote_change,
//             crate::node::node::RoleChange::DemoteToReplica { .. }
//         ));
//
//         // Other replicas process Coordinator (no role change)
//         let rc_non_winner_1 = if winner_tag != 1 {
//             replica1
//                 .handle_coordinator(&mut replica1_conn, winner_id, winner_addr)
//                 .await
//         } else {
//             crate::node::node::RoleChange::None
//         };
//         let rc_non_winner_2 = if winner_tag != 2 {
//             replica2
//                 .handle_coordinator(&mut replica2_conn, winner_id, winner_addr)
//                 .await
//         } else {
//             crate::node::node::RoleChange::None
//         };
//         let rc_non_winner_3 = if winner_tag != 3 {
//             replica3
//                 .handle_coordinator(&mut replica3_conn, winner_id, winner_addr)
//                 .await
//         } else {
//             crate::node::node::RoleChange::None
//         };
//
//         // Ensure other replicas reported no role change (non-winner replicas)
//         assert!(matches!(
//             rc_non_winner_1,
//             crate::node::node::RoleChange::None
//         ));
//         assert!(matches!(
//             rc_non_winner_2,
//             crate::node::node::RoleChange::None
//         ));
//         assert!(matches!(
//             rc_non_winner_3,
//             crate::node::node::RoleChange::None
//         ));
//
//         // El ganador aún es Replica aquí; su `leader_addr` no se actualiza hasta la conversión.
//         // Solo verificamos que las réplicas no ganadoras apuntan al nuevo líder.
//         if winner_tag != 1 {
//             assert_eq!(
//                 replica1.test_get_leader_id(),
//                 winner_id,
//                 "[TEST] Replica1 referencia líder correcto"
//             );
//         }
//         if winner_tag != 2 {
//             assert_eq!(
//                 replica2.test_get_leader_id(),
//                 winner_id,
//                 "[TEST] Replica2 referencia líder correcto"
//             );
//         }
//         if winner_tag != 3 {
//             assert_eq!(
//                 replica3.test_get_leader_id(),
//                 winner_id,
//                 "[TEST] Replica3 referencia líder correcto"
//             );
//         }
//
//         // Convert winner replica to Leader (consume winner)
//         let new_leader = match promote_change {
//             crate::node::node::RoleChange::PromoteToLeader => match winner_tag {
//                 1 => replica1.into_leader(),
//                 2 => replica2.into_leader(),
//                 3 => replica3.into_leader(),
//                 _ => unreachable!(),
//             },
//             other => panic!("[TEST] Esperaba PromoteToLeader, obtuve {:?}", other),
//         };
//
//         // Convert old leader to replica
//         let demoted_leader_replica = match demote_change {
//             crate::node::node::RoleChange::DemoteToReplica { new_leader_addr } => {
//                 assert_eq!(
//                     new_leader_addr, winner_addr,
//                     "[TEST] Dirección del nuevo líder incorrecta"
//                 );
//                 leader.into_replica(winner_addr)
//             }
//             other => panic!("[TEST] Esperaba DemoteToReplica, obtuve {:?}", other),
//         };
//
//         println!("[TEST] Verificando IDs y liderazgo tras conversion de roles...");
//         assert_eq!(
//             new_leader.test_get_id(),
//             winner_id,
//             "[TEST] Nuevo líder conserva ID ganador"
//         );
//         assert_eq!(
//             demoted_leader_replica.test_get_leader_id(),
//             winner_id,
//             "[TEST] Ex-líder ahora replica referencia nuevo líder"
//         );
//         assert_eq!(
//             demoted_leader_replica.test_get_id(),
//             leader_id,
//             "[TEST] Ex-líder mantiene su ID propio"
//         );
//     }
// }

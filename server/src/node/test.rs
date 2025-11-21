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

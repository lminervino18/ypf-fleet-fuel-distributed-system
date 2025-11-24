// tests/src/helpers.rs

use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

/// Handle for a test cluster:
/// - one leader
/// - N replicas
///
/// You get the addresses and the Child processes to poder matarlos al final del test.
pub struct ClusterProcesses {
    pub leader_addr: String,
    pub replica_addrs: Vec<String>,
    pub leader: Child,
    pub replicas: Vec<Child>,
}

impl ClusterProcesses {
    /// Best-effort shutdown of all processes.
    pub fn kill_all(&mut self) {
        // Kill replicas first
        for child in &mut self.replicas {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.replicas.clear();

        // Then leader
        let _ = self.leader.kill();
        let _ = self.leader.wait();
    }
}

/// Spawn a leader + `num_replicas` replicas.
///
/// Layout:
/// - Leader:  127.0.0.1:5100
/// - Replica: 127.0.0.1:5101, 5102, ...
///
/// Steps:
/// 1) Levanta leader.
/// 2) Espera 1s.
/// 3) Levanta N replicas.
/// 4) Espera 3s para que el cluster se estabilice.
/// 5) Devuelve las direcciones + los Child para cleanup.
pub fn spawn_cluster(num_replicas: usize) -> ClusterProcesses {
    let leader_port: u16 = 5100;
    let replica_base_port: u16 = 5101;

    let leader_addr = format!("127.0.0.1:{leader_port}");

    eprintln!("[TEST] Spawning LEADER at {leader_addr}...");

    let leader = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",  // crate del server
            "--bin",
            "server",  // bin del server
            "--",
            "--address",
            &leader_addr,
            "--pumps",
            "4",
            "leader",
            "--max-conns",
            "16",
        ])
        // Para tests: dejamos logs del server visibles.
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("[TEST] failed to spawn leader process");

    // Pequeña espera para que bindeé el puerto.
    eprintln!("[TEST] Leader spawned, sleeping 1s before replicas...");
    thread::sleep(Duration::from_secs(1));

    // Réplicas
    let mut replicas = Vec::new();
    let mut replica_addrs = Vec::new();

    for i in 0..num_replicas {
        let port = replica_base_port + i as u16;
        let addr = format!("127.0.0.1:{port}");
        eprintln!(
            "[TEST] Spawning REPLICA {} at {} (leader={})...",
            i, addr, leader_addr
        );

        let child = Command::new("cargo")
            .args([
                "run",
                "-p",
                "server",
                "--bin",
                "server",
                "--",
                "--address",
                &addr,
                "--pumps",
                "4",
                "replica",
                "--leader-addr",
                &leader_addr,
                "--max-conns",
                "16",
            ])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("[TEST] failed to spawn replica process");

        replicas.push(child);
        replica_addrs.push(addr);
    }

    // Esperar a que el cluster se “forme” y se haga Join/ClusterView.
    eprintln!("[TEST] Cluster spawned (leader + {num_replicas} replicas). Sleeping 3s for stabilization...");
    thread::sleep(Duration::from_secs(3));

    ClusterProcesses {
        leader_addr,
        replica_addrs,
        leader,
        replicas,
    }
}

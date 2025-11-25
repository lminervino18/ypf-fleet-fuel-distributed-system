// tests/src/cluster_smoke.rs

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Pick a free TCP port on 127.0.0.1 by binding to port 0 and reading the assigned port.
fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .expect("failed to bind to 127.0.0.1:0 to pick a free port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}


/// Wait until a TCP port is accepting connections, or timeout.
fn wait_for_port(addr: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        if TcpStream::connect(addr).is_ok() {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

/// Minimal smoke test:
/// - start a leader node (no replicas)
/// - start a node_client
/// - send one charge from pump 0
/// - expect at least one "CHARGE OK" in node_client stdout
#[test]
fn cluster_smoke_node_client_single_charge() {
    const LEADER_ADDR: &str = "127.0.0.1:5100";

    // Free port for the node_client local bind
    let nc_port = pick_free_port();
    let nc_bind_addr = format!("127.0.0.1:{nc_port}");

    // 1) Start leader
    let mut leader = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",
            "--bin",
            "server",
            "--",
            "--address",
            LEADER_ADDR,
            "--pumps",
            "4",
            "leader",
            "--max-conns",
            "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn leader process");

    // Wait until leader is listening
    let ready = wait_for_port(LEADER_ADDR, Duration::from_secs(10));
    if !ready {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("leader did not start listening on {LEADER_ADDR} in time");
    }

    // If leader died early, fail fast
    if let Ok(Some(status)) = leader.try_wait() {
        panic!("leader exited earlier than expected with status: {status:?}");
    }

    // 2) Start node_client
    let mut node_client = Command::new("cargo")
        .args([
            "run",
            "-p",
            "client",
            "--bin",
            "node_client",
            "--",
            &nc_bind_addr,   // bind_addr
            "16",            // max_connections
            "4",             // num_pumps
            LEADER_ADDR,     // known_node_1
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn node_client process");

    // 3) Send one pump command + exit
    {
        let stdin = node_client
            .stdin
            .as_mut()
            .expect("failed to open stdin for node_client");

        // One simple charge that should be OK
        writeln!(stdin, "0 100 10 50.0")
            .expect("failed to write charge line to node_client stdin");

        // Small delay to let it travel & be processed
        thread::sleep(Duration::from_millis(400));

        // Clean exit so simulator closes
        writeln!(stdin, "exit")
            .expect("failed to write exit to node_client stdin");
    }

    // 4) Wait for node_client and capture stdout
    let output = node_client
        .wait_with_output()
        .expect("failed to wait for node_client");

    // If node_client failed, show stderr in the panic
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("node_client exited with failure status: {:?}\nstderr:\n{stderr}",
               output.status);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // 5) Assert at least one CHARGE OK in Station output
    let ok_count = stdout.matches("CHARGE OK").count();
    assert!(
        ok_count >= 1,
        "expected at least 1 CHARGE OK line, found {ok_count}; stdout:\n{stdout}"
    );

    // 6) Cleanup leader
    let _ = leader.kill();
    let _ = leader.wait();
}

/// Smoke test con cluster de 2 nodos:
/// - start leader en un puerto libre
/// - start replica en otro puerto libre (apuntando al leader)
/// - esperar que ambos estén escuchando y que el cluster tenga tiempo de formarse
/// - start node_client con known_nodes = [leader, replica]
/// - mandar un CHARGE y luego exit
/// - esperar al menos un "CHARGE OK" en el stdout del node_client
#[test]
fn cluster_smoke_leader_with_replica_node_client_single_charge() {
    println!("===== [TEST] cluster_smoke_leader_with_replica_node_client_single_charge START =====");

    // Puertos libres para leader, replica y node_client
    let leader_port = pick_free_port();
    let replica_port = pick_free_port();
    let nc_port = pick_free_port();

    let leader_addr = format!("127.0.0.1:{leader_port}");
    let replica_addr = format!("127.0.0.1:{replica_port}");
    let nc_bind_addr = format!("127.0.0.1:{nc_port}");

    println!("[TEST] leader_addr  = {leader_addr}");
    println!("[TEST] replica_addr = {replica_addr}");
    println!("[TEST] nc_bind_addr = {nc_bind_addr}");

    // -------------------------
    // 1) Start LEADER
    // -------------------------
    let mut leader = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",
            "--bin",
            "server",
            "--",
            "--address",
            &leader_addr,
            "--pumps",
            "4",
            "leader",
            "--max-conns",
            "16",
        ])
        .stdout(Stdio::null())   // no queremos loguear nada del server en el test
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn leader process");

    // Esperar a que el leader esté escuchando
    let ready = wait_for_port(&leader_addr, Duration::from_secs(10));
    if !ready {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Leader did not start listening on {leader_addr} in time");
    }

    if let Ok(Some(status)) = leader.try_wait() {
        panic!("[TEST] Leader exited earlier than expected with status: {status:?}");
    }

    // -------------------------
    // 2) Start REPLICA
    // -------------------------
    let mut replica = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",
            "--bin",
            "server",
            "--",
            "--address",
            &replica_addr,
            "--pumps",
            "4",
            "replica",
            "--leader-addr",
            &leader_addr,
            "--max-conns",
            "16",
        ])
        .stdout(Stdio::null())   // sin logs del server
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn replica process");

    // Esperar a que la réplica esté escuchando
    let ready_replica = wait_for_port(&replica_addr, Duration::from_secs(10));
    if !ready_replica {
        let _ = replica.kill();
        let _ = replica.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica did not start listening on {replica_addr} in time");
    }

    if let Ok(Some(status)) = replica.try_wait() {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica exited earlier than expected with status: {status:?}");
    }

    // Pequeño colchón para que la réplica haga Join y el leader actualice el cluster.
    thread::sleep(Duration::from_secs(2));

    // -------------------------
    // 3) Start NODE_CLIENT
    // -------------------------
    let mut node_client = Command::new("cargo")
        .args([
            "run",
            "-p",
            "client",
            "--bin",
            "node_client",
            "--",
            &nc_bind_addr,           // bind_addr
            "16",                    // max_connections
            "4",                     // num_pumps
            &leader_addr,            // known_node_1
            &replica_addr,           // known_node_2
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())     // acá sí, queremos ver el output del Station
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn node_client process");

    // -------------------------
    // 4) Mandar un CHARGE + exit
    // -------------------------
    {
        let stdin = node_client
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for node_client");

        // Carga simple que debería dar OK
        writeln!(stdin, "0 100 10 50.0")
            .expect("[TEST] failed to write charge to node_client stdin");

        // Dejamos un ratito para que viaje al cluster y vuelva la respuesta
        thread::sleep(Duration::from_millis(400));

        // Cerramos prolijo el simulador
        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to node_client stdin");
    }

    // -------------------------
    // 5) Esperar node_client y revisar stdout
    // -------------------------
    let output = node_client
        .wait_with_output()
        .expect("[TEST] failed to wait for node_client");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // aunque stderr está en null, por las dudas
        panic!(
            "[TEST] node_client exited with failure status: {:?}\nstderr:\n{stderr}",
            output.status
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    let ok_count = stdout.matches("CHARGE OK").count();
    assert!(
        ok_count >= 1,
        "expected at least 1 CHARGE OK line with leader+replica, found {ok_count}; stdout:\n{stdout}"
    );

    // -------------------------
    // 6) Cleanup: matar replica y leader
    // -------------------------
    let _ = replica.kill();
    let _ = replica.wait();
    let _ = leader.kill();
    let _ = leader.wait();

    println!("===== [TEST] cluster_smoke_leader_with_replica_node_client_single_charge END =====");
}

/// Smoke test con cluster de 3 nodos:
/// - leader + 2 replicas + 1 node_client
/// - el node_client conoce los 3 nodos
/// - se manda un solo CHARGE y se espera al menos un "CHARGE OK"
#[test]
fn cluster_smoke_leader_with_two_replicas_node_client_single_charge() {
    println!("===== [TEST] cluster_smoke_leader_with_two_replicas_node_client_single_charge START =====");

    // Puertos libres para leader, replicas y node_client
    let leader_port = pick_free_port();
    let replica1_port = pick_free_port();
    let replica2_port = pick_free_port();
    let nc_port = pick_free_port();

    let leader_addr = format!("127.0.0.1:{leader_port}");
    let replica1_addr = format!("127.0.0.1:{replica1_port}");
    let replica2_addr = format!("127.0.0.1:{replica2_port}");
    let nc_bind_addr = format!("127.0.0.1:{nc_port}");

    println!("[TEST] leader_addr   = {leader_addr}");
    println!("[TEST] replica1_addr = {replica1_addr}");
    println!("[TEST] replica2_addr = {replica2_addr}");
    println!("[TEST] nc_bind_addr  = {nc_bind_addr}");

    // -------------------------
    // 1) Start LEADER
    // -------------------------
    let mut leader = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",
            "--bin",
            "server",
            "--",
            "--address",
            &leader_addr,
            "--pumps",
            "4",
            "leader",
            "--max-conns",
            "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn leader process");

    let ready_leader = wait_for_port(&leader_addr, Duration::from_secs(10));
    if !ready_leader {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Leader did not start listening on {leader_addr} in time");
    }

    if let Ok(Some(status)) = leader.try_wait() {
        panic!("[TEST] Leader exited earlier than expected with status: {status:?}");
    }

    // -------------------------
    // 2) Start REPLICA 1
    // -------------------------
    let mut replica1 = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",
            "--bin",
            "server",
            "--",
            "--address",
            &replica1_addr,
            "--pumps",
            "4",
            "replica",
            "--leader-addr",
            &leader_addr,
            "--max-conns",
            "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn replica1 process");

    let ready_replica1 = wait_for_port(&replica1_addr, Duration::from_secs(10));
    if !ready_replica1 {
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica1 did not start listening on {replica1_addr} in time");
    }

    if let Ok(Some(status)) = replica1.try_wait() {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica1 exited earlier than expected with status: {status:?}");
    }

    // -------------------------
    // 3) Start REPLICA 2
    // -------------------------
    let mut replica2 = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",
            "--bin",
            "server",
            "--",
            "--address",
            &replica2_addr,
            "--pumps",
            "4",
            "replica",
            "--leader-addr",
            &leader_addr,
            "--max-conns",
            "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn replica2 process");

    let ready_replica2 = wait_for_port(&replica2_addr, Duration::from_secs(10));
    if !ready_replica2 {
        let _ = replica2.kill();
        let _ = replica2.wait();
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica2 did not start listening on {replica2_addr} in time");
    }

    if let Ok(Some(status)) = replica2.try_wait() {
        let _ = replica2.kill();
        let _ = replica2.wait();
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica2 exited earlier than expected with status: {status:?}");
    }

    // Colchón para que ambas réplicas hagan Join y el leader actualice el cluster.
    thread::sleep(Duration::from_secs(2));

    // -------------------------
    // 4) Start NODE_CLIENT
    // -------------------------
    let mut node_client = Command::new("cargo")
        .args([
            "run",
            "-p",
            "client",
            "--bin",
            "node_client",
            "--",
            &nc_bind_addr,      // bind_addr
            "16",               // max_connections
            "4",                // num_pumps
            &leader_addr,       // known_node_1
            &replica1_addr,     // known_node_2
            &replica2_addr,     // known_node_3
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn node_client process");

    // -------------------------
    // 5) Mandar un CHARGE + exit
    // -------------------------
    {
        let stdin = node_client
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for node_client");

        writeln!(stdin, "0 100 10 50.0")
            .expect("[TEST] failed to write charge to node_client stdin");

        thread::sleep(Duration::from_millis(400));

        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to node_client stdin");
    }

    // -------------------------
    // 6) Esperar node_client y revisar stdout
    // -------------------------
    let output = node_client
        .wait_with_output()
        .expect("[TEST] failed to wait for node_client");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "[TEST] node_client exited with failure status: {:?}\nstderr:\n{stderr}",
            output.status
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let ok_count = stdout.matches("CHARGE OK").count();

    assert!(
        ok_count >= 1,
        "expected at least 1 CHARGE OK line with leader+2 replicas, found {ok_count}; stdout:\n{stdout}"
    );

    // -------------------------
    // 7) Cleanup
    // -------------------------
    let _ = replica2.kill();
    let _ = replica2.wait();
    let _ = replica1.kill();
    let _ = replica1.wait();
    let _ = leader.kill();
    let _ = leader.wait();

    println!("===== [TEST] cluster_smoke_leader_with_two_replicas_node_client_single_charge END =====");
}

/// Escenario:
/// - Leader + 2 replicas
/// - Administrator fija un límite de cuenta bajo (80.0) sobre account_id=100
/// - node_client envía 2 charges:
///     * 50.0  → dentro del límite → CHARGE OK
///     * 40.0  → total 90.0 > 80.0 → CHARGE DENIED (por límite de cuenta)
#[test]
fn cluster_smoke_limit_account_then_exceeding_charge_is_denied() {
    println!("===== [TEST] cluster_smoke_limit_account_then_exceeding_charge_is_denied START =====");

    // Puertos libres para leader, replicas, admin y node_client
    let leader_port = pick_free_port();
    let replica1_port = pick_free_port();
    let replica2_port = pick_free_port();
    let admin_port = pick_free_port();
    let nc_port = pick_free_port();

    let leader_addr = format!("127.0.0.1:{leader_port}");
    let replica1_addr = format!("127.0.0.1:{replica1_port}");
    let replica2_addr = format!("127.0.0.1:{replica2_port}");
    let admin_bind_addr = format!("127.0.0.1:{admin_port}");
    let nc_bind_addr = format!("127.0.0.1:{nc_port}");

    println!("[TEST] leader_addr      = {leader_addr}");
    println!("[TEST] replica1_addr    = {replica1_addr}");
    println!("[TEST] replica2_addr    = {replica2_addr}");
    println!("[TEST] admin_bind_addr  = {admin_bind_addr}");
    println!("[TEST] nc_bind_addr     = {nc_bind_addr}");

    // -------------------------
    // 1) Start LEADER
    // -------------------------
    let mut leader = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",
            "--bin",
            "server",
            "--",
            "--address",
            &leader_addr,
            "--pumps",
            "4",
            "leader",
            "--max-conns",
            "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn leader process");

    let ready_leader = wait_for_port(&leader_addr, Duration::from_secs(10));
    if !ready_leader {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Leader did not start listening on {leader_addr} in time");
    }

    if let Ok(Some(status)) = leader.try_wait() {
        panic!("[TEST] Leader exited earlier than expected with status: {status:?}");
    }

    // -------------------------
    // 2) Start REPLICA 1
    // -------------------------
    let mut replica1 = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",
            "--bin",
            "server",
            "--",
            "--address",
            &replica1_addr,
            "--pumps",
            "4",
            "replica",
            "--leader-addr",
            &leader_addr,
            "--max-conns",
            "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn replica1 process");

    let ready_replica1 = wait_for_port(&replica1_addr, Duration::from_secs(10));
    if !ready_replica1 {
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica1 did not start listening on {replica1_addr} in time");
    }

    if let Ok(Some(status)) = replica1.try_wait() {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica1 exited earlier than expected with status: {status:?}");
    }

    // -------------------------
    // 3) Start REPLICA 2
    // -------------------------
    let mut replica2 = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",
            "--bin",
            "server",
            "--",
            "--address",
            &replica2_addr,
            "--pumps",
            "4",
            "replica",
            "--leader-addr",
            &leader_addr,
            "--max-conns",
            "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn replica2 process");

    let ready_replica2 = wait_for_port(&replica2_addr, Duration::from_secs(10));
    if !ready_replica2 {
        let _ = replica2.kill();
        let _ = replica2.wait();
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica2 did not start listening on {replica2_addr} in time");
    }

    if let Ok(Some(status)) = replica2.try_wait() {
        let _ = replica2.kill();
        let _ = replica2.wait();
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica2 exited earlier than expected with status: {status:?}");
    }

    // Dejar que las réplicas hagan Join y el líder actualice cluster
    thread::sleep(Duration::from_secs(2));

    // -------------------------
    // 4) Administrator: setea límite de cuenta = 80.0 para account_id=100
    // -------------------------
    let mut admin = Command::new("cargo")
        .args([
            "run",
            "-p",
            "client",
            "--bin",
            "administrator",
            "--",
            &admin_bind_addr,  // admin bind_addr
            &leader_addr,      // target_node_addr
            "100",             // account_id
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn administrator process");

    {
        let stdin = admin
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for administrator");

        // Seteamos límite de cuenta al 80.0
        writeln!(stdin, "limit-account 80.0")
            .expect("[TEST] failed to write limit-account to admin stdin");

        // Cerramos el admin
        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to admin stdin");
    }

    let admin_output = admin
        .wait_with_output()
        .expect("[TEST] failed to wait for administrator");

    if !admin_output.status.success() {
        let stderr = String::from_utf8_lossy(&admin_output.stderr);
        // Aunque stdout lo descartamos, stderr lo mostramos si falla
        panic!(
            "[TEST] administrator exited with failure status: {:?}\nSTDERR:\n{stderr}",
            admin_output.status
        );
    }

    // Pequeño colchón para que el límite se replique / se aplique bien en el cluster
    thread::sleep(Duration::from_millis(500));

    // -------------------------
    // 5) Start NODE_CLIENT
    // -------------------------
    let mut node_client = Command::new("cargo")
        .args([
            "run",
            "-p",
            "client",
            "--bin",
            "node_client",
            "--",
            &nc_bind_addr,      // bind_addr
            "16",               // max_connections
            "4",                // num_pumps
            &leader_addr,       // known_node_1
            &replica1_addr,     // known_node_2
            &replica2_addr,     // known_node_3
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped()) // queremos ver los CHARGE OK / DENIED
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn node_client process");

    // -------------------------
    // 6) Mandar 2 cargos:
    //    - 50.0 (OK)
    //    - 40.0 (debería ser DENIED porque supera límite 80.0)
    //    - luego exit
    // -------------------------
    {
        let stdin = node_client
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for node_client");

        // Primer charge: 50.0 (dentro del límite)
        writeln!(stdin, "0 100 10 50.0")
            .expect("[TEST] failed to write first charge to node_client stdin");

        thread::sleep(Duration::from_millis(300));

        // Segundo charge: 40.0 (total 90.0 > 80.0 => debería fallar)
        writeln!(stdin, "1 100 10 40.0")
            .expect("[TEST] failed to write second charge to node_client stdin");

        thread::sleep(Duration::from_millis(300));

        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to node_client stdin");
    }

    // -------------------------
    // 7) Esperar node_client y revisar stdout
    // -------------------------
    let output = node_client
        .wait_with_output()
        .expect("[TEST] failed to wait for node_client");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "[TEST] node_client exited with failure status: {:?}\nstderr:\n{stderr}",
            output.status
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("--- [node_client STDOUT] ---\n{stdout}");

    let ok_count = stdout.matches("CHARGE OK").count();
    let denied_count = stdout.matches("CHARGE DENIED").count();

    assert!(
        ok_count >= 1,
        "expected at least 1 CHARGE OK line (first charge), found {ok_count}; stdout:\n{stdout}"
    );

    assert!(
        denied_count >= 1,
        "expected at least 1 CHARGE DENIED line (second charge should exceed limit), found {denied_count}; stdout:\n{stdout}"
    );

    // -------------------------
    // 8) Cleanup
    // -------------------------
    let _ = replica2.kill();
    let _ = replica2.wait();
    let _ = replica1.kill();
    let _ = replica1.wait();
    let _ = leader.kill();
    let _ = leader.wait();

    println!("===== [TEST] cluster_smoke_limit_account_then_exceeding_charge_is_denied END =====");
}

/// Escenario:
/// - Leader + 2 replicas
/// - Dos node_client distintos generan consumo sobre la misma cuenta:
///     * node_client 1: card 10 → 30.0 + 50.0 = 80.0
///     * node_client 2: card 20 → 70.0 + 50.0 = 120.0
///   → total_spent esperado = 200.0
/// - Luego un administrator hace `account-query` sobre account_id=100
/// - Se chequea que el resultado refleje esos valores.
#[test]
fn cluster_smoke_multiple_clients_then_account_query() {
    println!("===== [TEST] cluster_smoke_multiple_clients_then_account_query START =====");

    // Puertos libres para leader, replicas, admin y dos node_clients
    let leader_port = pick_free_port();
    let replica1_port = pick_free_port();
    let replica2_port = pick_free_port();
    let admin_port = pick_free_port();
    let nc1_port = pick_free_port();
    let nc2_port = pick_free_port();

    let leader_addr     = format!("127.0.0.1:{leader_port}");
    let replica1_addr   = format!("127.0.0.1:{replica1_port}");
    let replica2_addr   = format!("127.0.0.1:{replica2_port}");
    let admin_bind_addr = format!("127.0.0.1:{admin_port}");
    let nc1_bind_addr   = format!("127.0.0.1:{nc1_port}");
    let nc2_bind_addr   = format!("127.0.0.1:{nc2_port}");

    println!("[TEST] leader_addr     = {leader_addr}");
    println!("[TEST] replica1_addr   = {replica1_addr}");
    println!("[TEST] replica2_addr   = {replica2_addr}");
    println!("[TEST] admin_bind_addr = {admin_bind_addr}");
    println!("[TEST] nc1_bind_addr   = {nc1_bind_addr}");
    println!("[TEST] nc2_bind_addr   = {nc2_bind_addr}");

    // -------------------------
    // 1) Leader
    // -------------------------
    let mut leader = Command::new("cargo")
        .args([
            "run", "-p", "server", "--bin", "server",
            "--",
            "--address", &leader_addr,
            "--pumps", "4",
            "leader",
            "--max-conns", "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn leader process");

    let ready_leader = wait_for_port(&leader_addr, Duration::from_secs(10));
    if !ready_leader {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Leader did not start listening on {leader_addr} in time");
    }
    if let Ok(Some(status)) = leader.try_wait() {
        panic!("[TEST] Leader exited earlier than expected with status: {status:?}");
    }

    // -------------------------
    // 2) Réplica 1
    // -------------------------
    let mut replica1 = Command::new("cargo")
        .args([
            "run", "-p", "server", "--bin", "server",
            "--",
            "--address", &replica1_addr,
            "--pumps", "4",
            "replica",
            "--leader-addr", &leader_addr,
            "--max-conns", "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn replica1 process");

    let ready_replica1 = wait_for_port(&replica1_addr, Duration::from_secs(10));
    if !ready_replica1 {
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica1 did not start listening on {replica1_addr} in time");
    }
    if let Ok(Some(status)) = replica1.try_wait() {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica1 exited earlier than expected with status: {status:?}");
    }

    // -------------------------
    // 3) Réplica 2
    // -------------------------
    let mut replica2 = Command::new("cargo")
        .args([
            "run", "-p", "server", "--bin", "server",
            "--",
            "--address", &replica2_addr,
            "--pumps", "4",
            "replica",
            "--leader-addr", &leader_addr,
            "--max-conns", "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn replica2 process");

    let ready_replica2 = wait_for_port(&replica2_addr, Duration::from_secs(10));
    if !ready_replica2 {
        let _ = replica2.kill();
        let _ = replica2.wait();
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica2 did not start listening on {replica2_addr} in time");
    }
    if let Ok(Some(status)) = replica2.try_wait() {
        let _ = replica2.kill();
        let _ = replica2.wait();
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica2 exited earlier than expected with status: {status:?}");
    }

    // Dejar que las réplicas hagan Join y el leader actualice cluster
    thread::sleep(Duration::from_secs(2));

    // -------------------------
    // 4) node_client 1: card 10 → 30.0 + 50.0
    // -------------------------
    let mut node_client1 = Command::new("cargo")
        .args([
            "run", "-p", "client", "--bin", "node_client",
            "--",
            &nc1_bind_addr,
            "16", // max_connections
            "4",  // num_pumps
            &leader_addr,
            &replica1_addr,
            &replica2_addr,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn node_client1 process");

    {
        let stdin = node_client1
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for node_client1");

        // 30.0 para card 10
        writeln!(stdin, "0 100 10 30.0")
            .expect("[TEST] failed to write first charge to node_client1 stdin");
        thread::sleep(Duration::from_millis(200));

        // 50.0 para card 10 → total 80.0
        writeln!(stdin, "1 100 10 50.0")
            .expect("[TEST] failed to write second charge to node_client1 stdin");
        thread::sleep(Duration::from_millis(200));

        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to node_client1 stdin");
    }

    let nc1_output = node_client1
        .wait_with_output()
        .expect("[TEST] failed to wait for node_client1");
    if !nc1_output.status.success() {
        panic!(
            "[TEST] node_client1 exited with failure status: {:?}",
            nc1_output.status
        );
    }

    // -------------------------
    // 5) node_client 2: card 20 → 70.0 + 50.0
    // -------------------------
    let mut node_client2 = Command::new("cargo")
        .args([
            "run", "-p", "client", "--bin", "node_client",
            "--",
            &nc2_bind_addr,
            "16", // max_connections
            "4",  // num_pumps
            &leader_addr,
            &replica1_addr,
            &replica2_addr,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn node_client2 process");

    {
        let stdin = node_client2
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for node_client2");

        // 70.0 para card 20
        writeln!(stdin, "0 100 20 70.0")
            .expect("[TEST] failed to write first charge to node_client2 stdin");
        thread::sleep(Duration::from_millis(200));

        // 50.0 para card 20 → total 120.0
        writeln!(stdin, "1 100 20 50.0")
            .expect("[TEST] failed to write second charge to node_client2 stdin");
        thread::sleep(Duration::from_millis(200));

        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to node_client2 stdin");
    }

    let nc2_output = node_client2
        .wait_with_output()
        .expect("[TEST] failed to wait for node_client2");
    if !nc2_output.status.success() {
        panic!(
            "[TEST] node_client2 exited with failure status: {:?}",
            nc2_output.status
        );
    }

    // Pequeño colchón para que todos los Apply se propaguen
    thread::sleep(Duration::from_millis(500));

    // -------------------------
    // 6) Administrator: hace account-query sobre account_id=100
    // -------------------------
    let mut admin = Command::new("cargo")
        .args([
            "run", "-p", "client", "--bin", "administrator",
            "--",
            &admin_bind_addr,  // bind_addr
            &leader_addr,      // target_node_addr
            "100",             // account_id
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn administrator process");

    {
        let stdin = admin
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for administrator");

        writeln!(stdin, "account-query")
            .expect("[TEST] failed to write account-query to administrator stdin");
        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to administrator stdin");
    }

    let admin_output = admin
        .wait_with_output()
        .expect("[TEST] failed to wait for administrator");

    let stdout = String::from_utf8_lossy(&admin_output.stdout);
    println!("--- [administrator STDOUT] ---\n{stdout}");

    if !admin_output.status.success() {
        panic!(
            "[TEST] administrator exited with failure status: {:?}",
            admin_output.status
        );
    }

    // -------------------------
    // 7) Asserts sobre el AccountQuery
    // -------------------------

    // 1) Debe aparecer el marcador de éxito de AccountQuery
    assert!(
        stdout.contains("ACCOUNT_QUERY: OK"),
        "administrator stdout did not contain 'ACCOUNT_QUERY: OK'; stdout:\n{stdout}"
    );

    // 2) Debe mencionar la cuenta 100
    assert!(
        stdout.contains("account_id=100"),
        "administrator stdout did not mention account id 100; stdout:\n{stdout}"
    );

    // 3) Total 200.0 (aceptamos 200.0 o 200.00)
    let has_total_200 =
        stdout.contains("total_spent=200.0") || stdout.contains("total_spent=200.00");
    assert!(
        has_total_200,
        "administrator stdout did not contain total_spent=200.0; stdout:\n{stdout}"
    );

    // 4) card 10 → 80.0 (aceptamos 80.0 o 80.00)
    let has_card10_80 =
        stdout.contains("card_id=10 spent=80.0") || stdout.contains("card_id=10 spent=80.00");
    assert!(
        has_card10_80,
        "administrator stdout did not show card 10 with 80.0 spent; stdout:\n{stdout}"
    );

    // 5) card 20 → 120.0 (aceptamos 120.0 o 120.00)
    let has_card20_120 =
        stdout.contains("card_id=20 spent=120.0") || stdout.contains("card_id=20 spent=120.00");
    assert!(
        has_card20_120,
        "administrator stdout did not show card 20 with 120.0 spent; stdout:\n{stdout}"
    );

    // -------------------------
    // 8) Cleanup
    // -------------------------
    let _ = replica2.kill();
    let _ = replica2.wait();
    let _ = replica1.kill();
    let _ = replica1.wait();
    let _ = leader.kill();
    let _ = leader.wait();

    println!("===== [TEST] cluster_smoke_multiple_clients_then_account_query END =====");
}

/// Igual que el test anterior de múltiples clientes, pero:
/// - Después de generar consumo (total 200.0),
/// - El administrator hace:
///     * account-query  (vemos los 200.0)
///     * bill
///     * account-query  (esperamos total_spent=0 y consumos por tarjeta en 0)
#[test]
fn cluster_smoke_multiple_clients_bill_clears_account() {
    println!("===== [TEST] cluster_smoke_multiple_clients_bill_clears_account START =====");

    // Puertos para leader, replicas, admin y dos node_clients
    let leader_port = pick_free_port();
    let replica1_port = pick_free_port();
    let replica2_port = pick_free_port();
    let admin_port = pick_free_port();
    let nc1_port = pick_free_port();
    let nc2_port = pick_free_port();

    let leader_addr     = format!("127.0.0.1:{leader_port}");
    let replica1_addr   = format!("127.0.0.1:{replica1_port}");
    let replica2_addr   = format!("127.0.0.1:{replica2_port}");
    let admin_bind_addr = format!("127.0.0.1:{admin_port}");
    let nc1_bind_addr   = format!("127.0.0.1:{nc1_port}");
    let nc2_bind_addr   = format!("127.0.0.1:{nc2_port}");

    println!("[TEST] leader_addr     = {leader_addr}");
    println!("[TEST] replica1_addr   = {replica1_addr}");
    println!("[TEST] replica2_addr   = {replica2_addr}");
    println!("[TEST] admin_bind_addr = {admin_bind_addr}");
    println!("[TEST] nc1_bind_addr   = {nc1_bind_addr}");
    println!("[TEST] nc2_bind_addr   = {nc2_bind_addr}");

    // -------------------------
    // 1) Leader
    // -------------------------
    let mut leader = Command::new("cargo")
        .args([
            "run", "-p", "server", "--bin", "server",
            "--",
            "--address", &leader_addr,
            "--pumps", "4",
            "leader",
            "--max-conns", "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn leader process");

    let ready_leader = wait_for_port(&leader_addr, Duration::from_secs(10));
    if !ready_leader {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Leader did not start listening on {leader_addr} in time");
    }
    if let Ok(Some(status)) = leader.try_wait() {
        panic!("[TEST] Leader exited earlier than expected with status: {status:?}");
    }

    // -------------------------
    // 2) Réplica 1
    // -------------------------
    let mut replica1 = Command::new("cargo")
        .args([
            "run", "-p", "server", "--bin", "server",
            "--",
            "--address", &replica1_addr,
            "--pumps", "4",
            "replica",
            "--leader-addr", &leader_addr,
            "--max-conns", "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn replica1 process");

    let ready_replica1 = wait_for_port(&replica1_addr, Duration::from_secs(10));
    if !ready_replica1 {
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica1 did not start listening on {replica1_addr} in time");
    }
    if let Ok(Some(status)) = replica1.try_wait() {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica1 exited earlier than expected with status: {status:?}");
    }

    // -------------------------
    // 3) Réplica 2
    // -------------------------
    let mut replica2 = Command::new("cargo")
        .args([
            "run", "-p", "server", "--bin", "server",
            "--",
            "--address", &replica2_addr,
            "--pumps", "4",
            "replica",
            "--leader-addr", &leader_addr,
            "--max-conns", "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn replica2 process");

    let ready_replica2 = wait_for_port(&replica2_addr, Duration::from_secs(10));
    if !ready_replica2 {
        let _ = replica2.kill();
        let _ = replica2.wait();
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica2 did not start listening on {replica2_addr} in time");
    }
    if let Ok(Some(status)) = replica2.try_wait() {
        let _ = replica2.kill();
        let _ = replica2.wait();
        let _ = replica1.kill();
        let _ = replica1.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Replica2 exited earlier than expected with status: {status:?}");
    }

    // Dejar que las réplicas hagan Join y el líder actualice cluster
    thread::sleep(Duration::from_secs(2));

    // -------------------------
    // 4) node_client 1: card 10 → 30 + 50 = 80
    // -------------------------
    let mut node_client1 = Command::new("cargo")
        .args([
            "run", "-p", "client", "--bin", "node_client",
            "--",
            &nc1_bind_addr,
            "16", // max_connections
            "4",  // num_pumps
            &leader_addr,
            &replica1_addr,
            &replica2_addr,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn node_client1 process");

    {
        let stdin = node_client1
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for node_client1");

        writeln!(stdin, "0 100 10 30.0")
            .expect("[TEST] failed to write first charge to node_client1 stdin");
        thread::sleep(Duration::from_millis(200));

        writeln!(stdin, "1 100 10 50.0")
            .expect("[TEST] failed to write second charge to node_client1 stdin");
        thread::sleep(Duration::from_millis(200));

        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to node_client1 stdin");
    }

    let nc1_output = node_client1
        .wait_with_output()
        .expect("[TEST] failed to wait for node_client1");
    if !nc1_output.status.success() {
        panic!(
            "[TEST] node_client1 exited with failure status: {:?}",
            nc1_output.status
        );
    }

    // -------------------------
    // 5) node_client 2: card 20 → 70 + 50 = 120
    // -------------------------
    let mut node_client2 = Command::new("cargo")
        .args([
            "run", "-p", "client", "--bin", "node_client",
            "--",
            &nc2_bind_addr,
            "16", // max_connections
            "4",  // num_pumps
            &leader_addr,
            &replica1_addr,
            &replica2_addr,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn node_client2 process");

    {
        let stdin = node_client2
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for node_client2");

        writeln!(stdin, "0 100 20 70.0")
            .expect("[TEST] failed to write first charge to node_client2 stdin");
        thread::sleep(Duration::from_millis(200));

        writeln!(stdin, "1 100 20 50.0")
            .expect("[TEST] failed to write second charge to node_client2 stdin");
        thread::sleep(Duration::from_millis(200));

        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to node_client2 stdin");
    }

    let nc2_output = node_client2
        .wait_with_output()
        .expect("[TEST] failed to wait for node_client2");
    if !nc2_output.status.success() {
        panic!(
            "[TEST] node_client2 exited with failure status: {:?}",
            nc2_output.status
        );
    }

    // Dejar que los Apply se propaguen
    thread::sleep(Duration::from_millis(500));

    // -------------------------
    // 6) Administrator: account-query, bill, account-query
    // -------------------------
    let mut admin = Command::new("cargo")
        .args([
            "run", "-p", "client", "--bin", "administrator",
            "--",
            &admin_bind_addr,  // bind_addr
            &leader_addr,      // target_node_addr
            "100",             // account_id
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn administrator process");

    {
        let stdin = admin
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for administrator");

        writeln!(stdin, "account-query")
            .expect("[TEST] failed to write first account-query");
        writeln!(stdin, "bill")
            .expect("[TEST] failed to write bill command");
        writeln!(stdin, "account-query")
            .expect("[TEST] failed to write second account-query");
        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to administrator stdin");
    }

    let admin_output = admin
        .wait_with_output()
        .expect("[TEST] failed to wait for administrator");

    let stdout = String::from_utf8_lossy(&admin_output.stdout);
    println!("--- [administrator STDOUT] ---\n{stdout}");

    if !admin_output.status.success() {
        panic!(
            "[TEST] administrator exited with failure status: {:?}",
            admin_output.status
        );
    }

    // -------------------------
    // 7) Analizar las ACCOUNT_QUERY: OK
    //    Tenemos 3:
    //      - 1ª: account-query inicial (200)
    //      - 2ª: respuesta del bill (200)
    //      - 3ª: account-query final (0)
    // -------------------------
    let parts: Vec<&str> = stdout.split("ACCOUNT_QUERY: OK").collect();
    assert!(
        parts.len() >= 4,
        "expected at least 3 ACCOUNT_QUERY: OK results, got {} parts; stdout:\n{stdout}",
        parts.len()
    );

    let first_query_block = parts[1];
    let final_query_block = *parts.last().unwrap();

    // --- Primera query: total 200.0 y per-card 80 / 120 ---
    let has_total_200 =
        first_query_block.contains("total_spent=200.0") ||
        first_query_block.contains("total_spent=200.00");
    assert!(
        has_total_200,
        "first account-query block did not contain total_spent=200; block:\n{first_query_block}"
    );

    let has_card10_80 =
        first_query_block.contains("card_id=10 spent=80.0") ||
        first_query_block.contains("card_id=10 spent=80.00");
    assert!(
        has_card10_80,
        "first account-query block did not show card 10 with 80.0; block:\n{first_query_block}"
    );

    let has_card20_120 =
        first_query_block.contains("card_id=20 spent=120.0") ||
        first_query_block.contains("card_id=20 spent=120.00");
    assert!(
        has_card20_120,
        "first account-query block did not show card 20 with 120.0; block:\n{first_query_block}"
    );

    // --- Última query (post-bill): total 0 y consumos por tarjeta en 0 ---
    let has_total_0 =
        final_query_block.contains("total_spent=0.0") ||
        final_query_block.contains("total_spent=0.00");
    assert!(
        has_total_0,
        "final account-query block did not contain total_spent=0; block:\n{final_query_block}"
    );

    let has_card10_zero =
        final_query_block.contains("card_id=10 spent=0.0") ||
        final_query_block.contains("card_id=10 spent=0.00");
    let has_card20_zero =
        final_query_block.contains("card_id=20 spent=0.0") ||
        final_query_block.contains("card_id=20 spent=0.00");

    assert!(
        has_card10_zero && has_card20_zero,
        "final account-query block did not show per-card consumption in 0; block:\n{final_query_block}"
    );

    // -------------------------
    // 8) Cleanup
    // -------------------------
    let _ = replica2.kill();
    let _ = replica2.wait();
    let _ = replica1.kill();
    let _ = replica1.wait();
    let _ = leader.kill();
    let _ = leader.wait();

    println!("===== [TEST] cluster_smoke_multiple_clients_bill_clears_account END =====");
}


/// Concurrencia simple con 2 clientes sobre el mismo leader:
/// - Leader sin réplicas.
/// - node_client 1: 10 cargos sobre account_id=100, card_id=10, amount=5.0.
/// - node_client 2: 10 cargos sobre account_id=100, card_id=10, amount=5.0.
/// - Total esperado: 20 * 5.0 = 100.0.
/// - Las cargas se hacen en dos threads en paralelo.
/// - Luego un administrator hace un único account-query y esperamos ver total_spent=100.0.
#[test]
fn cluster_smoke_concurrent_charges_two_clients() {
    use std::thread;

    println!("===== [TEST] cluster_smoke_concurrent_charges_two_clients START =====");

    // Puertos libres para leader, node_clients y admin
    let leader_port = pick_free_port();
    let nc1_port = pick_free_port();
    let nc2_port = pick_free_port();
    let admin_port = pick_free_port();

    let leader_addr = format!("127.0.0.1:{leader_port}");
    let nc1_bind_addr = format!("127.0.0.1:{nc1_port}");
    let nc2_bind_addr = format!("127.0.0.1:{nc2_port}");
    let admin_bind_addr = format!("127.0.0.1:{admin_port}");

    println!("[TEST] leader_addr   = {leader_addr}");
    println!("[TEST] nc1_bind_addr = {nc1_bind_addr}");
    println!("[TEST] nc2_bind_addr = {nc2_bind_addr}");
    println!("[TEST] admin_bind_addr = {admin_bind_addr}");

    // -------------------------
    // 1) Leader
    // -------------------------
    let mut leader = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",
            "--bin",
            "server",
            "--",
            "--address",
            &leader_addr,
            "--pumps",
            "4",
            "leader",
            "--max-conns",
            "16",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn leader process");

    let ready_leader = wait_for_port(&leader_addr, Duration::from_secs(10));
    if !ready_leader {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Leader did not start listening on {leader_addr} in time");
    }

    if let Ok(Some(status)) = leader.try_wait() {
        panic!("[TEST] Leader exited earlier than expected with status: {status:?}");
    }

    // -------------------------
    // 2) node_client 1
    // -------------------------
    let mut nc1 = Command::new("cargo")
        .args([
            "run",
            "-p",
            "client",
            "--bin",
            "node_client",
            "--",
            &nc1_bind_addr,   // bind_addr
            "16",             // max_connections
            "4",              // num_pumps
            &leader_addr,     // known_node_1
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn node_client 1 process");

    // -------------------------
    // 3) node_client 2
    // -------------------------
    let mut nc2 = Command::new("cargo")
        .args([
            "run",
            "-p",
            "client",
            "--bin",
            "node_client",
            "--",
            &nc2_bind_addr,   // bind_addr
            "16",             // max_connections
            "4",              // num_pumps
            &leader_addr,     // known_node_1
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn node_client 2 process");

    // -------------------------
    // 4) Threads concurrentes para cada node_client
    // -------------------------

    // nc1: 10 cargos (pump 0)
    let handle1 = thread::spawn(move || {
        let mut child = nc1;

        {
            let stdin = child
                .stdin
                .as_mut()
                .expect("[TEST] failed to open stdin for node_client 1");

            for _ in 0..10 {
                writeln!(stdin, "0 100 10 5.0")
                    .expect("[TEST] failed to write charge to node_client 1 stdin");
                thread::sleep(Duration::from_millis(100));
            }

            writeln!(stdin, "exit")
                .expect("[TEST] failed to write exit to node_client 1 stdin");
        }

        let output = child
            .wait_with_output()
            .expect("[TEST] failed to wait for node_client 1");
        if !output.status.success() {
            panic!(
                "[TEST] node_client 1 exited with failure status: {:?}",
                output.status
            );
        }
    });

    // nc2: 10 cargos (pump 1)
    let handle2 = thread::spawn(move || {
        let mut child = nc2;

        {
            let stdin = child
                .stdin
                .as_mut()
                .expect("[TEST] failed to open stdin for node_client 2");

            for _ in 0..10 {
                writeln!(stdin, "1 100 10 5.0")
                    .expect("[TEST] failed to write charge to node_client 2 stdin");
                thread::sleep(Duration::from_millis(100));
            }

            writeln!(stdin, "exit")
                .expect("[TEST] failed to write exit to node_client 2 stdin");
        }

        let output = child
            .wait_with_output()
            .expect("[TEST] failed to wait for node_client 2");
        if !output.status.success() {
            panic!(
                "[TEST] node_client 2 exited with failure status: {:?}",
                output.status
            );
        }
    });

    // Esperamos a que terminen los dos clientes
    handle1.join().expect("[TEST] node_client 1 thread panicked");
    handle2.join().expect("[TEST] node_client 2 thread panicked");

    // Un poquito de aire para que el líder termine de aplicar todo
    thread::sleep(Duration::from_millis(200));

    // -------------------------
    // 5) Un solo account-query con administrator (sin helpers)
    // -------------------------
    let mut admin = Command::new("cargo")
        .args([
            "run",
            "-p",
            "client",
            "--bin",
            "administrator",
            "--",
            &admin_bind_addr,  // bind_addr
            &leader_addr,      // target_node_addr
            "100",             // account_id
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("[TEST] failed to spawn administrator process");

    {
        let stdin = admin
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for administrator");

        // Un solo account-query
        writeln!(stdin, "account-query")
            .expect("[TEST] failed to write account-query to admin stdin");
        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to admin stdin");
    }

    let output = admin
        .wait_with_output()
        .expect("[TEST] failed to wait for administrator");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "[TEST] administrator exited with failure status: {:?}\nstderr:\n{stderr}",
            output.status
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("--- [administrator STDOUT] ---\n{stdout}");

    // Buscar línea "total_spent=..."
    let mut maybe_total: Option<f32> = None;
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("total_spent=") {
            // Tomamos el primer token luego del '='
            if let Some(tok) = rest.split_whitespace().next() {
                if let Ok(val) = tok.parse::<f32>() {
                    maybe_total = Some(val);
                    break;
                }
            }
        }
    }

    let total = maybe_total.expect(
        "[TEST] admin stdout did not contain a parsable 'total_spent=' line",
    );

    let expected_total = 100.0; // 20 cargos * 5.0
    assert!(
        (total - expected_total).abs() < 0.01,
        "expected total_spent={expected_total}, got {total}"
    );

    // -------------------------
    // 6) Cleanup
    // -------------------------
    let _ = leader.kill();
    let _ = leader.wait();

    println!("===== [TEST] cluster_smoke_concurrent_charges_two_clients END =====");
}

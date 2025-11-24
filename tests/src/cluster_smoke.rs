// tests/src/cluster_smoke.rs

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Find a free TCP port on 127.0.0.1 by binding to port 0 and reading the assigned port.
fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .expect("[TEST] failed to bind to 127.0.0.1:0 to pick a free port");
    let port = listener.local_addr().unwrap().port();
    // Drop the listener so the port becomes available again
    drop(listener);
    port
}

/// Helper: wait until a TCP port is accepting connections, or timeout.
fn wait_for_port(addr: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    println!("[TEST] Waiting for {addr} to become reachable (timeout: {timeout:?})...");
    loop {
        match TcpStream::connect(addr) {
            Ok(_) => {
                println!("[TEST] {addr} is reachable.");
                return true;
            }
            Err(_) => {
                if start.elapsed() >= timeout {
                    println!("[TEST] Timeout waiting for {addr}.");
                    return false;
                }
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

/// Simple end-to-end smoke test:
/// - start a leader node
/// - wait until it's listening on 127.0.0.1:5100
/// - start an administrator client on a random free port
/// - send `account-query` + `exit`
/// - assert the admin prints something that looks like an AccountQuery result
#[test]
fn cluster_smoke_account_query() {
    println!("===== [TEST] cluster_smoke_account_query START =====");

    const LEADER_ADDR: &str = "127.0.0.1:5100";

    // Elegimos un puerto libre para el admin
    let admin_port = pick_free_port();
    let admin_bind_addr = format!("127.0.0.1:{admin_port}");
    println!("[TEST] Using admin bind addr: {admin_bind_addr}");

    // -------------------------
    // 1) Start LEADER
    // -------------------------
    println!("[TEST] STEP 1: spawning leader process...");

    let mut leader = Command::new("cargo")
        .args([
            "run",
            "-p",
            "server",          // server crate
            "--bin",
            "server",          // server bin
            "--",
            "--address",
            LEADER_ADDR,
            "--pumps",
            "4",
            "leader",
            "--max-conns",
            "16",
        ])
        .stdout(Stdio::inherit()) // see server logs
        .stderr(Stdio::inherit())
        .spawn()
        .expect("[TEST] failed to spawn leader process");

    // Wait for leader port to be listening
    let ready = wait_for_port(LEADER_ADDR, Duration::from_secs(10));
    if !ready {
        let _ = leader.kill();
        let _ = leader.wait();
        panic!("[TEST] Leader did not start listening on {LEADER_ADDR} in time");
    }

    if let Ok(Some(status)) = leader.try_wait() {
        panic!("[TEST] Leader exited earlier than expected with status: {status:?}");
    } else {
        println!("[TEST] Leader is running and port {LEADER_ADDR} is reachable.");
    }

    // -------------------------
    // 2) Start ADMINISTRATOR
    // -------------------------
    println!("[TEST] STEP 2: spawning administrator process...");

    let mut admin = Command::new("cargo")
        .args([
            "run",
            "-p",
            "client",               // client crate
            "--bin",
            "administrator",        // admin bin
            "--",
            &admin_bind_addr,       // admin bind_addr (fixed, free port)
            LEADER_ADDR,            // leader addr
            "100",                  // account_id
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("[TEST] failed to spawn administrator process");

    println!("[TEST] Administrator spawned.");

    // -------------------------
    // 3) Send commands via stdin
    // -------------------------
    println!("[TEST] STEP 3: sending commands to administrator stdin...");

    {
        let stdin = admin
            .stdin
            .as_mut()
            .expect("[TEST] failed to open stdin for administrator");

        println!("[TEST] -> writing 'account-query'");
        writeln!(stdin, "account-query")
            .expect("[TEST] failed to write account-query to admin stdin");

        println!("[TEST] -> writing 'exit'");
        writeln!(stdin, "exit")
            .expect("[TEST] failed to write exit to admin stdin");
    }

    println!("[TEST] Commands written. Waiting for administrator to exit...");

    // -------------------------
    // 4) Wait for admin and capture stdout
    // -------------------------
    let output = admin
        .wait_with_output()
        .expect("[TEST] failed to wait for administrator");

    println!("[TEST] Administrator exited. Collecting stdout/stderr...");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    println!("--- [administrator STDOUT] ---\n{stdout}");
    println!("--- [administrator STDERR] ---\n{stderr}");

    if !output.status.success() {
        panic!(
            "[TEST] Administrator exited with failure status: {:?}\nSTDERR:\n{stderr}",
            output.status
        );
    }

    // -------------------------
    // 5) Assert output looks like an AccountQuery
    // -------------------------
    println!("[TEST] STEP 5: asserting stdout contains some AccountQuery markers...");

    assert!(
        stdout.contains("ACCOUNT QUERY")
            || stdout.contains("Account ID")
            || stdout.contains("ACCOUNT QUERY RESULT")
            || stdout.contains("Account query result")
            || stdout.contains("Account:")
            || stdout.contains("Account ID:"),
        "administrator stdout did not contain an expected marker; stdout:\n{stdout}"
    );

    // -------------------------
    // 6) Cleanup: kill leader
    // -------------------------
    println!("[TEST] Cleaning up: killing leader...");

    let _ = leader.kill();
    let _ = leader.wait();

    println!("===== [TEST] cluster_smoke_account_query END =====");
}

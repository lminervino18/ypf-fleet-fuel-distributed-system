use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{tcp::OwnedReadHalf, tcp::OwnedWriteHalf, TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

use crate::errors::{AppError, AppResult};

/// ===== Tunables =====
const IDLE_READ_TIMEOUT_SECS: u64 = 120; // idle timeout for read loops

/// Commands the outside world can send to the ConnectionManager
#[derive(Debug)]
pub enum ManagerCmd {
    /// Send a text payload to a remote address (open or reuse a TCP connection)
    SendTo(SocketAddr, String),
    /// Ask the manager to shut down (optional)
    #[allow(dead_code)]
    Shutdown,
}

/// Events the ConnectionManager emits *to the outside* (your Node)
#[derive(Debug)]
pub enum InboundEvent {
    /// A line of text was read from a peer
    Received { peer: SocketAddr, payload: String },
    /// A connection was closed (peer or local)
    ConnClosed { peer: SocketAddr },
}

/// Internal record for an active connection
#[derive(Debug)]
struct ConnInfo {
    addr: SocketAddr,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    last_used: Instant,
    reader_task: JoinHandle<()>,
}

/// TCP service that owns the listener, active connections, and handles I/O.
pub struct ConnectionManager {
    listen_addr: SocketAddr,
    max_conns: usize,

    // active connections indexed by an internal ID
    active: HashMap<u64, ConnInfo>,

    // outbound commands (Node -> Manager)
    cmd_rx: mpsc::Receiver<ManagerCmd>,

    // inbound events (Manager -> Node)
    inbound_tx: mpsc::Sender<InboundEvent>,
}

impl ConnectionManager {
    /// Create channels and spawn the ConnectionManager task (listener included).
    ///
    /// Returns:
    /// - `ManagerCmd` sender for issuing send/shutdown commands
    /// - `InboundEvent` receiver for consuming inbound messages and closures
    pub fn start(
        listen_addr: SocketAddr,
        max_conns: usize,
    ) -> (mpsc::Sender<ManagerCmd>, mpsc::Receiver<InboundEvent>) {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ManagerCmd>(128);
        let (inbound_tx, inbound_rx) = mpsc::channel::<InboundEvent>(256);

        let manager = Self {
            listen_addr,
            max_conns,
            active: HashMap::new(),
            cmd_rx,
            inbound_tx,
        };

        tokio::spawn(manager.run());
        (cmd_tx, inbound_rx)
    }

    /// Main loop: binds the listener and multiplexes accept + command handling.
    async fn run(mut self) {
        let listener = match TcpListener::bind(self.listen_addr).await {
            Ok(l) => {
                println!(
                    "[INFO] ConnectionManager listening on {} (max_conns={})",
                    self.listen_addr, self.max_conns
                );
                l
            }
            Err(e) => {
                eprintln!(
                    "[FATAL] Failed to bind TCP listener on {}: {}",
                    self.listen_addr, e
                );
                return;
            }
        };

        loop {
            tokio::select! {
                // Accept new inbound connections
                accept_res = listener.accept() => {
                    match accept_res {
                        Ok((stream, peer)) => {
                            if let Err(e) = self.handle_new_inbound(stream, peer).await {
                                eprintln!("[ERROR] Failed to handle new connection {}: {e}", peer);
                            }
                        }
                        Err(e) => {
                            eprintln!("[ERROR] accept() failed: {}", e);
                        }
                    }
                }

                // Handle external commands (send/shutdown)
                cmd_opt = self.cmd_rx.recv() => {
                    match cmd_opt {
                        Some(ManagerCmd::SendTo(addr, msg)) => {
                            if let Err(e) = self.handle_send_to(addr, &msg).await {
                                eprintln!("[WARN] SendTo {} failed: {}", addr, e);
                            }
                        }
                        Some(ManagerCmd::Shutdown) => {
                            println!("[INFO] ConnectionManager: Shutdown requested");
                            break;
                        }
                        None => {
                            println!("[INFO] ConnectionManager: command channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // Graceful-ish shutdown: abort reader tasks and drop writers
        for (_, info) in self.active.drain() {
            info.reader_task.abort();
            let _ = self
                .inbound_tx
                .try_send(InboundEvent::ConnClosed { peer: info.addr });
        }

        println!("[INFO] ConnectionManager stopped");
    }

    /// Accept path: track the new inbound connection, spawn its reader, apply LRU if needed.
    async fn handle_new_inbound(&mut self, stream: TcpStream, peer: SocketAddr) -> AppResult<()> {
        self.evict_lru_if_full();

        println!("[INFO] New inbound connection from {}", peer);
        let (reader, writer) = stream.into_split();
        let writer_arc = Arc::new(Mutex::new(writer));
        let inbound_tx = self.inbound_tx.clone();

        let id = rand::random::<u64>();

        let reader_task = tokio::spawn(async move {
            if let Err(e) = handle_reader(id, peer, reader, inbound_tx.clone()).await {
                eprintln!("[ERROR] Reader #{} ({}): {}", id, peer, e);
            }
            let _ = inbound_tx
                .send(InboundEvent::ConnClosed { peer })
                .await;
        });

        self.active.insert(
            id,
            ConnInfo {
                addr: peer,
                writer: writer_arc,
                last_used: Instant::now(),
                reader_task,
            },
        );

        Ok(())
    }

    /// Outbound path: reuse an existing connection or open a new one.
    async fn handle_send_to(&mut self, addr: SocketAddr, msg: &str) -> AppResult<()> {
        // Try to reuse existing connection
        if let Some((id, info)) = self.active.iter_mut().find(|(_, c)| c.addr == addr) {
            info.last_used = Instant::now();
            let mut writer = info.writer.lock().await;
            writer
                .write_all(msg.as_bytes())
                .await
                .map_err(|e| AppError::ConnectionIO {
                    addr: addr.to_string(),
                    source: e,
                })?;
            writer.flush().await.map_err(|e| AppError::ConnectionIO {
                addr: addr.to_string(),
                source: e,
            })?;
            println!("[SEND] Reused connection #{} to {}", id, addr);
            return Ok(());
        }

        // Otherwise, open a new connection
        let stream = TcpStream::connect(addr)
            .await
            .map_err(|e| AppError::ConnectionRefused {
                addr: addr.to_string(),
            })?;

        let (reader, writer) = stream.into_split();
        let writer_arc = Arc::new(Mutex::new(writer));

        self.evict_lru_if_full();

        let inbound_tx = self.inbound_tx.clone();
        let id = rand::random::<u64>();

        let reader_task = tokio::spawn(async move {
            if let Err(e) = handle_reader(id, addr, reader, inbound_tx.clone()).await {
                eprintln!("[ERROR] Reader #{} ({}): {}", id, addr, e);
            }
            let _ = inbound_tx
                .send(InboundEvent::ConnClosed { peer: addr })
                .await;
        });

        self.active.insert(
            id,
            ConnInfo {
                addr,
                writer: writer_arc,
                last_used: Instant::now(),
                reader_task,
            },
        );

        // Send the initial payload
        if let Some(info) = self.active.get(&id) {
            let mut writer = info.writer.lock().await;
            writer
                .write_all(msg.as_bytes())
                .await
                .map_err(|e| AppError::ConnectionIO {
                    addr: addr.to_string(),
                    source: e,
                })?;
            writer.flush().await.map_err(|e| AppError::ConnectionIO {
                addr: addr.to_string(),
                source: e,
            })?;
        }

        println!("[SEND] Created new connection #{} to {}", id, addr);
        Ok(())
    }

    /// LRU eviction when connection limit is reached.
    fn evict_lru_if_full(&mut self) {
        if self.active.len() >= self.max_conns {
            if let Some((&old_id, _)) = self.active.iter().min_by_key(|(_, c)| c.last_used) {
                if let Some(info) = self.active.remove(&old_id) {
                    println!(
                        "[WARN] Connection limit ({}) reached. Dropping LRU: {}",
                        self.max_conns, info.addr
                    );
                    info.reader_task.abort();
                    let _ = self
                        .inbound_tx
                        .try_send(InboundEvent::ConnClosed { peer: info.addr });
                }
            }
        }
    }
}

/// Per-connection reader: reads newline-delimited frames and forwards them upstream.
/// Includes an idle timeout to close silent sockets.
async fn handle_reader(
    id: u64,
    peer: SocketAddr,
    reader: OwnedReadHalf,
    inbound_tx: mpsc::Sender<InboundEvent>,
) -> AppResult<()> {
    let mut lines = BufReader::new(reader).lines();
    let idle = Duration::from_secs(IDLE_READ_TIMEOUT_SECS);

    loop {
        match timeout(idle, lines.next_line()).await {
            Ok(res) => match res {
                Ok(Some(line)) => {
                    let _ = inbound_tx
                        .send(InboundEvent::Received {
                            peer,
                            payload: line,
                        })
                        .await;
                }
                Ok(None) => break, // EOF
                Err(e) => {
                    return Err(AppError::ConnectionIO {
                        addr: peer.to_string(),
                        source: e,
                    });
                }
            },
            Err(_) => {
                eprintln!(
                    "[CONN:{}][{}] idle read timeout ({}s) -> closing",
                    id, peer, IDLE_READ_TIMEOUT_SECS
                );
                return Err(AppError::ConnectionTimeout {
                    addr: peer.to_string(),
                });
            }
        }
    }

    Ok(())
}

// =========================
// ========== TESTS =========
// =========================
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};
    use std::time::Duration as StdDuration;

    /// Pick a free ephemeral port by binding to 0 and returning it.
    fn pick_free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind 0")
            .local_addr()
            .unwrap()
            .port()
    }

    async fn recv_with_timeout<T: std::fmt::Debug>(
        rx: &mut mpsc::Receiver<T>,
        dur: Duration,
    ) -> Option<T> {
        tokio::time::timeout(dur, rx.recv()).await.ok().flatten()
    }

    /// Repeatedly attempt to connect to `addr` until it succeeds or we time out.
    /// This eliminates races where the manager hasn't finished binding yet.
    async fn connect_with_retry(addr: SocketAddr, attempts: usize, delay: Duration) -> TcpStream {
        let mut last_err: Option<std::io::Error> = None;
        for _ in 0..attempts {
            match TcpStream::connect(addr).await {
                Ok(s) => return s,
                Err(e) => {
                    last_err = Some(e);
                    sleep(delay).await;
                }
            }
        }
        panic!("connect_with_retry failed: {:?}", last_err);
    }

    /// Start a manager on a free port, returning (cmd_tx, inbound_rx, addr).
    fn start_manager_on_free_port(max_conns: usize) -> (mpsc::Sender<ManagerCmd>, mpsc::Receiver<InboundEvent>, SocketAddr) {
        let port = pick_free_port();
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
        let (cmd_tx, inbound_rx) = ConnectionManager::start(addr, max_conns);
        (cmd_tx, inbound_rx, addr)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn start_and_accepts_inbound_payload() {
        let (_cmd_tx, mut inbound_rx, addr) = start_manager_on_free_port(16);

        // Wait until the listener is really accepting and reuse the stream as client.
        let mut client = connect_with_retry(addr, 100, Duration::from_millis(10)).await;

        client
            .write_all(b"hello-one\n")
            .await
            .expect("write line");
        client.flush().await.expect("flush");

        // The manager should emit InboundEvent::Received
        let evt = recv_with_timeout(&mut inbound_rx, Duration::from_secs(2))
            .await
            .expect("expected inbound event");
        match evt {
            InboundEvent::Received { peer: _, payload } => {
                assert_eq!(payload, "hello-one");
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    /// A sends to B using SendTo: B must receive the payload (twice).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sendto_opens_and_delivers() {
        // Receiver B
        let (_b_cmd, mut b_inbound, b_addr) = start_manager_on_free_port(16);
        // Ensure B is ready to accept before A sends
        let _probe = connect_with_retry(b_addr, 100, Duration::from_millis(10)).await;
        // Drop probe so B's reader closes; not needed further

        // Sender A
        let (a_cmd, _a_inbound, _a_addr) = start_manager_on_free_port(16);

        a_cmd
            .send(ManagerCmd::SendTo(b_addr, "hi-1\n".to_string()))
            .await
            .expect("send cmd");
        a_cmd
            .send(ManagerCmd::SendTo(b_addr, "hi-2\n".to_string()))
            .await
            .expect("send cmd");

        let ev1 = recv_with_timeout(&mut b_inbound, Duration::from_secs(2))
            .await
            .expect("expected event 1");
        let ev2 = recv_with_timeout(&mut b_inbound, Duration::from_secs(2))
            .await
            .expect("expected event 2");

        let (p1, a1) = match ev1 {
            InboundEvent::Received { peer, payload } => (payload, peer),
            _ => panic!("unexpected event 1"),
        };
        let (p2, a2) = match ev2 {
            InboundEvent::Received { peer, payload } => (payload, peer),
            _ => panic!("unexpected event 2"),
        };

        assert_eq!(p1, "hi-1");
        assert_eq!(p2, "hi-2");
        assert_eq!(a1, a2, "same peer indicates connection reuse");
    }

    /// Verify mapping to AppError::ConnectionRefused by calling handle_send_to directly.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_send_to_connection_refused_maps_error() {
        // Build a manager struct without running the loop
        let (_cmd_tx_dummy, cmd_rx) = mpsc::channel::<ManagerCmd>(1);
        let (inbound_tx, _inbound_rx) = mpsc::channel::<InboundEvent>(1);

        let port = pick_free_port(); // nobody will listen here
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

        let mut mgr = ConnectionManager {
            listen_addr: addr, // not used here
            max_conns: 16,
            active: HashMap::new(),
            cmd_rx,
            inbound_tx,
        };

        let err = mgr.handle_send_to(addr, "ping\n").await.unwrap_err();
        match err {
            AppError::ConnectionRefused { addr: s } => {
                assert_eq!(s, addr.to_string());
            }
            other => panic!("expected ConnectionRefused, got {other:?}"),
        }
    }

    /// Verify handle_reader emits Received and exits cleanly on EOF.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_reader_emits_and_eof_ok() {
        let port = pick_free_port();
        let listen: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let listener = TcpListener::bind(listen).await.expect("bind test listener");

        // Client connects and sends a single line; then drop -> EOF for server
        let client_task = tokio::spawn(async move {
            let mut c = connect_with_retry(listen, 100, Duration::from_millis(10)).await;
            c.write_all(b"line-1\n").await.expect("write");
            c.flush().await.expect("flush");
        });

        let (server, peer) = listener.accept().await.expect("accept");
        let (reader, _writer) = server.into_split();

        let (tx, mut rx) = mpsc::channel::<InboundEvent>(4);

        // Run reader
        let res = handle_reader(123, peer, reader, tx.clone()).await;
        assert!(res.is_ok(), "reader should end ok on EOF");

        // We must have received a Received
        let ev = recv_with_timeout(&mut rx, Duration::from_secs(1))
            .await
            .expect("expected Received");
        match ev {
            InboundEvent::Received { peer: p, payload } => {
                assert_eq!(p, peer);
                assert_eq!(payload, "line-1");
            }
            other => panic!("unexpected event: {:?}", other),
        }

        let _ = client_task.await;
    }

    /// With max_conns=1, the second inbound connection should evict the first (LRU),
    /// and we should get a ConnClosed for the first peer.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lru_eviction_on_inbound() {
        let (_cmd_tx, mut inbound_rx, addr) = start_manager_on_free_port(1);

        // Ensure listener is accepting
        let _probe = connect_with_retry(addr, 100, Duration::from_millis(10)).await;

        // First client connects and stays open
        let c1 = connect_with_retry(addr, 100, Duration::from_millis(10)).await;
        let c1_local = c1.local_addr().unwrap();

        // Give the manager a moment to insert the first connection
        sleep(Duration::from_millis(50)).await;

        // Second client connects; should trigger LRU eviction of the first
        let _c2 = connect_with_retry(addr, 100, Duration::from_millis(10)).await;

        // Expect a ConnClosed for the first client
        let mut got_closed = false;
        let start = std::time::Instant::now();
        while start.elapsed() < StdDuration::from_secs(2) {
            if let Some(ev) = recv_with_timeout(&mut inbound_rx, Duration::from_millis(100)).await {
                if let InboundEvent::ConnClosed { peer } = ev {
                    if peer == c1_local {
                        got_closed = true;
                        break;
                    }
                }
            }
        }
        assert!(got_closed, "expected ConnClosed for first client due to LRU eviction");
    }

    /// Shutdown should stop the loop and close the inbound channel.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_closes_inbound_channel() {
        let (cmd_tx, mut inbound_rx, _addr) = start_manager_on_free_port(16);

        cmd_tx.send(ManagerCmd::Shutdown).await.expect("send shutdown");

        // When manager exits, it drops the inbound sender; rx should end (None).
        let closed = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if inbound_rx.recv().await.is_none() {
                    break true;
                }
            }
        })
        .await
        .expect("timeout waiting for channel close");

        assert!(closed, "inbound channel should be closed after shutdown");
    }

    /// Connection reuse: two sends from A to B should arrive from the same peer on B.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sendto_reuses_connection_same_peer() {
        // Receiver B
        let (_b_cmd, mut b_inbound, b_addr) = start_manager_on_free_port(16);
        // Ensure B is accepting
        let _probe = connect_with_retry(b_addr, 100, Duration::from_millis(10)).await;

        // Sender A
        let (a_cmd, _a_inbound, _a_addr) = start_manager_on_free_port(16);

        a_cmd
            .send(ManagerCmd::SendTo(b_addr, "one\n".to_string()))
            .await
            .expect("send1");
        a_cmd
            .send(ManagerCmd::SendTo(b_addr, "two\n".to_string()))
            .await
            .expect("send2");

        let ev1 = recv_with_timeout(&mut b_inbound, Duration::from_secs(2))
            .await
            .expect("ev1");
        let ev2 = recv_with_timeout(&mut b_inbound, Duration::from_secs(2))
            .await
            .expect("ev2");

        let (p1, peer1) = match ev1 {
            InboundEvent::Received { peer, payload } => (payload, peer),
            _ => panic!("unexpected ev1"),
        };
        let (p2, peer2) = match ev2 {
            InboundEvent::Received { peer, payload } => (payload, peer),
            _ => panic!("unexpected ev2"),
        };

        assert_eq!(p1, "one");
        assert_eq!(p2, "two");
        assert_eq!(peer1, peer2, "same TCP peer implies connection reuse");
    }
}

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{tcp::OwnedReadHalf, tcp::OwnedWriteHalf, TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

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
    reader_task: JoinHandle<()>, // the reader task that pumps this connection
}

/// The TCP service that owns the listener, active connections, and handles IO.
pub struct ConnectionManager {
    listen_addr: SocketAddr,
    max_conns: usize,

    // active connections indexed by a synthetic id
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
        // channel for commands coming from the Node
        let (cmd_tx, cmd_rx) = mpsc::channel::<ManagerCmd>(128);
        // channel for events going up to the Node
        let (inbound_tx, inbound_rx) = mpsc::channel::<InboundEvent>(256);

        // build the manager
        let manager = Self {
            listen_addr,
            max_conns,
            active: HashMap::new(),
            cmd_rx,
            inbound_tx,
        };

        // spawn the manager task
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
                            self.handle_new_inbound(stream, peer).await;
                        }
                        Err(e) => {
                            eprintln!("[ERROR] accept() failed: {}", e);
                            // keep running; transient accept errors can happen
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
                            // All senders dropped — nothing more to do
                            println!("[INFO] ConnectionManager: command channel closed");
                            break;
                        }
                    }
                }
            }
        }

        // graceful-ish shutdown: abort reader tasks and drop writers
        for (_, info) in self.active.drain() {
            info.reader_task.abort();
            let _ = self
                .inbound_tx
                .try_send(InboundEvent::ConnClosed { peer: info.addr });
        }

        println!("[INFO] ConnectionManager stopped");
    }

    /// Accept path: track the new inbound connection, spawn its reader, apply LRU if needed.
    async fn handle_new_inbound(&mut self, stream: TcpStream, peer: SocketAddr) {
        self.evict_lru_if_full();

        println!("[INFO] New inbound connection from {}", peer);
        let (reader, writer) = stream.into_split();
        let writer_arc = Arc::new(Mutex::new(writer));
        let inbound_tx = self.inbound_tx.clone();

        // synthetic id for tracking
        let id = rand::random::<u64>();

        // spawn the reader
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
    }

    /// Outbound path: reuse an existing connection by addr, or open a new one.
    async fn handle_send_to(&mut self, addr: SocketAddr, msg: &str) -> anyhow::Result<()> {
        // Try to reuse an existing connection to this addr
        if let Some((id, info)) = self.active.iter_mut().find(|(_, c)| c.addr == addr) {
            info.last_used = Instant::now();
            let mut writer = info.writer.lock().await;
            writer.write_all(msg.as_bytes()).await?;
            writer.flush().await?;
            println!("[SEND] Reused connection #{} to {}", id, addr);
            return Ok(());
        }

        // Otherwise open a fresh connection
        let stream = TcpStream::connect(addr).await?;
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
            writer.write_all(msg.as_bytes()).await?;
            writer.flush().await?;
        }

        println!("[SEND] Created new connection #{} to {}", id, addr);
        Ok(())
    }

    /// LRU eviction when the active set hits the configured limit.
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
) -> anyhow::Result<()> {
    let mut lines = BufReader::new(reader).lines();
    let idle = Duration::from_secs(IDLE_READ_TIMEOUT_SECS);

    loop {
        match timeout(idle, lines.next_line()).await {
            Ok(res) => match res? {
                Some(line) => {
                    // Push the payload to the Node for business handling
                    let _ = inbound_tx
                        .send(InboundEvent::Received {
                            peer,
                            payload: line,
                        })
                        .await;
                }
                None => {
                    // EOF from peer
                    break;
                }
            },
            Err(_) => {
                eprintln!(
                    "[CONN:{}][{}] idle read timeout ({}s) -> closing",
                    id, peer, IDLE_READ_TIMEOUT_SECS
                );
                break;
            }
        }
    }

    Ok(())
}

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{tcp::OwnedReadHalf, tcp::OwnedWriteHalf, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

/// ===== Config =====
const IDLE_READ_TIMEOUT_SECS: u64 = 120; // timeout de lectura para cortar sockets "colgados"

/// Events coming from the Node (e.g., new accepted TCP connections)
#[derive(Debug)]
pub enum ConnEvent {
    /// Incoming connection accepted by the Node (id, stream, peer_addr)
    NewConn(u64, TcpStream, SocketAddr),

    /// Connection closed (id)
    ConnClosed(u64),
}

/// Commands sent to the ConnectionManager from outside (e.g., Node logic)
#[derive(Debug)]
pub enum ManagerCmd {
    /// Send a message to a remote address (open connection if needed)
    SendTo(SocketAddr, String),
    /// (Reserved) Graceful shutdown if you ever need it
    #[allow(dead_code)]
    Shutdown,
}

/// Represents one active TCP connection
#[derive(Debug)]
pub struct ConnInfo {
    pub addr: SocketAddr,
    pub writer: Arc<Mutex<OwnedWriteHalf>>,
    pub last_used: Instant,
    pub handle: JoinHandle<()>, // task lectora asociada a esta conexión
}

/// Manages all active TCP connections (both incoming and outgoing)
#[derive(Debug)]
pub struct ConnectionManager {
    pub max_conns: usize,
    pub active: HashMap<u64, ConnInfo>,

    /// Events from the Node (new incoming connections, closures)
    pub rx: mpsc::Receiver<ConnEvent>,
    pub tx: mpsc::Sender<ConnEvent>,

    /// Commands from Node logic (send messages, etc.)
    pub cmd_rx: mpsc::Receiver<ManagerCmd>,
    pub cmd_tx: mpsc::Sender<ManagerCmd>,
}

impl ConnectionManager {
    pub fn new(
        max_conns: usize,
        rx: mpsc::Receiver<ConnEvent>,
        tx: mpsc::Sender<ConnEvent>,
        cmd_rx: mpsc::Receiver<ManagerCmd>,
        cmd_tx: mpsc::Sender<ManagerCmd>,
    ) -> Self {
        Self {
            max_conns,
            active: HashMap::new(),
            rx,
            tx,
            cmd_rx,
            cmd_tx,
        }
    }

    /// Main event loop: multiplex ConnEvent (from Node) and ManagerCmd (from Node logic)
    pub async fn run(mut self) {
        println!("[INFO] ConnectionManager running (max: {})", self.max_conns);

        loop {
            tokio::select! {
                // Events from Node (e.g., accepted connections)
                maybe_evt = self.rx.recv() => {
                    match maybe_evt {
                        Some(ConnEvent::NewConn(id, stream, addr)) => {
                            self.handle_new_conn(id, stream, addr).await;
                        }
                        Some(ConnEvent::ConnClosed(id)) => {
                            // Si llegó un cierre desde el lector, removemos y abortamos por si sigue viva
                            if let Some(info) = self.active.remove(&id) {
                                info.handle.abort();
                            }
                            println!("[INFO] Connection #{} closed ({} remaining)", id, self.active.len());
                        }
                        None => {
                            eprintln!("[WARN] ConnEvent channel closed; manager may continue handling commands");
                        }
                    }
                }

                // Commands from Node logic (e.g., send a message)
                maybe_cmd = self.cmd_rx.recv() => {
                    match maybe_cmd {
                        Some(ManagerCmd::SendTo(addr, msg)) => {
                            if let Err(e) = self.handle_send_to(addr, &msg).await {
                                eprintln!("[WARN] SendTo {} failed: {}", addr, e);
                            }
                        }
                        Some(ManagerCmd::Shutdown) => {
                            println!("[INFO] ConnectionManager received Shutdown");
                            break;
                        }
                        None => {
                            eprintln!("[WARN] ManagerCmd channel closed; exiting manager loop");
                            break;
                        }
                    }
                }
            }
        }

        // Abortamos cualquier lector que haya quedado vivo
        for (_, info) in self.active.drain() {
            info.handle.abort();
        }

        println!("[INFO] ConnectionManager stopped");
    }

    async fn handle_new_conn(&mut self, id: u64, stream: TcpStream, addr: SocketAddr) {
        // Drop LRU if full (y abortá su task lectora)
        self.evict_lru_if_full();

        println!("[INFO] New connection #{} from {}", id, addr);

        // Split into read/write halves
        let (reader, writer) = stream.into_split();
        let writer_arc = Arc::new(Mutex::new(writer));

        let tx_clone = self.tx.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = handle_connection(id, reader, tx_clone.clone()).await {
                eprintln!("[ERROR] Connection #{} ended: {}", id, e);
            }
            let _ = tx_clone.send(ConnEvent::ConnClosed(id)).await;
        });

        self.active.insert(
            id,
            ConnInfo {
                addr,
                writer: writer_arc,
                last_used: Instant::now(),
                handle,
            },
        );
    }

    /// Core send logic used by ManagerCmd::SendTo
    async fn handle_send_to(&mut self, addr: SocketAddr, msg: &str) -> anyhow::Result<()> {
        // Reuse if any existing connection targets `addr`
        if let Some((id, info)) = self.active.iter_mut().find(|(_, i)| i.addr == addr) {
            info.last_used = Instant::now();
            let mut writer = info.writer.lock().await;
            writer.write_all(msg.as_bytes()).await?;
            writer.flush().await?;
            println!("[SEND] Reused connection #{} to {}", id, addr);
            return Ok(());
        }

        // Otherwise connect (posible nueva conexión saliente)
        let stream = TcpStream::connect(addr).await?;
        let id = rand::random::<u64>();

        // Drop LRU if full (y abortá su task lectora)
        self.evict_lru_if_full();

        let (reader, writer) = stream.into_split();
        let writer_arc = Arc::new(Mutex::new(writer));

        let tx_clone = self.tx.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = handle_connection(id, reader, tx_clone.clone()).await {
                eprintln!("[ERROR] Connection #{} ended: {}", id, e);
            }
            let _ = tx_clone.send(ConnEvent::ConnClosed(id)).await;
        });

        self.active.insert(
            id,
            ConnInfo {
                addr,
                writer: writer_arc,
                last_used: Instant::now(),
                handle,
            },
        );

        // Send initial payload
        if let Some(info) = self.active.get(&id) {
            let mut writer = info.writer.lock().await;
            writer.write_all(msg.as_bytes()).await?;
            writer.flush().await?;
        }

        println!("[SEND] Created new connection #{} to {}", id, addr);
        Ok(())
    }

    /// Expulsa la conexión LRU si superamos `max_conns`, y aborta su task lectora.
    fn evict_lru_if_full(&mut self) {
        if self.active.len() >= self.max_conns {
            if let Some((&old_id, _)) = self.active.iter().min_by_key(|(_, i)| i.last_used) {
                if let Some(info) = self.active.remove(&old_id) {
                    println!(
                        "[WARN] Connection limit reached ({}). Dropping oldest: {}",
                        self.max_conns, info.addr
                    );
                    info.handle.abort(); // cancelar lector asociado
                }
            }
        }
    }
}

/// Async reader for each connection.
/// It listens for incoming data and logs messages.
/// Posee timeout de inactividad para cortar conexiones "silenciosas".
async fn handle_connection(
    id: u64,
    reader: OwnedReadHalf,
    tx: mpsc::Sender<ConnEvent>,
) -> anyhow::Result<()> {
    let mut lines = BufReader::new(reader).lines();
    let idle = Duration::from_secs(IDLE_READ_TIMEOUT_SECS);

    loop {
        match timeout(idle, lines.next_line()).await {
            Ok(res) => match res? {
                Some(line) => {
                    println!("[CONN:{}] {}", id, line);
                }
                None => {
                    // EOF: peer cerró escritura
                    break;
                }
            },
            Err(_) => {
                // timeout de inactividad
                eprintln!("[CONN:{}] idle read timeout ({}s) -> closing", id, IDLE_READ_TIMEOUT_SECS);
                break;
            }
        }
    }

    let _ = tx.send(ConnEvent::ConnClosed(id)).await;
    Ok(())
}

//! TCP acceptor that creates Handler instances for incoming connections.
//!
//! The Acceptor binds a listening socket and, for each accepted inbound
//! connection, initializes a `Handler` and registers it in the shared
//! `active` map. Connection initialization performs a small handshake
//! (handled by `Handler::start_from`). The acceptor enforces a maximum
//! number of active handlers via `add_handler_from`.
//!
//! This component runs in a background task and is designed to be resilient:
//! failing to initialize a particular handler does not stop the accept loop.

use super::Message;
use super::active_helpers::add_handler_from;
use super::handler::Handler;
use crate::errors::{AppError, AppResult};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::sync::mpsc::Sender;
use tokio::task::{self, JoinHandle};

// const INCOMING_BUFF_SIZE: usize = 300;

/// Accept incoming TCP connections and attach `Handler` instances.
///
/// The acceptor stores active handlers in a shared, locked `HashMap` so other
/// components can inspect or manage them. `max_conns` bounds the number of
/// handlers kept active; eviction is delegated to `add_handler_from`.
pub struct Acceptor {
    listener: TcpListener,
    active: Arc<Mutex<HashMap<SocketAddr, Handler>>>,
    max_conns: usize,
}

impl Acceptor {
    /// Start the acceptor loop in a background task.
    ///
    /// - `address`: local socket address to bind and listen on.
    /// - `active`: shared map where new handlers will be registered.
    /// - `messages_tx`: channel used by handlers to forward inbound `Message`s.
    /// - `max_conns`: maximum number of connections to keep in `active`.
    ///
    /// Returns a `JoinHandle` for the spawned background task.
    pub async fn start(
        address: SocketAddr,
        active: Arc<Mutex<HashMap<SocketAddr, Handler>>>,
        messages_tx: Arc<Sender<AppResult<Message>>>,
        max_conns: usize,
    ) -> AppResult<JoinHandle<()>> {
        let mut acceptor = Acceptor::new(address, active, max_conns).await?;
        let handle = task::spawn(async move {
            acceptor.run(messages_tx).await;
        });

        Ok(handle)
    }

    async fn new(
        address: SocketAddr,
        active: Arc<Mutex<HashMap<SocketAddr, Handler>>>,
        max_conns: usize,
    ) -> AppResult<Self> {
        Ok(Self {
            listener: TcpListener::bind(address).await.map_err(|_| {
                AppError::ConnectionRefused {
                    address: address.to_string(),
                }
            })?,
            active,
            max_conns,
        })
    }

    /// Accept loop that creates handlers for incoming streams.
    ///
    /// For each accepted stream:
    /// - initialize the handler via `Handler::start_from`,
    /// - on success insert the handler into `active` using `add_handler_from`,
    /// - on failure skip and continue accepting new connections.
    async fn run(&mut self, messages_tx: Arc<Sender<AppResult<Message>>>) {
        while let Ok((stream, _)) = self.listener.accept().await {
            let mut guard = self.active.lock().await;
            let Ok(handler) = Handler::start_from(stream, messages_tx.clone()).await else {
                continue;
            };

            add_handler_from(&mut guard, handler, self.max_conns);
        }
    }
}

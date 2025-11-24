// server/src/node/database.rs

use actix::{Actor, Addr};
use tokio::sync::{mpsc, oneshot};
// IMPORTANT: use std::thread::JoinHandle here, because we are using std::thread::spawn.
use std::thread::JoinHandle;

use super::actors::{ActorEvent, ActorRouter, RouterCmd};
use crate::errors::{AppError, AppResult};
use common::operation::Operation;

/// Commands that the node can send to the actor-based "database".
///
/// For now we only support Execute, but this enum can grow with more
/// high-level operations if needed.
#[derive(Debug)]
pub enum DatabaseCmd {
    Execute { op_id: u32, operation: Operation },
}

/// Handle to the actor-based "database" subsystem.
///
/// Responsibilities:
/// - Hide Actix and the concrete ActorRouter implementation.
/// - Provide a simple `send(cmd)` API for the node.
/// - Provide an async `recv()` API for the node to receive ActorEvent
///   responses (symmetric to Station::recv()).
/// - Keep the Actix System (thread) alive while this struct is in scope.
/// - Trigger a clean shutdown of the Actix System when dropped.
///
/// NOTE: This struct is now **bidirectional** from the node's POV:
///   - `send()`  → Node  -> Database (ActorRouter)
///   - `recv()`  → Node <- Database (ActorEvent)
pub struct Database {
    /// Address of the ActorRouter living in the Actix system thread.
    router: Addr<ActorRouter>,

    /// One-shot used to request shutdown of the Actix System thread.
    shutdown_tx: Option<oneshot::Sender<()>>,

    /// JoinHandle of the spawned OS thread. We keep it only to ensure
    /// the thread is not detached; we do not join on drop to avoid blocking.
    _thread: JoinHandle<()>, // note: std::thread::JoinHandle, not tokio::task::JoinHandle

    /// Channel used to receive events from the ActorRouter.
    ///
    /// This is the "Database → Node" side, symmetric to Station::recv().
    from_db_rx: mpsc::Receiver<ActorEvent>,
}

impl Database {
    /// Boot the actor system and return a `Database` handle.
    ///
    /// From the node's point of view:
    ///   - `Database::send(cmd)` is used to push high-level commands
    ///     (e.g. Execute) into the actor subsystem.
    ///   - `Database::recv().await` is used to pop `ActorEvent`s produced
    ///     by the ActorRouter (e.g. OperationResult).
    pub async fn start() -> AppResult<Self> {
        // Channel ActorRouter -> Node.
        //
        // `actor_tx` is given to the ActorRouter so that it can push
        // `ActorEvent`s whenever something completes.
        //
        // `actor_rx` is kept inside the Database as `from_db_rx`, so the
        // node can call `db.recv().await`.
        let (actor_tx, actor_rx) = mpsc::channel::<ActorEvent>(128);

        // One-shot used to send the ActorRouter Addr from the Actix thread
        // back to the caller.
        let (router_tx, router_rx) = oneshot::channel::<Addr<ActorRouter>>();

        // One-shot used to request a graceful shutdown of the Actix System.
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        // Spawn the Actix System in a dedicated OS thread.
        //
        // This keeps the actor world completely isolated from the async node
        // code, except for the messages exchanged through:
        // - `actor_tx` / `from_db_rx`
        // - `RouterCmd` (inside the Actix world).
        let thread: JoinHandle<()> = std::thread::spawn(move || {
            let sys = actix::System::new();
            sys.block_on(async move {
                // Start the ActorRouter and send its Addr to the caller.
                let router = ActorRouter::new(actor_tx).start();
                let _ = router_tx.send(router.clone());

                // Keep the Actix System alive until we receive a shutdown signal.
                //
                // Once `shutdown_rx.await` completes, we simply return from
                // this async block, which ends `System::block_on` and shuts
                // down the Actix runtime.
                let _ = shutdown_rx.await;
            });
        });

        // Wait for the ActorRouter Addr to arrive from the Actix thread.
        let router = router_rx.await.map_err(|e| AppError::ActorSystem {
            details: format!("failed to receive ActorRouter address: {e}"),
        })?;

        Ok(Self {
            router,
            shutdown_tx: Some(shutdown_tx),
            _thread: thread,
            from_db_rx: actor_rx,
        })
    }

    /// Send a command into the actor-based "database".
    ///
    /// This is intentionally simple: the node does not know anything about
    /// Actix or ActorRouter internals, it only pushes high-level commands
    /// that the Database translates into `RouterCmd`s.
    pub fn send(&self, cmd: DatabaseCmd) {
        match cmd {
            DatabaseCmd::Execute { op_id, operation } => {
                self.router.do_send(RouterCmd::Execute { op_id, operation });
            }
        }
    }

    /// Receive the next `ActorEvent` coming from the actor subsystem.
    ///
    /// Returns:
    ///   - `Some(ActorEvent)` if the actor system is still alive,
    ///   - `None` if the channel was closed (actor system stopped).
    ///
    /// This is symmetric to `Station::recv()`, so the node can use both
    /// inside a `tokio::select!` in its main loop.
    pub async fn recv(&mut self) -> Option<ActorEvent> {
        self.from_db_rx.recv().await
    }
}

impl Drop for Database {
    /// When the Database handle goes out of scope, request a shutdown of
    /// the Actix System thread by sending on the one-shot channel.
    ///
    /// We do NOT block here by joining the thread; the OS thread will
    /// terminate once the Actix System finishes its shutdown.
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            // If this fails, it usually means the Actix thread is already
            // shutting down or has finished; nothing else to do.
            let _ = tx.send(());
        }
    }
}

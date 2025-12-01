//! Utilities for managing the active connection handlers map.
//!
//! This module provides a small set of helpers to maintain the collection of
//! currently active `Handler` instances. It supports inserting a newly-created
//! handler while enforcing a maximum number of concurrent connections by
//! evicting the least-recently-used handler when necessary.

use super::handler::Handler;
use std::{collections::HashMap, net::SocketAddr};
use tokio::sync::MutexGuard;

/// Insert `handler` into the active handlers map.
///
/// If the map size is already >= `max_conns`, evict the least-recently-used
/// handler to make space, calling `stop()` on the removed handler to abort
/// its background task and release resources.
pub fn add_handler_from(
    active: &mut MutexGuard<'_, HashMap<SocketAddr, Handler>>,
    handler: Handler,
    max_conns: usize,
) {
    if active.len() >= max_conns {
        if let Some(mut removed) = remove_last_recently_used(active) {
            removed.stop();
        }
    }

    active.insert(handler.address, handler);
}

/// Remove and return the least-recently-used handler from the map.
///
/// The LRU handler is determined by the `last_used` timestamp stored in each
/// `Handler`. Returns `Some(Handler)` when an entry was removed, or `None` if
/// the map was empty.
fn remove_last_recently_used(
    active: &mut MutexGuard<'_, HashMap<SocketAddr, Handler>>,
) -> Option<Handler> {
    if let Some((&address, _)) = active.iter().min_by_key(|(_, handler)| handler.last_used) {
        return active.remove(&address);
    }

    None
}

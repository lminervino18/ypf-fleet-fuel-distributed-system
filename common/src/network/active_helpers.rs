use super::handler::Handler;
use std::{collections::HashMap, net::SocketAddr};
use tokio::sync::MutexGuard;

/// Adds the provided `Handler` to the collection of active handlers, if the length of active is
/// greater or equal to `max_conns` then the last recently used handler is removed to make space
/// for the new one.
pub fn add_handler_from(
    active: &mut MutexGuard<'_, HashMap<SocketAddr, Handler>>,
    handler: Handler,
    max_conns: usize,
) {
    if active.len() >= max_conns &&
    let Some(mut removed) = remove_last_recently_used(active) {
        removed.stop();
    }

    active.insert(handler.address, handler);
}

fn remove_last_recently_used(
    active: &mut MutexGuard<'_, HashMap<SocketAddr, Handler>>,
) -> Option<Handler> {
    if let Some((&address, _)) = active.iter().min_by_key(|(_, handler)| handler.last_used) {
        return active.remove(&address);
    }

    None
}

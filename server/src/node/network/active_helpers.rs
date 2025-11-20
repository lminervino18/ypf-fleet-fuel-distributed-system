use super::handler::Handler;
use std::{collections::HashMap, net::SocketAddr};
use tokio::sync::MutexGuard;

pub fn add_handler_from(
    mut active: MutexGuard<'_, HashMap<SocketAddr, Handler>>,
    handler: Handler,
    max_conns: usize,
) {
    if active.len() >= max_conns {
        if let Some(mut removed) = remove_last_recently_used(active) {
            removed.stop();
        };
    }

    active.insert(handler.address, handler);
}

pub fn remove_last_recently_used(
    mut active: MutexGuard<'_, HashMap<SocketAddr, Handler>>,
) -> Option<Handler> {
    if let Some((&address, _)) = active.iter().min_by_key(|(_, handler)| handler.last_used) {
        return active.remove(&address);
    }

    None
}

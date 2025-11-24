use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;

/// Simple hash function to derive a u64 ID from a SocketAddr based on its IP and port
/// That way we have an unique and consistent ID for each node based on its address
pub fn get_id_given_addr(addr: SocketAddr) -> u64 {
    let mut hasher = DefaultHasher::new();
    addr.ip().hash(&mut hasher);
    addr.port().hash(&mut hasher);
    let hash = hasher.finish();
    let max = 100000;
    (hash % max) + 1
}

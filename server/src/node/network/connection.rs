use super::manager::Manager;
use crate::errors::AppError;
use std::net::ToSocketAddrs;

pub struct Connection {
    manager: Manager,
}

impl Connection {
    fn new<Addr: ToSocketAddrs>(listener_addr: Addr, max_streams: usize) -> Self {
        Self {
            manager: Manager::new(max_streams),
        }
    }
}

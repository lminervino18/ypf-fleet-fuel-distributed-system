use common::operation::Operation;
use std::net::SocketAddr;

pub struct PendingOperation {
    pub op: Operation,
    pub client_addr: SocketAddr,
    pub ack_count: usize,
    pub station_request_id: Option<u32>,
}

impl PendingOperation {
    pub fn new(op: Operation, client_addr: SocketAddr, station_request_id: Option<u32>) -> Self {
        Self {
            op,
            client_addr,
            ack_count: 0,
            station_request_id,
        }
    }
}

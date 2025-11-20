use super::handler::Handler;
use std::{collections::HashMap, net::SocketAddr};
use tokio::sync::Mutex;

pub struct ActiveMonitor {
    active: HashMap<SocketAddr, Handler>,
    lck: Mutex<()>,
}

impl<'a> ActiveMonitor {
    pub async fn add(&mut self, address: SocketAddr, stream: Handler) {
        self.lck.lock().await;
        self.active.insert(address, stream);
    }

    pub async fn get_ref(&'a self) -> &'a HashMap<SocketAddr, Handler> {
        self.lck.lock().await;
        &self.active
    }

    pub async fn get_mut(&'a mut self, address: &SocketAddr) -> Option<&'a mut Handler> {
        self.lck.lock().await; // esto se dropea en el return... FIXME
        self.active.get_mut(address)
    }

    pub async fn remove(&mut self, address: &SocketAddr) -> Option<Handler> {
        self.lck.lock().await;
        self.active.remove(address)
    }
}

use super::{acceptor::Acceptor, active_monitor::ActiveMonitor, handler::Handler};
use crate::{
    errors::{AppError, AppResult},
    node::node_message::NodeMessage,
};
use std::{collections::HashMap, net::SocketAddr};
use tokio::{
    net::TcpStream,
    sync::mpsc::{self, Receiver},
    task::JoinHandle,
};

/// Dynamic container for active TCP streams.
pub struct Manager {
    incoming_msgs: Receiver<(SocketAddr, NodeMessage)>,
}

impl Manager {
    pub async fn start(listener_addr: SocketAddr, max_streams: usize) -> AppResult<Self> {
        let acceptor_handle = Acceptor::start(listener_addr).await?;
        let manager = 
    }

    /* async fn create_stream(&mut self, address: SocketAddr) -> Result<&mut Handler, AppError> {
        if self.streams.len() >= self.max_streams {
            self.remove_least_recently_used_stream();
        }

        self.streams
            .insert(address, Handler::new(address).await?);
        if let Some(stream) = self.streams.get_mut(&address) {
            Ok(stream)
        } else {
            Err(AppError::ConnectionRefused {
                addr: address.to_string(),
            })
        }
    }

    async fn get_stream(&mut self, address: SocketAddr) -> Result<&mut Handler, AppError> {
        if !self.streams.contains_key(&address) {
            self.create_stream(address).await?;
        }

        Ok(self.streams.get_mut(&address).unwrap()) // should never panic since conn is single-threaded
    }

    fn remove_least_recently_used_stream(&mut self) {
        if let Some((&address, _)) = self.streams.iter().min_by_key(|(_, conn)| conn.last_used) {
            let Some(stream) = self.streams.remove(&address) else {
                return;
            };

            // TODO: handler.handler.abort();
        }
    } */
}

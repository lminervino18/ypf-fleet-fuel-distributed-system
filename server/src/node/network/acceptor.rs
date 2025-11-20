use super::active_monitor::ActiveMonitor;
use super::handler::Handler;
use crate::errors::{AppError, AppResult};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::{self, JoinHandle};

const INCOMING_BUFF_SIZE: usize = 300;

pub struct Acceptor {
    listener: TcpListener,
    active: Arc<ActiveMonitor>,
}

impl Acceptor {
    pub async fn start(
        address: SocketAddr,
        active: Arc<ActiveMonitor>,
    ) -> AppResult<JoinHandle<()>> {
        let mut acceptor = Acceptor::new(address, active).await?;
        let handle = task::spawn(async move {
            acceptor.run().await;
        });

        Ok(handle)
    }

    async fn new(address: SocketAddr, active: Arc<ActiveMonitor>) -> AppResult<(Self)> {
        Ok(Self {
            listener: TcpListener::bind(address).await.map_err(|_| {
                AppError::ConnectionRefused {
                    addr: address.to_string(),
                }
            })?,
            active,
        })
    }

    async fn run(&mut self) {
        while let Ok((stream, address)) = self.listener.accept().await {
            let Ok(handler) = Handler::start_from(stream).await else {
                continue;
            };

            // self.active.add(address).await;
        }
    }
}

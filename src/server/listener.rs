//! RTMP server listener
//!
//! Handles TCP accept loop and spawns connection handlers.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;

use crate::error::Result;
use crate::server::config::ServerConfig;
use crate::server::connection::Connection;
use crate::server::handler::RtmpHandler;

/// RTMP server
pub struct RtmpServer<H: RtmpHandler> {
    config: ServerConfig,
    handler: Arc<H>,
    next_session_id: AtomicU64,
    connection_semaphore: Option<Arc<Semaphore>>,
}

impl<H: RtmpHandler> RtmpServer<H> {
    /// Create a new server with the given configuration and handler
    pub fn new(config: ServerConfig, handler: H) -> Self {
        let connection_semaphore = if config.max_connections > 0 {
            Some(Arc::new(Semaphore::new(config.max_connections)))
        } else {
            None
        };

        Self {
            config,
            handler: Arc::new(handler),
            next_session_id: AtomicU64::new(1),
            connection_semaphore,
        }
    }

    /// Run the server
    ///
    /// This method blocks until the server is shut down.
    pub async fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(self.config.bind_addr).await?;
        tracing::info!(addr = %self.config.bind_addr, "RTMP server listening");

        loop {
            match listener.accept().await {
                Ok((socket, peer_addr)) => {
                    self.handle_connection(socket, peer_addr).await;
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to accept connection");
                }
            }
        }
    }

    /// Run the server with graceful shutdown
    pub async fn run_until<F>(&self, shutdown: F) -> Result<()>
    where
        F: std::future::Future<Output = ()>,
    {
        let listener = TcpListener::bind(self.config.bind_addr).await?;
        tracing::info!(addr = %self.config.bind_addr, "RTMP server listening");

        tokio::select! {
            _ = shutdown => {
                tracing::info!("Shutdown signal received");
                Ok(())
            }
            result = self.accept_loop(&listener) => result,
        }
    }

    async fn accept_loop(&self, listener: &TcpListener) -> Result<()> {
        loop {
            match listener.accept().await {
                Ok((socket, peer_addr)) => {
                    self.handle_connection(socket, peer_addr).await;
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to accept connection");
                }
            }
        }
    }

    async fn handle_connection(&self, socket: TcpStream, peer_addr: SocketAddr) {
        // Check connection limit
        let _permit = if let Some(ref sem) = self.connection_semaphore {
            match sem.clone().try_acquire_owned() {
                Ok(permit) => Some(permit),
                Err(_) => {
                    tracing::warn!(peer = %peer_addr, "Connection rejected: limit reached");
                    return;
                }
            }
        } else {
            None
        };

        // Generate session ID
        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);

        tracing::debug!(
            session_id = session_id,
            peer = %peer_addr,
            "New connection"
        );

        // Configure socket
        if let Err(e) = self.configure_socket(&socket) {
            tracing::error!(error = %e, "Failed to configure socket");
            return;
        }

        // Spawn connection handler
        let config = self.config.clone();
        let handler = Arc::clone(&self.handler);

        tokio::spawn(async move {
            let mut connection = Connection::new(
                session_id,
                socket,
                peer_addr,
                config,
                handler,
            );

            if let Err(e) = connection.run().await {
                tracing::debug!(
                    session_id = session_id,
                    error = %e,
                    "Connection error"
                );
            }

            tracing::debug!(session_id = session_id, "Connection closed");
        });
    }

    fn configure_socket(&self, socket: &TcpStream) -> std::io::Result<()> {
        if self.config.tcp_nodelay {
            socket.set_nodelay(true)?;
        }

        // Note: Setting buffer sizes requires platform-specific handling
        // and may fail on some systems. We ignore errors here.
        // if self.config.tcp_recv_buffer > 0 {
        //     let _ = socket.set_recv_buffer_size(self.config.tcp_recv_buffer);
        // }
        // if self.config.tcp_send_buffer > 0 {
        //     let _ = socket.set_send_buffer_size(self.config.tcp_send_buffer);
        // }

        Ok(())
    }

    /// Get the bind address
    pub fn bind_addr(&self) -> SocketAddr {
        self.config.bind_addr
    }
}

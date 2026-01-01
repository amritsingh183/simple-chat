mod chat;

use std::{env, sync::Arc};

use chat::{broker::get_broker, connection::handle_connection};
use common::{consts::MAX_CONNECTIONS, telemetry};
use tokio::{
    net::TcpListener,
    sync::Semaphore,
    time::{Duration, interval},
};
use tracing::{error, info, warn};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: &str = "8080";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = telemetry::init_logging().map_err(|e| format!("Failed to initialize logging: {e}"))?;

    let host = env::var("CHAT_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
    let port = env::var("CHAT_PORT").unwrap_or_else(|_| DEFAULT_PORT.to_string());
    let addr = format!("{host}:{port}");

    let listener = TcpListener::bind(&addr).await?;
    info!("Chat server listening on {addr}");

    let _broker = get_broker();
    chat::broker::start_dispatcher().await;
    info!("Message dispatcher started");

    let connection_semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    info!("Max concurrent connections: {MAX_CONNECTIONS}");

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let shutdown = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            error!("Failed to listen for CTRL+C: {e}");
        }
        info!("Shutdown signal received");
    };

    tokio::select! {
        () = accept_connections(&listener, connection_semaphore, shutdown_rx) => {}
        () = shutdown => {
            let _ = shutdown_tx.send(true);
            info!("Shutting down server...");
        }
    }

    get_broker().shutdown().await;
    info!("Server shutdown complete");
    Ok(())
}

async fn accept_connections(
    listener: &TcpListener,
    semaphore: Arc<Semaphore>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let mut error_backoff = interval(Duration::from_millis(100));
    loop {
        let permit = if let Ok(p) = semaphore.clone().try_acquire_owned() {
            p
        } else {
            warn!("Connection limit reached ({MAX_CONNECTIONS}), waiting...");
            let Ok(p) = semaphore.clone().acquire_owned().await else {
                error!("Semaphore closed unexpectedly");
                return;
            };
            p
        };

        // Now accept — we have capacity
        if let Ok((tcp_stream, sock_addr)) = listener.accept().await {
            let conn_shutdown_rx = shutdown_rx.clone();
            tokio::spawn(async move {
                let _permit = permit;
                handle_connection(tcp_stream, sock_addr, conn_shutdown_rx).await;
            });
        } else {
            error!("Failed to accept connection");
            error_backoff.tick().await;
        }
    }
}

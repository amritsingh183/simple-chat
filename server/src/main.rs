mod chat;

use std::{env, sync::Arc};

use chat::{broker::get_broker, connection::handle_connection};
use common::{security::MAX_CONNECTIONS, telemetry};
use tokio::{net::TcpListener, sync::Semaphore};
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
    info!("Message dispatcher started");

    let connection_semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    info!("Max concurrent connections: {MAX_CONNECTIONS}");

    let shutdown = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            error!("Failed to listen for CTRL+C: {e}");
        }
        info!("Shutdown signal received");
    };

    tokio::select! {
        () = accept_connections(&listener, connection_semaphore) => {}
        () = shutdown => {
            info!("Shutting down server...");
        }
    }

    info!("Server shutdown complete");
    Ok(())
}

async fn accept_connections(listener: &TcpListener, semaphore: Arc<Semaphore>) {
    loop {
        let accept_result = listener.accept().await;

        let permit = if let Ok(permit) = semaphore.clone().try_acquire_owned() {
            permit
        } else {
            warn!("Connection limit reached ({MAX_CONNECTIONS}), new connection may be delayed");
            if let Ok(permit) = semaphore.clone().acquire_owned().await {
                permit
            } else {
                error!("Semaphore closed unexpectedly");
                return;
            }
        };

        match accept_result {
            Ok((socket, addr)) => {
                tokio::spawn(async move {
                    let _permit = permit;
                    handle_connection(socket, addr).await;
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {e}");

                drop(permit);

                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
    }
}

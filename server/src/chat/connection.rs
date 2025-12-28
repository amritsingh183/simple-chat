use std::net::SocketAddr;

use common::security::{self, MAX_LINE_LENGTH, READ_TIMEOUT};
use crossbeam::channel::{Receiver, Sender, bounded};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpStream, tcp::OwnedWriteHalf},
    time::timeout,
};
use tracing::{error, info, warn};

use crate::chat::{
    broker::get_broker,
    client_protocol::ClientCommand,
    message::ChatMessage,
    rate_limiter::RateLimiter,
    server_protocol::ServerMessage,
    user::{User, Username},
};

const USER_CHANNEL_BUFFER_SIZE: usize = 256;

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("read timeout")]
    Timeout,

    #[error("message too long (max {MAX_LINE_LENGTH} bytes)")]
    MessageTooLong,
}

struct Unauthenticated {
    addr: SocketAddr,
    tx: Sender<String>,
    rx: Receiver<String>,
}

struct Joined {
    user: User,
    addr: SocketAddr,
    rx: Receiver<String>,

    rate_limiter: RateLimiter,
}

impl Unauthenticated {
    fn new(addr: SocketAddr) -> Self {
        let (tx, rx) = bounded(USER_CHANNEL_BUFFER_SIZE);
        Self { addr, tx, rx }
    }

    fn try_join(self, raw_username: String) -> Result<Joined, (Self, String)> {
        let username = match Username::new(raw_username) {
            Ok(u) => u,
            Err(e) => return Err((self, e.to_string())),
        };

        let broker = get_broker();
        let registry = broker.registry();

        match registry.register(&username, self.tx.clone()) {
            Ok(registered_user) => {
                let join_msg = ServerMessage::UserJoined {
                    username: registered_user.get_username().to_string(),
                };

                let msg = ChatMessage::new(&registered_user, join_msg.to_string()).serialize();
                if let Err(e) = broker.forward_to_room(msg) {
                    warn!("Failed to send message to room: {e}");
                }

                let safe_username = security::sanitize_for_log(&username.to_string());
                info!("User '{}' joined from {}", safe_username, self.addr);

                Ok(Joined {
                    user: registered_user,
                    addr: self.addr,
                    rx: self.rx,
                    rate_limiter: RateLimiter::new(),
                })
            }
            Err(e) => Err((self, e.to_string())),
        }
    }
}

impl Joined {
    async fn drain_broadcasts(&self, writer: &mut OwnedWriteHalf) -> Result<(), ConnectionError> {
        while let Ok(msg) = self.rx.try_recv() {
            writer.write_all(msg.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
        Ok(())
    }

    fn cleanup(self) {
        let username = self.user.get_username();
        let broker = get_broker();
        let registry = broker.registry();

        let _ = registry.unregister(&self.user);

        let leave_msg = ServerMessage::UserLeft {
            username: username.to_string(),
        };
        let msg = ChatMessage::new(&self.user, leave_msg.to_string()).serialize();
        if let Err(e) = broker.forward_to_room(msg) {
            warn!("Failed to send message to room: {e}");
        }

        let safe_username = security::sanitize_for_log(&username.to_string());
        info!("User '{}' disconnected", safe_username);
    }
}

pub async fn handle_connection(stream: TcpStream, addr: SocketAddr) {
    info!("New connection from {addr}");

    if let Err(e) = handle_connection_inner(stream, addr).await {
        error!("Connection {addr} error: {e}");
    }
}

async fn handle_connection_inner(stream: TcpStream, addr: SocketAddr) -> Result<(), ConnectionError> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    let state = Unauthenticated::new(addr);

    let joined = wait_for_join(state, &mut reader, &mut writer, &mut line).await?;

    if let Some(joined) = joined {
        handle_joined_session(joined, &mut reader, &mut writer, &mut line).await?;
    }

    Ok(())
}

async fn wait_for_join(
    mut state: Unauthenticated,
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    line: &mut String,
) -> Result<Option<Joined>, ConnectionError> {
    loop {
        line.clear();

        let read_result = timeout(READ_TIMEOUT, reader.read_line(line)).await;

        let bytes_read = match read_result {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(ConnectionError::Io(e)),
            Err(_) => {
                warn!("Connection {} timed out during join", state.addr);
                return Err(ConnectionError::Timeout);
            }
        };

        if bytes_read > MAX_LINE_LENGTH {
            warn!("Connection {} sent oversized message during join", state.addr);
            send_message_to_client(
                writer,
                &ServerMessage::Error {
                    reason: "message too long".to_string(),
                },
            )
            .await?;
            return Err(ConnectionError::MessageTooLong);
        }

        if bytes_read == 0 {
            info!("Connection {} closed before joining", state.addr);
            return Ok(None);
        }

        match line.trim().parse::<ClientCommand>() {
            Ok(ClientCommand::Join { username }) => match state.try_join(username) {
                Ok(joined) => {
                    send_message_to_client(writer, &ServerMessage::Ok).await?;
                    return Ok(Some(joined));
                }
                Err((returned_state, reason)) => {
                    state = returned_state;
                    send_message_to_client(writer, &ServerMessage::Error { reason }).await?;
                }
            },

            Ok(_) => {
                send_message_to_client(
                    writer,
                    &ServerMessage::Error {
                        reason: "must join first".to_string(),
                    },
                )
                .await?;
            }
            Err(e) => {
                warn!("Invalid command from {}: {e}", state.addr);
                send_message_to_client(writer, &ServerMessage::Error { reason: e.to_string() }).await?;
            }
        }
    }
}

async fn handle_joined_session(
    joined: Joined,
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    line: &mut String,
) -> Result<(), ConnectionError> {
    let broker = get_broker();
    loop {
        line.clear();

        tokio::select! {

            read_result = timeout(READ_TIMEOUT, reader.read_line(line)) => {
                let bytes_read = match read_result {
                    Ok(Ok(n)) => n,
                    Ok(Err(e)) => return Err(ConnectionError::Io(e)),
                    Err(_) => {

                        continue;
                    }
                };


                if bytes_read > MAX_LINE_LENGTH {
                    let safe_username = security::sanitize_for_log(&joined.user.get_username().to_string());
                    warn!("User '{}' sent oversized message", safe_username);
                    send_message_to_client(writer, &ServerMessage::Error {
                        reason: "message too long".to_string(),
                    }).await?;
                    continue;
                }

                if bytes_read == 0 {
                    info!("Connection {} closed by client", joined.addr);
                    break;
                }

                match line.trim().parse::<ClientCommand>() {
                    Ok(ClientCommand::Join { .. }) => {
                        send_message_to_client(writer, &ServerMessage::Error {
                            reason: "already joined".to_string(),
                        }).await?;
                    }

                    Ok(ClientCommand::Send { message }) => {
                        joined.rate_limiter.acquire().await;

                        let broadcast_message = ServerMessage::BroadcastMessage {
                            text: ChatMessage::new(&joined.user, message).serialize(),
                        };

                        let msg = ChatMessage::new(&joined.user, broadcast_message.to_string()).serialize();
                        if let Err(e) = broker.forward_to_room(msg) {
                            warn!("Failed to send message to room: {e}");
                            send_message_to_client(writer, &ServerMessage::Error {
                                reason: e.to_string(),
                            }).await?;
                        }
                    }

                    Ok(ClientCommand::Leave) => {
                        let safe_username = security::sanitize_for_log(&joined.user.get_username().to_string());
                        info!("User '{}' requested leave from {}", safe_username, joined.addr);
                        break;
                    }

                    Err(e) => {
                        warn!("Invalid command from {}: {e}", joined.addr);
                        send_message_to_client(writer, &ServerMessage::Error {
                            reason: e.to_string(),
                        }).await?;
                    }
                }
            }


            () = tokio::task::yield_now() => {
                joined.drain_broadcasts(writer).await?;
            }
        }
    }

    joined.cleanup();
    Ok(())
}

async fn send_message_to_client(writer: &mut OwnedWriteHalf, msg: &ServerMessage) -> Result<(), std::io::Error> {
    writer.write_all(msg.to_string().as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

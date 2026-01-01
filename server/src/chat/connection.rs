use std::net::SocketAddr;

use common::{
    consts::{MAX_CLIENT_BUFFER_SIZE, READ_TIMEOUT},
    tcp_message::{self, ClientMessage, ServerMessage, WireDecode, WireEncode},
};
use thiserror::Error as ThisError;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpStream, tcp::OwnedWriteHalf},
    sync::mpsc::{self, Receiver, Sender},
    time::timeout,
};
use tracing::{error, info, warn};

use crate::chat::{
    broker::get_broker,
    rate_limiter::RateLimiter,
    room::OneToMany,
    user::{Error as UserError, User, Username},
};

const USER_CHANNEL_BUFFER_SIZE: usize = 256;

#[derive(Debug, ThisError)]
pub enum ConnectionError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("read timeout")]
    Timeout,

    #[error("message too long (max {MAX_CLIENT_BUFFER_SIZE} bytes)")]
    MessageTooLong,
}

/// Connection state machine.
/// Transitions: Unauthenticated -> Joined -> Disconnected
enum ConnectionState {
    Unauthenticated(Unauthenticated),
    Joined(Joined),
    Disconnected,
}

struct Unauthenticated {
    addr: SocketAddr,
    tx: Sender<OneToMany>,
    rx: Receiver<OneToMany>,
}

struct Joined {
    user: User,
    addr: SocketAddr,
    rx: Receiver<OneToMany>,

    rate_limiter: RateLimiter,
}

impl Unauthenticated {
    fn new(addr: SocketAddr) -> Self {
        let (tx, rx) = mpsc::channel(USER_CHANNEL_BUFFER_SIZE);
        Self { addr, tx, rx }
    }
    // shall not be responsible for sending notifications
    fn join(self, raw_username: &String) -> Result<Joined, (Self, String)> {
        let username = match Username::new(raw_username) {
            Ok(u) => u,
            Err(e) => return Err((self, e.to_string())),
        };

        match get_broker().registry().register(&username, self.tx.clone()) {
            Ok(registered_user) => Ok(Joined {
                user: registered_user,
                addr: self.addr,
                rx: self.rx,
                rate_limiter: RateLimiter::new(),
            }),
            Err(e) => Err((self, e.to_string())),
        }
    }
}

impl Joined {
    async fn drain_broadcasts(&mut self, writer: &mut OwnedWriteHalf) -> Result<(), ConnectionError> {
        while let Ok(msg) = self.rx.try_recv() {
            writer.write_all(&msg).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
        Ok(())
    }

    fn leave(self) -> Result<bool, UserError> {
        get_broker().registry().unregister(&self.user)
    }
}

pub async fn handle_connection(stream: TcpStream, addr: SocketAddr, shutdown_rx: tokio::sync::watch::Receiver<bool>) {
    info!("New connection from {addr}");
    if let Err(e) = run_state_machine(stream, addr, shutdown_rx).await {
        error!("Connection {addr} error: {e}");
    }
}

async fn run_state_machine(
    stream: TcpStream,
    addr: SocketAddr,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), ConnectionError> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::with_capacity(MAX_CLIENT_BUFFER_SIZE);
    let mut state = ConnectionState::Unauthenticated(Unauthenticated::new(addr));
    loop {
        state = match state {
            ConnectionState::Unauthenticated(unauth) => {
                buf.clear();
                match tick_unauthenticated(unauth, &mut reader, &mut writer, &mut buf, &mut shutdown_rx).await {
                    Ok(s) => s,
                    Err(e) => return Err(e),
                }
            }
            ConnectionState::Joined(mut joined) => {
                // Drain pending broadcasts first
                joined.drain_broadcasts(&mut writer).await?;
                buf.clear();
                match tick_joined(joined, &mut reader, &mut writer, &mut buf, &mut shutdown_rx).await {
                    Ok(s) => s,
                    Err(e) => return Err(e),
                }
            }
            ConnectionState::Disconnected => break,
        };
    }

    Ok(())
}

enum InputEvent {
    Data(usize),
    Broadcast(OneToMany),
    Shutdown,
    Timeout,
    Continue,
}

async fn wait_for_input(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    buf: &mut Vec<u8>,
    shutdown_rx: &mut tokio::sync::watch::Receiver<bool>,
    rx: Option<&mut Receiver<OneToMany>>,
) -> Result<InputEvent, ConnectionError> {
    tokio::select! {
        biased; // poll top to bottom
        _ = shutdown_rx.changed() => {
            if *shutdown_rx.borrow() {
                Ok(InputEvent::Shutdown)
            } else {
                Ok(InputEvent::Continue)
            }
        }
        maybe_msg = async {
            if let Some(r) = rx {
                r.recv().await
            } else {
                std::future::pending().await
            }
        } => {
            maybe_msg.map_or( Ok(InputEvent::Continue), |msg|{
                Ok(InputEvent::Broadcast(msg))
            })
        }
        result = timeout(READ_TIMEOUT, async {
            let limit = (MAX_CLIENT_BUFFER_SIZE + 1) as u64;
            let mut take = reader.take(limit);
            take.read_until(b'\n', buf).await
        }) => {
            match result {
                Ok(Ok(n)) => {
                    if n > MAX_CLIENT_BUFFER_SIZE {
                        Err(ConnectionError::MessageTooLong)
                    } else {
                        Ok(InputEvent::Data(n))
                    }
                }
                Ok(Err(e)) => Err(ConnectionError::Io(e)),
                Err(_) => Ok(InputEvent::Timeout),
            }
        }
    }
}

async fn tick_unauthenticated(
    mut state: Unauthenticated,
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    buf: &mut Vec<u8>,
    shutdown_rx: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<ConnectionState, ConnectionError> {
    let event = match wait_for_input(reader, buf, shutdown_rx, Some(&mut state.rx)).await {
        Ok(event) => event,
        Err(e) => return Err(e),
    };

    match event {
        InputEvent::Broadcast(_) => {
            // Should not happen for unauthenticated user
            Ok(ConnectionState::Unauthenticated(state))
        }
        InputEvent::Shutdown => {
            info!("Shutdown signal received for connection {} during join", state.addr);
            Ok(ConnectionState::Disconnected)
        }
        InputEvent::Continue => Ok(ConnectionState::Unauthenticated(state)),
        InputEvent::Timeout => {
            warn!("Connection {} timed out during join", state.addr);
            Err(ConnectionError::Timeout)
        }
        InputEvent::Data(0) => {
            info!("Connection {} closed before joining", state.addr);
            Ok(ConnectionState::Disconnected)
        }
        InputEvent::Data(_) => match tcp_message::ClientMessage::decode(buf) {
            Ok(ClientMessage::Join { username }) => match state.join(&username) {
                Ok(joined) => {
                    let broadcast_message = ServerMessage::UserJoined { username };
                    if let Err(e) = get_broker().forward_to_room(broadcast_message.encode()) {
                        warn!("Failed to send message to room: {e}");
                        send_message_to_client(writer, &ServerMessage::Err { reason: e.to_string() }).await?;
                    }
                    send_message_to_client(writer, &ServerMessage::Ok).await?;
                    Ok(ConnectionState::Joined(joined))
                }
                Err((returned_state, reason)) => {
                    send_message_to_client(writer, &ServerMessage::Err { reason }).await?;
                    Ok(ConnectionState::Unauthenticated(returned_state))
                }
            },
            Ok(_) => {
                send_message_to_client(
                    writer,
                    &ServerMessage::Err {
                        reason: "must join first".to_string(),
                    },
                )
                .await?;
                Ok(ConnectionState::Unauthenticated(state))
            }
            Err(e) => {
                warn!("Invalid command from {}: {e}", state.addr);
                send_message_to_client(writer, &ServerMessage::Err { reason: e.to_string() }).await?;
                Ok(ConnectionState::Unauthenticated(state))
            }
        },
    }
}

/// Process one tick in Joined state. Returns next state.
async fn tick_joined(
    mut joined: Joined,
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    buf: &mut Vec<u8>,
    shutdown_rx: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<ConnectionState, ConnectionError> {
    let username = joined.user.get_username().to_string();

    let rx = &mut joined.rx;
    let event = match wait_for_input(reader, buf, shutdown_rx, Some(rx)).await {
        Ok(event) => event,
        Err(ConnectionError::MessageTooLong) => {
            // Drain the rest of the line if incomplete
            if buf.last() != Some(&b'\n') {
                loop {
                    buf.clear();
                    let limit = common::consts::MAX_CLIENT_MESSAGE_LENGTH as u64;
                    let n = reader.take(limit).read_until(b'\n', buf).await?;
                    if n == 0 || buf.last() == Some(&b'\n') {
                        break;
                    }
                }
            }
            buf.clear();

            send_message_to_client(
                writer,
                &ServerMessage::Err {
                    reason: ConnectionError::MessageTooLong.to_string(),
                },
            )
            .await?;
            return Ok(ConnectionState::Joined(joined));
        }
        Err(e) => return Err(e),
    };

    match event {
        InputEvent::Broadcast(msg) => {
            writer.write_all(&msg).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            Ok(ConnectionState::Joined(joined))
        }
        InputEvent::Shutdown => {
            info!("Shutdown signal received for connection {}", joined.addr);
            joined.drain_broadcasts(writer).await?;
            if let Err(e) = joined.leave() {
                warn!("Failed to leave: {e}");
            }
            Ok(ConnectionState::Disconnected)
        }
        InputEvent::Timeout | InputEvent::Continue => Ok(ConnectionState::Joined(joined)),
        InputEvent::Data(0) => {
            info!("Connection {} closed by client", joined.addr);
            joined.drain_broadcasts(writer).await?;
            if let Err(e) = joined.leave() {
                warn!("Failed to leave: {e}");
            }
            let broadcast_message = ServerMessage::UserLeft { username };
            if let Err(e) = get_broker().forward_to_room(broadcast_message.encode()) {
                warn!("Failed to send message to room: {e}");
                send_message_to_client(writer, &ServerMessage::Err { reason: e.to_string() }).await?;
            }
            Ok(ConnectionState::Disconnected)
        }
        InputEvent::Data(_) => {
            let should_disconnect = handle_joined_message(&joined, writer, buf).await?;
            if should_disconnect {
                joined.drain_broadcasts(writer).await?;
                if let Err(e) = joined.leave() {
                    warn!("Failed to leave: {e}");
                }
                Ok(ConnectionState::Disconnected)
            } else {
                Ok(ConnectionState::Joined(joined))
            }
        }
    }
}

/// Handle a message while in Joined state.
async fn handle_joined_message(
    joined: &Joined,
    writer: &mut OwnedWriteHalf,
    buf: &[u8],
) -> Result<bool, ConnectionError> {
    // Size check is handled in wait_for_input

    let broker = get_broker();

    match ClientMessage::decode(buf) {
        Ok(ClientMessage::Send { message }) => {
            joined.rate_limiter.acquire().await;
            let broadcast_message = ServerMessage::Broadcast {
                username: joined.user.get_username().to_string(),
                message,
            };

            if let Err(e) = broker.forward_to_room(broadcast_message.encode()) {
                warn!("Failed to send message to room: {e}");
                send_message_to_client(writer, &ServerMessage::Err { reason: e.to_string() }).await?;
            }
        }
        Ok(ClientMessage::Leave) => {
            info!(
                "User '{}' requested leave from {}",
                joined.user.get_username(),
                joined.addr
            );
            return Ok(true);
        }
        Ok(_) => {
            let msg = "invlaid command for `Joined state`".to_string();
            warn!("{} from {}", msg, joined.addr);
            send_message_to_client(writer, &ServerMessage::Err { reason: msg }).await?;
        }
        Err(e) => {
            warn!("Invalid command from {}: {e}", joined.addr);
            send_message_to_client(writer, &ServerMessage::Err { reason: e.to_string() }).await?;
        }
    }

    Ok(false)
}

async fn send_message_to_client(writer: &mut OwnedWriteHalf, msg: &ServerMessage) -> Result<(), std::io::Error> {
    writer.write_all(msg.to_string().as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

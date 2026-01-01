use std::{
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use clap::Parser;
use common::{
    consts,
    tcp_message::{ClientMessage, ServerMessage, WireDecode, WireEncode},
};
use rustyline::{DefaultEditor, error::ReadlineError};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::mpsc,
};
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(author, version, about = "Chat client CLI")]
struct Args {
    #[arg(long, env = consts::ENV_CHAT_HOST, default_value = "127.0.0.1")]
    host: String,

    #[arg(long, env = consts::ENV_CHAT_PORT, default_value = "8080")]
    port: u16,

    #[arg(long, env = consts::ENV_CHAT_USERNAME,)]
    username: String,
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("connection failed: {0}")]
    Connection(#[from] std::io::Error),

    #[error("server error: {0}")]
    ServerError(String),

    #[error("readline error: {0}")]
    Readline(#[from] ReadlineError),
}

struct DisconnectedClient {
    host: String,
    port: u16,
    username: String,
}

struct ConnectedClient {
    username: String,
    reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: tokio::net::tcp::OwnedWriteHalf,
}

struct JoinedClient {
    username: String,
    shutdown: Arc<AtomicBool>,
}

impl DisconnectedClient {
    fn new(args: Args) -> Self {
        Self {
            host: args.host,
            port: args.port,
            username: args.username,
        }
    }

    async fn connect(self) -> Result<ConnectedClient, ClientError> {
        let addr = format!("{}:{}", self.host, self.port);
        println!("Connecting to {addr}...");

        let stream = TcpStream::connect(&addr).await?;
        println!("Connected!");

        let (reader, writer) = stream.into_split();
        let reader = BufReader::new(reader);

        Ok(ConnectedClient {
            username: self.username,
            reader,
            writer,
        })
    }
}

impl ConnectedClient {
    async fn join(
        mut self,
    ) -> Result<
        (
            JoinedClient,
            BufReader<tokio::net::tcp::OwnedReadHalf>,
            tokio::net::tcp::OwnedWriteHalf,
        ),
        ClientError,
    > {
        let join_msg = ClientMessage::Join {
            username: self.username.clone(),
        };
        let encoded = join_msg.encode();
        self.writer.write_all(&encoded).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;

        let mut response = String::new();
        self.reader.read_line(&mut response).await?;

        // Parse response using new wire protocol
        match ServerMessage::decode(response.trim().as_bytes()) {
            Ok(ServerMessage::Ok) => {}
            Ok(ServerMessage::Err { reason }) => {
                return Err(ClientError::ServerError(reason));
            }
            _ => {
                return Err(ClientError::ServerError(response.trim().to_string()));
            }
        }

        println!(
            "Joined as '{}'. Type 'send <message>' or 'leave' to exit.",
            self.username
        );
        println!("Use arrow keys for history navigation.\n");

        let joined = JoinedClient {
            username: self.username,
            shutdown: Arc::new(AtomicBool::new(false)),
        };

        Ok((joined, self.reader, self.writer))
    }
}

impl JoinedClient {
    async fn run(
        self,
        reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
        mut writer: tokio::net::tcp::OwnedWriteHalf,
    ) -> Result<(), ClientError> {
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<String>(32);
        let shutdown_clone = Arc::clone(&self.shutdown);
        let reader_handle = tokio::spawn(async move {
            read_server_messages(&self.username, reader, shutdown_clone).await;
        });
        let shutdown_clone = Arc::clone(&self.shutdown);
        let readline_handle = std::thread::spawn(move || {
            read_joined_user_input(&cmd_tx, &shutdown_clone);
        });
        while let Some(input) = cmd_rx.recv().await {
            if self.shutdown.load(Ordering::SeqCst) {
                break;
            }
            let trimmed = input.trim();
            if trimmed.eq_ignore_ascii_case(consts::CLIENT_LEAVE_CMD) {
                let encoded = ClientMessage::Leave.encode();
                writer.write_all(&encoded).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                println!("Goodbye!");
                break;
            } else if let Some(msg) = trimmed
                .strip_prefix(consts::CLIENT_SEND_PREFIX.to_ascii_lowercase().as_str())
                .or_else(|| trimmed.strip_prefix(consts::CLIENT_SEND_PREFIX))
            {
                let send_msg = ClientMessage::Send {
                    message: msg.to_string(),
                };
                let encoded = send_msg.encode();
                if let Err(e) = writer.write_all(&encoded).await {
                    eprintln!("Failed to send: {e}");
                    break;
                }
                let _ = writer.write_all(b"\n").await;
                let _ = writer.flush().await;
            } else {
                println!("Unknown command. Use 'send <message>' or 'leave'.");
            }
        }
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = reader_handle.await;
        let _ = readline_handle.join();
        Ok(())
    }
}

async fn read_server_messages(
    username: &str,
    mut reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    shutdown: Arc<AtomicBool>,
) {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                println!("\nDisconnected from server.");
                shutdown.store(true, Ordering::SeqCst);
                break;
            }
            Ok(_) => parse_server_message(username, &line),
            Err(e) => {
                eprintln!("\nRead error: {e}");
                shutdown.store(true, Ordering::SeqCst);
                break;
            }
        }
    }
}

fn read_joined_user_input(cmd_tx: &mpsc::Sender<String>, shutdown: &Arc<AtomicBool>) {
    let Ok(mut rl) = DefaultEditor::new() else {
        error!("unable to create DefaultEditor");
        return;
    };

    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        match rl.readline("> ") {
            Ok(line) => {
                let user_input = line.trim();
                if user_input.is_empty() {
                    continue;
                }
                if cmd_tx.blocking_send(user_input.to_string()).is_err() {
                    break;
                }
                // add to history, only success cmds
                let _ = rl.add_history_entry(user_input).map_err(|_| {
                    info!("unable to add to history {}", user_input);
                });
            }
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => {
                if cmd_tx.blocking_send("leave".to_string()).is_err() {
                    break;
                }
                break;
            }
            Err(e) => {
                warn!("line error {}", e.to_string());
                break;
            }
        }
    }
}

/// Parse server message using new wire protocol
fn parse_server_message(this_user: &str, line: &str) {
    let trimmed = line.trim();
    match ServerMessage::decode(trimmed.as_bytes()) {
        Ok(ServerMessage::Ok) => {
            // Silent acknowledgment
        }
        Ok(ServerMessage::Err { reason }) => {
            println!("\r[ERROR]: {reason}");
        }
        Ok(ServerMessage::UserJoined { username }) => {
            if username != this_user {
                println!("\r*** {username} joined the chat ***");
            }
        }
        Ok(ServerMessage::UserLeft { username }) => {
            if username != this_user {
                println!("\r*** {username} left the chat ***");
            }
        }
        Ok(ServerMessage::Broadcast { username, message }) => {
            if username != this_user {
                println!("\r[{username}]: {message}");
            }
        }
        Err(_) => {
            if !trimmed.is_empty() {
                println!("\r{trimmed}");
            }
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    let disconnected = DisconnectedClient::new(args);

    let connected = match disconnected.connect().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Connection error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let (joined, reader, writer) = match connected.join().await {
        Ok(j) => j,
        Err(e) => {
            eprintln!("Join error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = joined.run(reader, writer).await {
        eprintln!("Error: {e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

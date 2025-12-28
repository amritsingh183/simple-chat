use std::{
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use clap::Parser;
use common::consts;
use rustyline::{DefaultEditor, error::ReadlineError};
use stringzilla::sz;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::mpsc,
};

#[derive(Parser, Debug)]
#[command(author, version, about = "Chat client CLI")]
struct Args {
    #[arg(long, env = "CHAT_HOST", default_value = "127.0.0.1")]
    host: String,

    #[arg(long, env = "CHAT_PORT", default_value = "8080")]
    port: u16,

    #[arg(long, env = "CHAT_USERNAME")]
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
        let join_cmd = format!("{}{}\n", consts::CLIENT_JOIN_PREFIX, self.username);
        self.writer.write_all(join_cmd.as_bytes()).await?;
        self.writer.flush().await?;

        let mut response = String::new();
        self.reader.read_line(&mut response).await?;

        if response.trim().starts_with(consts::SERVER_ERR_PREFIX.trim()) {
            return Err(ClientError::ServerError(response.trim().to_string()));
        }

        println!(
            "Joined as '{}'. Type 'send <message>' or 'leave' to exit.",
            self.username
        );
        println!("Use arrow keys for history navigation.\n");

        let joined = JoinedClient {
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
            read_server_messages(reader, shutdown_clone).await;
        });

        let shutdown_clone = Arc::clone(&self.shutdown);
        let readline_handle = std::thread::spawn(move || {
            read_user_input(&cmd_tx, &shutdown_clone);
        });

        while let Some(input) = cmd_rx.recv().await {
            if self.shutdown.load(Ordering::SeqCst) {
                break;
            }

            let trimmed = input.trim();

            if trimmed.eq_ignore_ascii_case(consts::CLIENT_LEAVE_CMD) {
                writer
                    .write_all(format!("{}\n", consts::CLIENT_LEAVE_PREFIX).as_bytes())
                    .await?;
                writer.flush().await?;
                println!("Goodbye!");
                break;
            } else if let Some(msg) = trimmed
                .strip_prefix("send ")
                .or_else(|| trimmed.strip_prefix(consts::CLIENT_SEND_PREFIX))
            {
                let cmd = format!("{}{}\n", consts::CLIENT_SEND_PREFIX, msg);
                if let Err(e) = writer.write_all(cmd.as_bytes()).await {
                    eprintln!("Failed to send: {e}");
                    break;
                }
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

async fn read_server_messages(mut reader: BufReader<tokio::net::tcp::OwnedReadHalf>, shutdown: Arc<AtomicBool>) {
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                println!("\nDisconnected from server.");
                shutdown.store(true, Ordering::SeqCst);
                break;
            }
            Ok(_) => handle_server_message(&line),
            Err(e) => {
                eprintln!("\nRead error: {e}");
                shutdown.store(true, Ordering::SeqCst);
                break;
            }
        }
    }
}

fn read_user_input(cmd_tx: &mpsc::Sender<String>, shutdown: &Arc<AtomicBool>) {
    let Ok(mut rl) = DefaultEditor::new() else {
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

                let _ = rl.add_history_entry(user_input);
                if user_input.eq_ignore_ascii_case(consts::CLIENT_LEAVE_CMD) {
                    let _ = cmd_tx.blocking_send(consts::CLIENT_LEAVE_CMD.to_string());
                    break;
                }
                if cmd_tx.blocking_send(user_input.to_string()).is_err() {
                    break;
                }
            }
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => {
                let _ = cmd_tx.blocking_send(consts::CLIENT_LEAVE_CMD.to_string());
                break;
            }
            Err(_) => break,
        }
    }
}

fn handle_server_message(line: &str) {
    let trimmed = line.trim();
    if sz::find(trimmed, consts::SERVER_BROADCAST_PREFIX) == Some(0) {
        let rest = trimmed.get(consts::SERVER_BROADCAST_PREFIX.len()..).unwrap_or("");
        if let Some(idx) = sz::find(rest, ":") {
            let from = rest.get(..idx).unwrap_or("?");
            let text = rest.get(idx.saturating_add(1)..).unwrap_or("");
            println!("\r[{from}]: {text}");
        } else {
            println!("\r{trimmed}");
        }
    } else if sz::find(trimmed, consts::SERVER_JOINED_PREFIX) == Some(0) {
        let user = trimmed.get(consts::SERVER_JOINED_PREFIX.len()..).unwrap_or("?");
        println!("\r*** {user} joined the chat ***");
    } else if sz::find(trimmed, consts::SERVER_LEFT_PREFIX) == Some(0) {
        let user = trimmed.get(consts::SERVER_LEFT_PREFIX.len()..).unwrap_or("?");
        println!("\r*** {user} left the chat ***");
    } else if trimmed != "OK" {
        println!("\r{trimmed}");
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

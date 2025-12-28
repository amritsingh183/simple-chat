use std::{
    fmt::{Display, Formatter},
    sync::{LazyLock, RwLock, RwLockReadGuard, TryLockError},
    time::Duration,
};

use crossbeam::channel::{Receiver, SendTimeoutError, Sender, TrySendError, bounded};
use jiff::Timestamp;
use thiserror::Error as this_error;
use uuid::Uuid;

const DEFAULT_BUFFER_LENGTH: u16 = u16::MAX;

#[derive(this_error, Debug)]
pub enum Error {
    #[error("room busy, message not sent: {0}")]
    Busy(String),

    #[error("room closed, message not sent: {0}")]
    Closed(String),

    #[error("room buffer full, message not sent: {0}")]
    Full(String),

    #[error("room send timed out, message not sent: {0}")]
    Timeout(String),

    #[error("room lock poisoned, message not sent: {0}")]
    Poisoned(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvError {
    Timeout,
    Disconnected,
}

pub trait MessageReceiver: Send + Sync {
    fn recv_timeout(&self, timeout: Duration) -> Result<String, RecvError>;
}

pub trait MessageQueue: Send + Sync {
    fn send_timeout(&self, msg: String, timeout: Duration) -> Result<(), Error>;
    fn receiver(&self) -> &dyn MessageReceiver;
}

impl MessageReceiver for Receiver<String> {
    fn recv_timeout(&self, timeout: Duration) -> Result<String, RecvError> {
        Self::recv_timeout(self, timeout).map_err(|e| match e {
            crossbeam::channel::RecvTimeoutError::Timeout => RecvError::Timeout,
            crossbeam::channel::RecvTimeoutError::Disconnected => RecvError::Disconnected,
        })
    }
}

static ROOM: LazyLock<Room> = LazyLock::new(|| Room::new(DEFAULT_BUFFER_LENGTH));

pub fn get_room() -> &'static dyn MessageQueue {
    &*ROOM
}

impl From<TrySendError<String>> for Error {
    fn from(err: TrySendError<String>) -> Self {
        match err {
            TrySendError::Full(m) => Self::Full(m),
            TrySendError::Disconnected(m) => Self::Closed(m),
        }
    }
}

impl From<SendTimeoutError<String>> for Error {
    fn from(err: SendTimeoutError<String>) -> Self {
        match err {
            SendTimeoutError::Timeout(m) => Self::Timeout(m),
            SendTimeoutError::Disconnected(m) => Self::Closed(m),
        }
    }
}

#[derive(Debug)]
pub struct Room {
    id: Uuid,
    created_at: Timestamp,

    sender: RwLock<Option<Sender<String>>>,
    receiver: Receiver<String>,
}

impl Display for Room {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Room {{ id: {}, created_at: {} }}", self.id, self.created_at)
    }
}

impl Room {
    fn new(buffer_length: u16) -> Self {
        let effective_buffer_length = { if buffer_length < 1 { 1 } else { buffer_length } };
        let (tx, rx) = bounded(usize::from(effective_buffer_length));
        Self {
            id: Uuid::new_v4(),
            created_at: Timestamp::now(),
            sender: RwLock::new(Some(tx)),
            receiver: rx,
        }
    }

    fn with_sender<F, E>(&self, msg: String, f: F) -> Result<(), Error>
    where
        F: FnOnce(&Sender<String>, String) -> Result<(), E>,
        E: Into<Error>,
    {
        let guard: RwLockReadGuard<'_, Option<Sender<String>>> = self.sender.try_read().map_err(|e| match e {
            TryLockError::WouldBlock => Error::Busy(msg.clone()),
            TryLockError::Poisoned(_) => Error::Poisoned(msg.clone()),
        })?;
        let sender: &Sender<String> = guard.as_ref().ok_or_else(|| Error::Closed(msg.clone()))?;
        let result = f(sender, msg);
        drop(guard);
        result.map_err(Into::into)
    }

    fn send_timeout_impl(&self, msg: String, timeout: Duration) -> Result<(), Error> {
        self.with_sender(msg, |sender, m| sender.send_timeout(m, timeout))
    }
}

impl MessageQueue for Room {
    fn send_timeout(&self, msg: String, timeout: Duration) -> Result<(), Error> {
        self.send_timeout_impl(msg, timeout)
    }

    fn receiver(&self) -> &dyn MessageReceiver {
        &self.receiver
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_room_new_enforces_minimum_buffer() {
        let room = Room::new(0);
        // Should be able to send at least one message (buffer is 1, not 0)
        let result = room.send_timeout("test".to_string(), Duration::from_millis(100));
        assert!(result.is_ok());
    }

    #[test]
    fn test_room_new_with_valid_buffer() {
        let room = Room::new(5);
        // Should be able to send 5 messages without blocking
        for i in 0..5 {
            let result = room.send_timeout(format!("msg{i}"), Duration::from_millis(100));
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_room_send_and_receive() {
        let room = Room::new(10);
        let msg = "hello".to_string();
        room.send_timeout(msg.clone(), Duration::from_millis(100)).unwrap();

        let received = room.receiver().recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(received, msg);
    }

    #[test]
    fn test_room_display() {
        let room = Room::new(1);
        let display = format!("{room}");
        assert!(display.contains("Room"));
        assert!(display.contains("id:"));
        assert!(display.contains("created_at:"));
    }

    #[test]
    fn test_error_from_try_send_full() {
        let err = TrySendError::Full("test_msg".to_string());
        let room_err: Error = err.into();
        assert!(matches!(room_err, Error::Full(m) if m == "test_msg"));
    }

    #[test]
    fn test_error_from_try_send_disconnected() {
        let err = TrySendError::Disconnected("test_msg".to_string());
        let room_err: Error = err.into();
        assert!(matches!(room_err, Error::Closed(m) if m == "test_msg"));
    }

    #[test]
    fn test_error_from_send_timeout_timeout() {
        let err = SendTimeoutError::Timeout("test_msg".to_string());
        let room_err: Error = err.into();
        assert!(matches!(room_err, Error::Timeout(m) if m == "test_msg"));
    }

    #[test]
    fn test_error_from_send_timeout_disconnected() {
        let err = SendTimeoutError::Disconnected("test_msg".to_string());
        let room_err: Error = err.into();
        assert!(matches!(room_err, Error::Closed(m) if m == "test_msg"));
    }

    #[test]
    fn test_error_display() {
        let busy = Error::Busy("msg".to_string());
        assert!(busy.to_string().contains("busy"));

        let closed = Error::Closed("msg".to_string());
        assert!(closed.to_string().contains("closed"));

        let full = Error::Full("msg".to_string());
        assert!(full.to_string().contains("full"));

        let timeout = Error::Timeout("msg".to_string());
        assert!(timeout.to_string().contains("timed out"));
    }
}

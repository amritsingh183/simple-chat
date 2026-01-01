use std::{
    fmt::{Display, Formatter},
    sync::{Arc, LazyLock, RwLock, RwLockReadGuard, TryLockError},
    time::Duration,
};

use crossbeam::channel::{Receiver, SendTimeoutError, Sender, TrySendError, bounded};
use jiff::Timestamp;
use thiserror::Error as this_error;
use uuid::Uuid;

const DEFAULT_BUFFER_LENGTH: u16 = u16::MAX;

#[derive(this_error, Debug)]
pub enum Error {
    #[error("room busy, message not sent")]
    Busy,

    #[error("room closed, message not sent")]
    Closed,

    #[error("room buffer full, message not sent")]
    Full,

    #[error("room send timed out, message not sent")]
    Timeout,

    #[error("room lock poisoned, message not sent")]
    Poisoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvError {
    Timeout,
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneToOne(pub Vec<u8>); // Source of message need not fanout(user->room)

#[derive(Debug, Clone)]
pub struct OneToMany(Arc<Vec<u8>>); // Source of message need fanout(room->to all users)

impl From<Vec<u8>> for OneToOne {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}

impl From<OneToOne> for OneToMany {
    fn from(one: OneToOne) -> Self {
        Self(Arc::new(one.0))
    }
}
impl std::ops::Deref for OneToOne {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::Deref for OneToMany {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub trait MessageReceiver: Send + Sync {
    fn recv_timeout(&self, timeout: Duration) -> Result<OneToMany, RecvError>;
}

impl MessageReceiver for Receiver<OneToMany> {
    fn recv_timeout(&self, timeout: Duration) -> Result<OneToMany, RecvError> {
        Self::recv_timeout(self, timeout).map_err(|e| match e {
            crossbeam::channel::RecvTimeoutError::Timeout => RecvError::Timeout,
            crossbeam::channel::RecvTimeoutError::Disconnected => RecvError::Disconnected,
        })
    }
}
pub trait MessageQueue: Send + Sync {
    fn send_timeout(&self, msg: OneToOne, timeout: Duration) -> Result<(), Error>;
    fn receiver(&self) -> &dyn MessageReceiver;
}
static ROOM: LazyLock<Room> = LazyLock::new(|| Room::new(DEFAULT_BUFFER_LENGTH));

pub fn get_room() -> &'static dyn MessageQueue {
    &*ROOM
}

impl From<TrySendError<OneToMany>> for Error {
    fn from(err: TrySendError<OneToMany>) -> Self {
        match err {
            TrySendError::Full(_) => Self::Full,
            TrySendError::Disconnected(_) => Self::Closed,
        }
    }
}

impl From<SendTimeoutError<OneToMany>> for Error {
    fn from(err: SendTimeoutError<OneToMany>) -> Self {
        match err {
            SendTimeoutError::Timeout(_) => Self::Timeout,
            SendTimeoutError::Disconnected(_) => Self::Closed,
        }
    }
}

#[derive(Debug)]
pub struct Room {
    id: Uuid,
    created_at: Timestamp,

    sender: RwLock<Option<Sender<OneToMany>>>,
    receiver: Receiver<OneToMany>, // we need fanout 1:n
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

    fn with_sender<F, E>(&self, msg: OneToMany, f: F) -> Result<(), Error>
    where
        F: FnOnce(&Sender<OneToMany>, OneToMany) -> Result<(), E>,
        E: Into<Error>,
    {
        let guard: RwLockReadGuard<'_, Option<Sender<OneToMany>>> = self.sender.try_read().map_err(|e| match e {
            TryLockError::WouldBlock => Error::Busy,
            TryLockError::Poisoned(_) => Error::Poisoned,
        })?;
        let sender: &Sender<OneToMany> = guard.as_ref().ok_or(Error::Closed)?;
        let result = f(sender, msg);
        drop(guard);
        result.map_err(Into::into)
    }

    fn send_timeout(&self, msg: OneToMany, timeout: Duration) -> Result<(), Error> {
        self.with_sender(msg, |sender, m| sender.send_timeout(m, timeout))
    }
}

impl MessageQueue for Room {
    // also converts OneToOne->OneToMany
    fn send_timeout(&self, msg: OneToOne, timeout: Duration) -> Result<(), Error> {
        self.send_timeout(msg.into(), timeout)
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
        let result = MessageQueue::send_timeout(&room, OneToOne::from(b"test".to_vec()), Duration::from_millis(100));
        assert!(result.is_ok());
    }

    #[test]
    fn test_room_new_with_valid_buffer() {
        let room = Room::new(5);
        // Should be able to send 5 messages without blocking
        for i in 0..5 {
            let result = MessageQueue::send_timeout(
                &room,
                OneToOne::from(format!("msg{i}").into_bytes()),
                Duration::from_millis(100),
            );
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_room_send_and_receive() {
        let room = Room::new(10);
        let msg = b"hello".to_vec();
        MessageQueue::send_timeout(&room, OneToOne::from(msg.clone()), Duration::from_millis(100)).unwrap();

        let received = room.receiver().recv_timeout(Duration::from_millis(100)).unwrap();
        assert_eq!(&*received, msg.as_slice());
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
        let err = TrySendError::Full(OneToMany::from(OneToOne::from(b"test_msg".to_vec())));
        let room_err: Error = err.into();
        assert!(matches!(room_err, Error::Full));
    }

    #[test]
    fn test_error_from_try_send_disconnected() {
        let err = TrySendError::Disconnected(OneToMany::from(OneToOne::from(b"test_msg".to_vec())));
        let room_err: Error = err.into();
        assert!(matches!(room_err, Error::Closed));
    }

    #[test]
    fn test_error_from_send_timeout_timeout() {
        let err = SendTimeoutError::Timeout(OneToMany::from(OneToOne::from(b"test_msg".to_vec())));
        let room_err: Error = err.into();
        assert!(matches!(room_err, Error::Timeout));
    }

    #[test]
    fn test_error_from_send_timeout_disconnected() {
        let err = SendTimeoutError::Disconnected(OneToMany::from(OneToOne::from(b"test_msg".to_vec())));
        let room_err: Error = err.into();
        assert!(matches!(room_err, Error::Closed));
    }

    #[test]
    fn test_error_display() {
        let busy = Error::Busy;
        assert!(busy.to_string().contains("busy"));

        let closed = Error::Closed;
        assert!(closed.to_string().contains("closed"));

        let full = Error::Full;
        assert!(full.to_string().contains("full"));

        let timeout = Error::Timeout;
        assert!(timeout.to_string().contains("timed out"));
    }
}

use std::{
    collections::{HashMap, hash_map::Entry},
    fmt::{Display, Formatter},
    sync::LazyLock,
    time::Duration,
};

use futures::stream::{self, StreamExt};
use parking_lot::RwLock;
use stringzilla::sz;
use thiserror::Error as this_error;
use tokio::sync::mpsc::Sender;

use super::string::{self as my_string, ValidationResult};
use crate::chat::room;

const SEND_TIMEOUT: Duration = Duration::from_millis(100);
const LOCK_TIMEOUT: Duration = Duration::from_millis(50);
const CONCURRENT_LIMIT: usize = 1024;

static REGISTRY: LazyLock<UserRegistry> = LazyLock::new(UserRegistry::new);

pub fn get_registry() -> &'static UserRegistry {
    &REGISTRY
}

#[derive(Debug, Clone, this_error, PartialEq, Eq)]
pub enum Error {
    #[error("username cannot be empty")]
    UsernameEmpty,

    #[error("username too long (max 32 chars)")]
    UsernameTooLong,

    #[error("username must be alphanumeric")]
    UsernameNotAlphanumeric,

    #[error("username '{0}' is already taken")]
    UsernameTaken(String),

    #[error("registry lock timeout")]
    LockTimeout,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Username(String);

impl Username {
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let s = s.into();
        let trimmed = s.trim();

        match my_string::validate_username(trimmed) {
            ValidationResult::Valid => Ok(Self(trimmed.to_string())),
            ValidationResult::Empty => Err(Error::UsernameEmpty),
            ValidationResult::TooLong => Err(Error::UsernameTooLong),
            ValidationResult::InvalidChars => Err(Error::UsernameNotAlphanumeric),
        }
    }
}

impl Display for Username {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct User {
    username: Username,
    tx: Sender<room::OneToMany>,
}
impl Display for User {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.username)
    }
}
impl User {
    const fn new(username: Username, tx: Sender<room::OneToMany>) -> Self {
        Self { username, tx }
    }
    pub fn get_username(&self) -> Username {
        self.username.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NormalizedKey(String);

impl NormalizedKey {
    fn from_username(username: &Username) -> Self {
        Self(my_string::to_lowercase(&username.0))
    }
}

#[derive(Debug)]
pub struct UserRegistry {
    users: RwLock<HashMap<NormalizedKey, User, sz::BuildSzHasher>>,
}

impl UserRegistry {
    pub fn new() -> Self {
        Self {
            users: RwLock::new(HashMap::with_hasher(sz::BuildSzHasher::default())),
        }
    }

    pub fn register(&self, username: &Username, tx: Sender<room::OneToMany>) -> Result<User, Error> {
        match self
            .users
            .try_write_for(LOCK_TIMEOUT)
            .ok_or(Error::LockTimeout)?
            .entry(NormalizedKey::from_username(username))
        {
            Entry::Occupied(_) => Err(Error::UsernameTaken(username.to_string())),
            Entry::Vacant(e) => {
                let registered_user = e.insert(User::new(username.clone(), tx));
                Ok(registered_user.clone())
            }
        }
    }

    pub fn unregister(&self, user: &User) -> Result<bool, Error> {
        Ok(self
            .users
            .try_write_for(LOCK_TIMEOUT)
            .ok_or(Error::LockTimeout)?
            .remove(&NormalizedKey::from_username(&user.get_username()))
            .is_some())
    }

    pub async fn broadcast(&self, message: &room::OneToMany, exclude: Option<&Username>) -> Result<usize, Error> {
        let senders: Vec<_> = {
            let guard = self.users.try_read_for(LOCK_TIMEOUT).ok_or(Error::LockTimeout)?;
            guard
                .values()
                .filter(|user| exclude != Some(&user.username))
                .map(|user| user.tx.clone())
                .collect()
        };

        // Stream with bounded concurrency - max CONCURRENT_LIMIT in-flight
        let sent_count = stream::iter(senders)
            .map(|tx| {
                let msg = message.clone();
                async move {
                    tokio::time::timeout(SEND_TIMEOUT, tx.send(msg))
                        .await
                        .is_ok_and(|r| r.is_ok())
                }
            })
            .buffer_unordered(CONCURRENT_LIMIT)
            .filter(|&success| async move { success })
            .count()
            .await;

        Ok(sent_count)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;

    #[test]
    fn test_username_valid() {
        assert!(Username::new("alice").is_ok());
        assert!(Username::new("user_123").is_ok());
        assert!(Username::new("你好").is_ok());
    }

    #[test]
    fn test_username_trims_whitespace() {
        let username = Username::new("  alice  ").unwrap();
        assert_eq!(username.to_string(), "alice");
    }

    #[test]
    fn test_username_empty() {
        assert_eq!(Username::new("").unwrap_err(), Error::UsernameEmpty);
        assert_eq!(Username::new("   ").unwrap_err(), Error::UsernameEmpty);
    }

    #[test]
    fn test_username_too_long() {
        let long_name = "a".repeat(33);
        assert_eq!(Username::new(&long_name).unwrap_err(), Error::UsernameTooLong);
    }

    #[test]
    fn test_username_invalid_chars() {
        assert_eq!(Username::new("user@name").unwrap_err(), Error::UsernameNotAlphanumeric);
        assert_eq!(Username::new("hello!").unwrap_err(), Error::UsernameNotAlphanumeric);
    }

    #[test]
    fn test_username_display() {
        let username = Username::new("alice").unwrap();
        assert_eq!(format!("{username}"), "alice");
    }

    #[test]
    fn test_registry_register_success() {
        let registry = UserRegistry::new();
        let (tx, _rx) = mpsc::channel(256);
        let username = Username::new("alice").unwrap();

        let result = registry.register(&username, tx);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().get_username(), username);
    }

    #[test]
    fn test_registry_duplicate_detection() {
        let registry = UserRegistry::new();
        let (tx1, _rx1) = mpsc::channel(256);
        let (tx2, _rx2) = mpsc::channel(256);
        let username = Username::new("bob").unwrap();

        assert!(registry.register(&username, tx1).is_ok());
        let err = registry.register(&username, tx2).unwrap_err();
        assert_eq!(err, Error::UsernameTaken("bob".to_string()));
    }

    #[test]
    fn test_registry_case_insensitive_duplicate() {
        let registry = UserRegistry::new();
        let (tx1, _rx1) = mpsc::channel(256);
        let (tx2, _rx2) = mpsc::channel(256);

        let alice_lower = Username::new("alice").unwrap();
        let alice_upper = Username::new("ALICE").unwrap();

        assert!(registry.register(&alice_lower, tx1).is_ok());
        let err = registry.register(&alice_upper, tx2).unwrap_err();
        assert_eq!(err, Error::UsernameTaken("ALICE".to_string()));
    }

    #[test]
    fn test_registry_unregister() {
        let registry = UserRegistry::new();
        let (tx, _rx) = mpsc::channel(256);
        let username = Username::new("charlie").unwrap();

        let user = registry.register(&username, tx).unwrap();
        assert!(registry.unregister(&user).unwrap());

        assert!(!registry.unregister(&user).unwrap());
    }

    #[test]
    fn test_registry_reregister_after_unregister() {
        let registry = UserRegistry::new();
        let (tx1, _rx1) = mpsc::channel(256);
        let (tx2, _rx2) = mpsc::channel(256);
        let username = Username::new("dave").unwrap();

        let user = registry.register(&username, tx1).unwrap();
        assert!(registry.unregister(&user).unwrap());

        assert!(registry.register(&username, tx2).is_ok());
    }

    #[test]
    fn test_user_display() {
        let (tx, _rx) = mpsc::channel(256);
        let username = Username::new("echo").unwrap();
        let user = User::new(username, tx);
        assert_eq!(format!("{user}"), "echo");
    }
}

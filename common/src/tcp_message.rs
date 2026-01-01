//! Wire protocol encoder/decoder using pipe-delimited format.
//!
//! Format: `EVENT_TYPE|FIELD1|FIELD2|...`
//!
//! Fixed positions:
//! - 1st: `EVENT_TYPE`
//! - 2nd: reason (error), username (join/left/broadcast)
//! - 3rd: message (broadcast only)

use stringzilla::sz;
use thiserror::Error;

use crate::consts;

/// Separator for wire protocol fields
pub const FIELD_SEPARATOR: &str = "|";

/// Wire protocol encode trait
pub trait WireEncode {
    /// Encode to wire format bytes
    fn encode(&self) -> Vec<u8>;
}

/// Wire protocol decode trait
pub trait WireDecode: Sized {
    /// Associated error type for decode failures
    type Error;

    /// Decode from wire format bytes
    ///
    /// # Errors
    ///
    /// Returns an error if the bytes cannot be parsed as a valid message.
    fn decode(bytes: &[u8]) -> Result<Self, Self::Error>;
}

/// Server message types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMessage {
    /// Acknowledgment
    Ok,
    /// Error response with reason
    Err { reason: String },
    /// User joined notification
    UserJoined { username: String },
    /// User left notification
    UserLeft { username: String },
    /// Broadcast message from a user
    Broadcast { username: String, message: String },
}

/// Parse error for server messages
#[derive(Debug, Clone, Error)]
pub enum ServerParseError {
    #[error("empty message")]
    Empty,
    #[error("invalid utf-8")]
    InvalidUtf8,
    #[error("unknown event type: {0}")]
    UnknownEventType(String),
    #[error("missing field: {0}")]
    MissingField(&'static str),
}

impl WireEncode for ServerMessage {
    fn encode(&self) -> Vec<u8> {
        let s = match self {
            Self::Ok => consts::SERVER_EVENT_OK.to_string(),
            Self::Err { reason } => [consts::SERVER_EVENT_ERR, reason].join(FIELD_SEPARATOR),
            Self::UserJoined { username } => [consts::SERVER_EVENT_USER_JOINED, username].join(FIELD_SEPARATOR),
            Self::UserLeft { username } => [consts::SERVER_EVENT_USER_LEFT, username].join(FIELD_SEPARATOR),
            Self::Broadcast { username, message } => {
                [consts::SERVER_EVENT_BROADCAST, username, message].join(FIELD_SEPARATOR)
            }
        };
        s.into_bytes()
    }
}

impl WireDecode for ServerMessage {
    type Error = ServerParseError;

    fn decode(bytes: &[u8]) -> Result<Self, Self::Error> {
        let s = std::str::from_utf8(bytes).map_err(|_| ServerParseError::InvalidUtf8)?;
        let trimmed = s.trim();

        if trimmed.is_empty() {
            return Err(ServerParseError::Empty);
        }

        // Find first separator
        let (event_type, rest) = match sz::find(trimmed, FIELD_SEPARATOR) {
            Some(idx) => (
                trimmed.get(..idx).ok_or(ServerParseError::Empty)?,
                trimmed.get(idx.saturating_add(1)..),
            ),
            None => (trimmed, None),
        };

        match event_type.to_uppercase().as_str() {
            consts::SERVER_EVENT_OK => Ok(Self::Ok),
            consts::SERVER_EVENT_ERR => {
                let reason = rest.ok_or(ServerParseError::MissingField("reason"))?.to_string();
                Ok(Self::Err { reason })
            }
            consts::SERVER_EVENT_USER_JOINED => {
                let username = rest.ok_or(ServerParseError::MissingField("username"))?.to_string();
                Ok(Self::UserJoined { username })
            }
            consts::SERVER_EVENT_USER_LEFT => {
                let username = rest.ok_or(ServerParseError::MissingField("username"))?.to_string();
                Ok(Self::UserLeft { username })
            }
            consts::SERVER_EVENT_BROADCAST => {
                let rest = rest.ok_or(ServerParseError::MissingField("username"))?;
                // Find second separator for message
                let (username, message) = match sz::find(rest, FIELD_SEPARATOR) {
                    Some(idx) => (
                        rest.get(..idx).ok_or(ServerParseError::MissingField("username"))?,
                        rest.get(idx.saturating_add(1)..).unwrap_or(""),
                    ),
                    None => (rest, ""),
                };
                Ok(Self::Broadcast {
                    username: username.to_string(),
                    message: message.to_string(),
                })
            }
            _ => Err(ServerParseError::UnknownEventType(event_type.to_string())),
        }
    }
}

impl std::fmt::Display for ServerMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes = self.encode();
        let s = String::from_utf8_lossy(&bytes);
        write!(f, "{s}")
    }
}

/// Client command types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMessage {
    /// Join with username
    Join { username: String },
    /// Send a message
    Send { message: String },
    /// Leave the chat
    Leave,
}

/// Parse error for client messages
#[derive(Debug, Clone, Error)]
pub enum ClientParseError {
    #[error("empty message")]
    Empty,
    #[error("invalid utf-8")]
    InvalidUtf8,
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    #[error("missing field: {0}")]
    MissingField(&'static str),
}

impl WireEncode for ClientMessage {
    fn encode(&self) -> Vec<u8> {
        let s = match self {
            Self::Join { username } => [consts::CLIENT_JOIN_CMD, username].join(FIELD_SEPARATOR),
            Self::Send { message } => [consts::CLIENT_SEND_CMD, message].join(FIELD_SEPARATOR),
            Self::Leave => consts::CLIENT_LEAVE_CMD.to_string(),
        };
        s.into_bytes()
    }
}

impl WireDecode for ClientMessage {
    type Error = ClientParseError;

    fn decode(bytes: &[u8]) -> Result<Self, Self::Error> {
        let s = std::str::from_utf8(bytes).map_err(|_| ClientParseError::InvalidUtf8)?;
        let trimmed = s.trim();

        if trimmed.is_empty() {
            return Err(ClientParseError::Empty);
        }

        // Find first separator
        let (command, rest) = match sz::find(trimmed, FIELD_SEPARATOR) {
            Some(idx) => (
                trimmed.get(..idx).ok_or(ClientParseError::Empty)?,
                trimmed.get(idx.saturating_add(1)..),
            ),
            None => (trimmed, None),
        };

        match command.to_uppercase().as_str() {
            consts::CLIENT_JOIN_CMD => {
                let username = rest.ok_or(ClientParseError::MissingField("username"))?.to_string();
                if username.is_empty() {
                    return Err(ClientParseError::MissingField("username"));
                }
                Ok(Self::Join { username })
            }
            consts::CLIENT_SEND_CMD => {
                let message = rest.ok_or(ClientParseError::MissingField("message"))?.to_string();
                if message.is_empty() {
                    return Err(ClientParseError::MissingField("message"));
                }
                Ok(Self::Send { message })
            }
            consts::CLIENT_LEAVE_CMD => Ok(Self::Leave),
            _ => Err(ClientParseError::UnknownCommand(command.to_string())),
        }
    }
}

impl std::fmt::Display for ClientMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes = self.encode();
        let s = String::from_utf8_lossy(&bytes);
        write!(f, "{s}")
    }
}
#[allow(clippy::unwrap_used)]
#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use super::*;

    // Server message tests
    #[test]
    fn test_server_ok_encode() {
        let msg = ServerMessage::Ok;
        assert_eq!(msg.encode(), b"OK");
    }

    #[test]
    fn test_server_ok_decode() {
        let msg = ServerMessage::decode(b"OK").expect("should decode");
        assert_eq!(msg, ServerMessage::Ok);
    }

    #[test]
    fn test_server_error_encode() {
        let msg = ServerMessage::Err {
            reason: "username taken".to_string(),
        };
        assert_eq!(msg.encode(), b"ERR|username taken");
    }

    #[test]
    fn test_server_error_decode() {
        let msg = ServerMessage::decode(b"ERR|username taken").expect("should decode");
        assert_eq!(
            msg,
            ServerMessage::Err {
                reason: "username taken".to_string()
            }
        );
    }

    #[test]
    fn test_server_user_joined_encode() {
        let msg = ServerMessage::UserJoined {
            username: "alice".to_string(),
        };
        assert_eq!(msg.encode(), b"JOINED|alice");
    }

    #[test]
    fn test_server_user_joined_decode() {
        let msg = ServerMessage::decode(b"JOINED|alice").expect("should decode");
        assert_eq!(
            msg,
            ServerMessage::UserJoined {
                username: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_server_user_left_encode() {
        let msg = ServerMessage::UserLeft {
            username: "bob".to_string(),
        };
        assert_eq!(msg.encode(), b"LEFT|bob");
    }

    #[test]
    fn test_server_user_left_decode() {
        let msg = ServerMessage::decode(b"LEFT|bob").expect("should decode");
        assert_eq!(
            msg,
            ServerMessage::UserLeft {
                username: "bob".to_string()
            }
        );
    }

    #[test]
    fn test_server_broadcast_encode() {
        let msg = ServerMessage::Broadcast {
            username: "alex".to_string(),
            message: "hello world".to_string(),
        };
        assert_eq!(msg.encode(), b"BROADCAST|alex|hello world");
    }

    #[test]
    fn test_server_broadcast_decode() {
        let msg = ServerMessage::decode(b"BROADCAST|alex|hello world").expect("should decode");
        assert_eq!(
            msg,
            ServerMessage::Broadcast {
                username: "alex".to_string(),
                message: "hello world".to_string()
            }
        );
    }

    #[test]
    fn test_server_broadcast_with_pipes_in_message() {
        // Pipes in message content should be preserved (everything after second | is message)
        let msg = ServerMessage::decode(b"BROADCAST|alex|hello|world|test").expect("should decode");
        assert_eq!(
            msg,
            ServerMessage::Broadcast {
                username: "alex".to_string(),
                message: "hello|world|test".to_string()
            }
        );
    }

    #[test]
    fn test_server_decode_case_insensitive() {
        let msg = ServerMessage::decode(b"joined|alice").expect("should decode");
        assert_eq!(
            msg,
            ServerMessage::UserJoined {
                username: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_server_decode_empty() {
        let result = ServerMessage::decode(b"");
        assert!(result.is_err());
    }

    #[test]
    fn test_server_decode_unknown() {
        let result = ServerMessage::decode(b"UNKNOWN|test");
        assert!(result.is_err());
    }

    // Client message tests
    #[test]
    fn test_client_join_encode() {
        let msg = ClientMessage::Join {
            username: "alice".to_string(),
        };
        assert_eq!(msg.encode(), b"JOIN|alice");
    }

    #[test]
    fn test_client_join_decode() {
        let msg = ClientMessage::decode(b"JOIN|alice").expect("should decode");
        assert_eq!(
            msg,
            ClientMessage::Join {
                username: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_client_send_encode() {
        let msg = ClientMessage::Send {
            message: "hello world".to_string(),
        };
        assert_eq!(msg.encode(), b"SEND|hello world");
    }

    #[test]
    fn test_client_send_decode() {
        let msg = ClientMessage::decode(b"SEND|hello world").expect("should decode");
        assert_eq!(
            msg,
            ClientMessage::Send {
                message: "hello world".to_string()
            }
        );
    }

    #[test]
    fn test_client_leave_encode() {
        let msg = ClientMessage::Leave;
        assert_eq!(msg.encode(), b"LEAVE");
    }

    #[test]
    fn test_client_leave_decode() {
        let msg = ClientMessage::decode(b"LEAVE").expect("should decode");
        assert_eq!(msg, ClientMessage::Leave);
    }

    #[test]
    fn test_client_decode_case_insensitive() {
        let msg = ClientMessage::decode(b"join|alice").expect("should decode");
        assert_eq!(
            msg,
            ClientMessage::Join {
                username: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_client_decode_whitespace() {
        let msg = ClientMessage::decode(b"  JOIN|alice  \n").expect("should decode");
        assert_eq!(
            msg,
            ClientMessage::Join {
                username: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_roundtrip_server_broadcast() {
        let original = ServerMessage::Broadcast {
            username: "test".to_string(),
            message: "hello".to_string(),
        };
        let encoded = original.encode();
        let decoded = ServerMessage::decode(&encoded).expect("should roundtrip");
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_roundtrip_client_send() {
        let original = ClientMessage::Send {
            message: "test message".to_string(),
        };
        let encoded = original.encode();
        let decoded = ClientMessage::decode(&encoded).expect("should roundtrip");
        assert_eq!(original, decoded);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod special_char_tests {
    use super::*;

    #[test]
    fn test_semicolon_in_message() {
        let msg = ClientMessage::Send {
            message: "ola;".to_string(),
        };
        let encoded = msg.encode();
        println!("Encoded: {:?}", String::from_utf8_lossy(&encoded));
        assert_eq!(&encoded, b"SEND|ola;");

        let decoded = ClientMessage::decode(&encoded).expect("should decode");
        assert_eq!(decoded, msg);
    }
}

#[test]
fn test_sz_find_with_semicolon() {
    use stringzilla::sz;
    let input = "SEND|ola;";
    let idx = sz::find(input, "|");
    println!("sz::find result for 'SEND|ola;': {idx:?}");
    assert_eq!(idx, Some(4));

    let rest = input.get(5..);
    println!("rest after pipe: {rest:?}");
    assert_eq!(rest, Some("ola;"));
}

#[test]
fn test_decode_with_semicolon_full() {
    let input = b"SEND|ola;";
    let result = ClientMessage::decode(input);
    println!("Decode result: {result:?}");
    assert!(result.is_ok());
}

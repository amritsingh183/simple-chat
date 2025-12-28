use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use common::consts::{self, SERVER_OK_PREFIX};
use stringzilla::sz;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum ParseError {
    #[error("empty message")]
    Empty,

    #[error("unknown command: {0}")]
    UnknownCommand(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMessage {
    Ok,

    Error { reason: String },

    UserJoined { username: String },

    UserLeft { username: String },

    BroadcastMessage { text: String },
}

impl FromStr for ServerMessage {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(ParseError::Empty);
        }

        let (command, rest) = match sz::find(trimmed, " ") {
            Some(idx) => (
                trimmed.get(..idx).ok_or(ParseError::Empty)?,
                trimmed.get(idx.saturating_add(1)..).map(str::trim),
            ),
            None => (trimmed, None),
        };

        match command.to_uppercase().as_str() {
            consts::SERVER_OK_CMD => Ok(Self::Ok),
            consts::SERVER_ERR_CMD => Ok(Self::Error {
                reason: rest.unwrap_or("unknown error").to_string(),
            }),
            consts::SERVER_JOINED_CMD => Ok(Self::UserJoined {
                username: rest.unwrap_or("").to_string(),
            }),
            consts::SERVER_LEFT_CMD => Ok(Self::UserLeft {
                username: rest.unwrap_or("").to_string(),
            }),
            _ => Err(ParseError::UnknownCommand(command.to_string())),
        }
    }
}

impl Display for ServerMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "{SERVER_OK_PREFIX}"),
            Self::Error { reason } => write!(f, "{}{reason}", consts::SERVER_ERR_PREFIX),
            Self::UserJoined { username } => write!(f, "{}{username}", consts::SERVER_JOINED_PREFIX),
            Self::UserLeft { username } => write!(f, "{}{username}", consts::SERVER_LEFT_PREFIX),
            Self::BroadcastMessage { text } => write!(f, "{}{text}", consts::SERVER_BROADCAST_PREFIX),
        }
    }
}

#[cfg(test)]
mod tests {
    use common::consts;

    use super::*;

    #[test]
    fn test_ok_message_format_for_client() {
        let msg = ServerMessage::Ok;
        let output = msg.to_string();

        assert_eq!(output, "OK");
    }

    #[test]
    fn test_err_message_format_for_client() {
        let msg = ServerMessage::Error {
            reason: "username taken".to_string(),
        };
        let output = msg.to_string();

        assert!(output.starts_with(consts::SERVER_ERR_PREFIX.trim()));
        assert_eq!(output, "ERR username taken");
    }

    #[test]
    fn test_joined_message_format_for_client() {
        let msg = ServerMessage::UserJoined {
            username: "alice".to_string(),
        };
        let output = msg.to_string();

        let username = output.strip_prefix(consts::SERVER_JOINED_PREFIX);
        assert!(username.is_some(), "output should start with JOINED prefix");
        assert_eq!(username.unwrap_or(""), "alice");
    }

    #[test]
    fn test_left_message_format_for_client() {
        let msg = ServerMessage::UserLeft {
            username: "bob".to_string(),
        };
        let output = msg.to_string();

        let username = output.strip_prefix(consts::SERVER_LEFT_PREFIX);
        assert!(username.is_some(), "output should start with LEFT prefix");
        assert_eq!(username.unwrap_or(""), "bob");
    }

    #[test]
    fn test_broadcast_message_format_for_client() {
        let msg = ServerMessage::BroadcastMessage {
            text: "charlie:Hello everyone!".to_string(),
        };
        let output = msg.to_string();

        let rest = output.strip_prefix(consts::SERVER_BROADCAST_PREFIX);
        assert!(rest.is_some(), "output should start with BROADCAST prefix");
        let rest = rest.unwrap_or("");

        assert!(rest.contains(':'), "broadcast message should contain colon");
        if let Some((from, text)) = rest.split_once(':') {
            assert_eq!(from, "charlie");
            assert_eq!(text, "Hello everyone!");
        }
    }

    #[test]
    fn test_broadcast_empty_message_for_client() {
        let msg = ServerMessage::BroadcastMessage {
            text: "dave:".to_string(),
        };
        let output = msg.to_string();
        let rest = output.strip_prefix(consts::SERVER_BROADCAST_PREFIX);
        assert!(rest.is_some(), "output should start with BROADCAST prefix");
        let rest = rest.unwrap_or("");
        assert!(rest.contains(':'), "broadcast message should contain colon");
        if let Some((from, text)) = rest.split_once(':') {
            assert_eq!(from, "dave");
            assert_eq!(text, "");
        }
    }

    #[test]
    fn test_broadcast_no_colon_fallback_for_client() {
        let msg = ServerMessage::BroadcastMessage {
            text: "system notification".to_string(),
        };
        let output = msg.to_string();

        assert!(output.starts_with(consts::SERVER_BROADCAST_PREFIX));
        assert_eq!(output, "BROADCAST system notification");
    }
}

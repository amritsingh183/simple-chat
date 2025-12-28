use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use common::consts;
use stringzilla::sz;

#[derive(Debug, Clone)]
pub enum ParseError {
    Empty,
    UnknownCommand(String),
    MissingUsername,
    MissingMessage,
}

impl std::error::Error for ParseError {}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty message"),
            Self::UnknownCommand(cmd) => write!(f, "unknown command: {cmd}"),
            Self::MissingUsername => {
                write!(f, "missing username for {} command", consts::CLIENT_JOIN_CMD)
            }
            Self::MissingMessage => {
                write!(f, "missing message for {} command", consts::CLIENT_SEND_CMD)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCommand {
    Join { username: String },

    Send { message: String },

    Leave,
}

impl FromStr for ClientCommand {
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
            consts::CLIENT_JOIN_CMD => {
                let username = rest.ok_or(ParseError::MissingUsername)?.to_string();
                if username.is_empty() {
                    return Err(ParseError::MissingUsername);
                }
                Ok(Self::Join { username })
            }
            consts::CLIENT_SEND_CMD => {
                let message = rest.ok_or(ParseError::MissingMessage)?.to_string();
                if message.is_empty() {
                    return Err(ParseError::MissingMessage);
                }
                Ok(Self::Send { message })
            }
            consts::CLIENT_LEAVE_CMD => Ok(Self::Leave),
            _ => Err(ParseError::UnknownCommand(command.to_string())),
        }
    }
}

impl Display for ClientCommand {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Join { username } => write!(f, "{}{username}", consts::CLIENT_JOIN_PREFIX),
            Self::Send { message } => write!(f, "{}{message}", consts::CLIENT_SEND_PREFIX),
            Self::Leave => write!(f, "{}", consts::CLIENT_LEAVE_PREFIX),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_client_join_command_format() {
        let input = format!("{}{}", consts::CLIENT_JOIN_PREFIX, "alice");
        let parsed: ClientCommand = input.parse().expect("should parse JOIN command");
        assert_eq!(
            parsed,
            ClientCommand::Join {
                username: "alice".to_string()
            }
        );
    }

    #[test]
    fn test_client_send_command_format() {
        let input = format!("{}{}", consts::CLIENT_SEND_PREFIX, "hello world");
        let parsed: ClientCommand = input.parse().expect("should parse SEND command");
        assert_eq!(
            parsed,
            ClientCommand::Send {
                message: "hello world".to_string()
            }
        );
    }

    #[test]
    fn test_client_leave_command_format() {
        let input = consts::CLIENT_LEAVE_PREFIX.to_string();
        let parsed: ClientCommand = input.parse().expect("should parse LEAVE command");
        assert_eq!(parsed, ClientCommand::Leave);
    }

    #[test]
    fn test_display_roundtrip_join() {
        let original = ClientCommand::Join {
            username: "bob".to_string(),
        };
        let formatted = original.to_string();
        let parsed: ClientCommand = formatted.parse().expect("roundtrip should work");
        assert_eq!(parsed, original);
    }

    #[test]
    fn test_display_roundtrip_send() {
        let original = ClientCommand::Send {
            message: "test message".to_string(),
        };
        let formatted = original.to_string();
        let parsed: ClientCommand = formatted.parse().expect("roundtrip should work");
        assert_eq!(parsed, original);
    }

    #[test]
    fn test_display_roundtrip_leave() {
        let original = ClientCommand::Leave;
        let formatted = original.to_string();
        let parsed: ClientCommand = formatted.parse().expect("roundtrip should work");
        assert_eq!(parsed, original);
    }

    #[test]
    fn test_case_insensitive_join() {
        let lower: ClientCommand = "join testuser".parse().expect("lowercase should work");
        let upper: ClientCommand = "JOIN testuser".parse().expect("uppercase should work");
        let mixed: ClientCommand = "Join testuser".parse().expect("mixed case should work");
        assert_eq!(lower, upper);
        assert_eq!(upper, mixed);
    }

    #[test]
    fn test_case_insensitive_leave() {
        let lower: ClientCommand = "leave".parse().expect("lowercase should work");
        let upper: ClientCommand = "LEAVE".parse().expect("uppercase should work");
        assert_eq!(lower, upper);
        assert_eq!(lower, ClientCommand::Leave);
    }

    #[test]
    fn test_whitespace_trimming() {
        let with_newline: ClientCommand = "JOIN alice\n".parse().expect("should handle newline");
        let with_spaces: ClientCommand = "  JOIN alice  ".parse().expect("should handle spaces");
        assert_eq!(with_newline, with_spaces);
    }
}

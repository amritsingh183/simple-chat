//! Security utilities for the chat application.
//!
//! Provides functions for sanitizing user input before logging,
//! and security-related constants.

use std::time::Duration;

/// Maximum allowed message length in bytes.
pub const MAX_MESSAGE_LENGTH: usize = 4096;

/// Maximum allowed line length when reading from socket.
pub const MAX_LINE_LENGTH: usize = 4096;

/// Read timeout for socket operations.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum concurrent connections the server will accept.
pub const MAX_CONNECTIONS: usize = 10_000;

/// Rate limit: maximum messages per second per user.
pub const MAX_MESSAGES_PER_SECOND: u32 = 10;

/// Rate limit: burst capacity for message rate limiting.
pub const MESSAGE_BURST_CAPACITY: u32 = 20;

/// Sanitizes a string for safe logging by escaping control characters.
///
/// This prevents log injection attacks where malicious input could:
/// - Forge log entries by injecting newlines
/// - Corrupt log files with control characters
/// - Bypass log analysis tools
///
/// # Examples
///
/// ```
/// use common::security::sanitize_for_log;
///
/// assert_eq!(sanitize_for_log("normal"), "normal");
/// assert_eq!(sanitize_for_log("line1\nline2"), "line1\\nline2");
/// assert_eq!(sanitize_for_log("with\r\nCRLF"), "with\\r\\nCRLF");
/// ```
#[must_use]
pub fn sanitize_for_log(s: &str) -> String {
    use std::fmt::Write;

    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\0' => result.push_str("\\0"),
            // Escape other control characters as hex
            c if c.is_control() => {
                for byte in c.to_string().bytes() {
                    // Use write! to avoid extra allocation from format!
                    let _ = write!(result, "\\x{byte:02x}");
                }
            }
            c => result.push(c),
        }
    }
    result
}

/// Truncates a string to a maximum length, appending "..." if truncated.
///
/// Useful for logging potentially large user input without filling logs.
#[must_use]
pub fn truncate_for_log(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{truncated}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_normal_string() {
        assert_eq!(sanitize_for_log("hello world"), "hello world");
        assert_eq!(sanitize_for_log("user123"), "user123");
        assert_eq!(sanitize_for_log(""), "");
    }

    #[test]
    fn test_sanitize_newlines() {
        assert_eq!(sanitize_for_log("line1\nline2"), "line1\\nline2");
        assert_eq!(sanitize_for_log("\n"), "\\n");
        assert_eq!(sanitize_for_log("a\nb\nc"), "a\\nb\\nc");
    }

    #[test]
    fn test_sanitize_carriage_return() {
        assert_eq!(sanitize_for_log("line1\rline2"), "line1\\rline2");
        assert_eq!(sanitize_for_log("with\r\nCRLF"), "with\\r\\nCRLF");
    }

    #[test]
    fn test_sanitize_tabs() {
        assert_eq!(sanitize_for_log("col1\tcol2"), "col1\\tcol2");
    }

    #[test]
    fn test_sanitize_null() {
        assert_eq!(sanitize_for_log("before\0after"), "before\\0after");
    }

    #[test]
    fn test_sanitize_unicode() {
        // Unicode characters should pass through unchanged
        assert_eq!(sanitize_for_log("你好"), "你好");
        assert_eq!(sanitize_for_log("émoji 🎉"), "émoji 🎉");
    }

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate_for_log("hello", 10), "hello");
        assert_eq!(truncate_for_log("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        assert_eq!(truncate_for_log("hello world", 8), "hello...");
        assert_eq!(truncate_for_log("abcdefghij", 6), "abc...");
    }
}

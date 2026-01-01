//! Security utilities for the chat application.
//!
//! Provides functions for sanitizing user input before logging,
//! and security-related constants.

/// Sanitizes a string for safe logging by escaping control characters.
///
/// This prevents log injection attacks where malicious input could:
/// - Forge log entries by injecting newlines
/// - Corrupt log files with control characters
/// - Bypass log analysis tools
///
/// ANSI escape sequences (for terminal colors) are preserved.
///
/// # Examples
///
/// ```
/// use common::security::sanitize_for_log;
///
/// assert_eq!(sanitize_for_log("normal"), "normal");
/// assert_eq!(sanitize_for_log("line1\nline2"), "line1\\nline2");
/// // ANSI colors are preserved
/// assert_eq!(sanitize_for_log("\x1b[32mgreen\x1b[0m"), "\x1b[32mgreen\x1b[0m");
/// ```
#[must_use]
pub fn sanitize_for_log(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // Check for ANSI escape sequence start
            '\x1b' => {
                result.push(c);
                // If followed by '[', it's an ANSI CSI sequence - pass it through
                if chars.peek() == Some(&'[') {
                    if let Some(bracket) = chars.next() {
                        result.push(bracket);
                    }
                    // Consume until we hit the terminating letter (@ through ~)
                    loop {
                        match chars.peek() {
                            Some(&next) if next.is_ascii_alphabetic() || next == '~' || next == '@' => {
                                if let Some(term) = chars.next() {
                                    result.push(term);
                                }
                                break;
                            }
                            Some(_) => {
                                if let Some(ch) = chars.next() {
                                    result.push(ch);
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\0' => result.push_str("\\0"),
            // Escape other control characters as hex (but not ANSI which we handled above)
            c if c.is_control() => {
                use std::fmt::Write;
                for byte in c.to_string().bytes() {
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

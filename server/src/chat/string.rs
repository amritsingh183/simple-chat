use stringzilla::{stringzilla as sz_core, sz};

pub const MAX_USERNAME_LEN: usize = 32;

const ASCII_VALID_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";

#[inline]
#[cfg(test)]
pub fn is_blank(s: &str) -> bool {
    s.trim().is_empty()
}

#[inline]
pub fn is_valid_username_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[inline]
pub fn is_ascii_valid_fast(s: &str) -> bool {
    sz::find_byte_not_from(s, ASCII_VALID_CHARS).is_none()
}

#[inline]
pub fn is_valid_username_str(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    if s.is_ascii() {
        return is_ascii_valid_fast(s);
    }

    s.chars().all(is_valid_username_char)
}

#[inline]
pub fn is_too_long(s: &str) -> bool {
    s.chars().count() > MAX_USERNAME_LEN
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationResult {
    Valid,

    Empty,

    TooLong,

    InvalidChars,
}

pub fn validate_username(s: &str) -> ValidationResult {
    let trimmed = s.trim();

    if trimmed.is_empty() {
        return ValidationResult::Empty;
    }

    if is_too_long(trimmed) {
        return ValidationResult::TooLong;
    }

    if !is_valid_username_str(trimmed) {
        return ValidationResult::InvalidChars;
    }

    ValidationResult::Valid
}

#[cfg(test)]
pub fn validated_username(s: &str) -> Result<&str, ValidationResult> {
    let trimmed = s.trim();
    match validate_username(trimmed) {
        ValidationResult::Valid => Ok(trimmed),
        err => Err(err),
    }
}

const CASE_FOLD_BUFFER_SIZE: usize = MAX_USERNAME_LEN * 3 * 4;

pub fn case_fold(s: &str) -> Option<String> {
    if s.is_empty() {
        return Some(String::new());
    }

    let mut buffer = [0u8; CASE_FOLD_BUFFER_SIZE];

    let len = sz_core::utf8_case_fold(s, &mut buffer);

    String::from_utf8(buffer.get(..len)?.to_vec()).ok()
}

#[cfg(test)]
pub fn usernames_equal_ignore_case(a: &str, b: &str) -> bool {
    match (case_fold(a), case_fold(b)) {
        (Some(folded_a), Some(folded_b)) => folded_a == folded_b,
        _ => false,
    }
}

#[inline]
pub fn to_lowercase(s: &str) -> String {
    case_fold(s).unwrap_or_else(|| s.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_blank() {
        assert!(is_blank(""));
        assert!(is_blank("   "));
        assert!(is_blank("\t\n"));
        assert!(!is_blank("a"));
        assert!(!is_blank(" a "));
    }

    #[test]
    fn test_ascii_valid_fast() {
        assert!(is_ascii_valid_fast("john"));
        assert!(is_ascii_valid_fast("john_doe"));
        assert!(is_ascii_valid_fast("User123"));
        assert!(is_ascii_valid_fast("ABC_123_xyz"));

        assert!(!is_ascii_valid_fast("john doe"));
        assert!(!is_ascii_valid_fast("user@name"));
        assert!(!is_ascii_valid_fast("hello!"));
        assert!(!is_ascii_valid_fast("test#123"));
    }

    #[test]
    fn test_valid_username_chars() {
        assert!(is_valid_username_char('a'));
        assert!(is_valid_username_char('Z'));
        assert!(is_valid_username_char('5'));
        assert!(is_valid_username_char('_'));

        assert!(is_valid_username_char('你'));
        assert!(is_valid_username_char('好'));
        assert!(is_valid_username_char('अ'));
        assert!(is_valid_username_char('ਅ'));
        assert!(is_valid_username_char('α'));
        assert!(is_valid_username_char('ñ'));

        assert!(!is_valid_username_char(' '));
        assert!(!is_valid_username_char('@'));
        assert!(!is_valid_username_char('#'));
        assert!(!is_valid_username_char('!'));
        assert!(!is_valid_username_char('\n'));
        assert!(!is_valid_username_char('\0'));
    }

    #[test]
    fn test_validate_username_valid() {
        assert_eq!(validate_username("john"), ValidationResult::Valid);
        assert_eq!(validate_username("john_doe"), ValidationResult::Valid);
        assert_eq!(validate_username("user123"), ValidationResult::Valid);

        assert_eq!(validate_username("你好"), ValidationResult::Valid);
        assert_eq!(validate_username("अमृत"), ValidationResult::Valid);
        assert_eq!(validate_username("ਸਿੰਘ"), ValidationResult::Valid);
        assert_eq!(validate_username("Ελληνικά"), ValidationResult::Valid);
        assert_eq!(validate_username("日本語"), ValidationResult::Valid);
    }

    #[test]
    fn test_validate_username_empty() {
        assert_eq!(validate_username(""), ValidationResult::Empty);
        assert_eq!(validate_username("   "), ValidationResult::Empty);
        assert_eq!(validate_username("\t"), ValidationResult::Empty);
    }

    #[test]
    fn test_validate_username_too_long() {
        let long_name = "a".repeat(33);
        assert_eq!(validate_username(&long_name), ValidationResult::TooLong);

        let max_name = "a".repeat(32);
        assert_eq!(validate_username(&max_name), ValidationResult::Valid);
    }

    #[test]
    fn test_validate_username_invalid_chars() {
        assert_eq!(validate_username("john doe"), ValidationResult::InvalidChars);
        assert_eq!(validate_username("user@name"), ValidationResult::InvalidChars);
        assert_eq!(validate_username("hello!"), ValidationResult::InvalidChars);
        assert_eq!(validate_username("test#123"), ValidationResult::InvalidChars);

        assert_eq!(validate_username("你好 世界"), ValidationResult::InvalidChars);
        assert_eq!(validate_username("你好@世界"), ValidationResult::InvalidChars);
        assert_eq!(validate_username("你好!"), ValidationResult::InvalidChars);

        assert_eq!(validate_username("ਸਿੰਘ ਜੀ"), ValidationResult::InvalidChars);
        assert_eq!(validate_username("ਅਮ੍ਰਿਤ@"), ValidationResult::InvalidChars);
        assert_eq!(validate_username("ਪੰਜਾਬੀ#"), ValidationResult::InvalidChars);

        assert_eq!(validate_username("Привет мир"), ValidationResult::InvalidChars);
        assert_eq!(validate_username("Иван@почта"), ValidationResult::InvalidChars);
        assert_eq!(validate_username("Москва!"), ValidationResult::InvalidChars);

        assert_eq!(validate_username("नमस्ते दुनिया"), ValidationResult::InvalidChars);
        assert_eq!(validate_username("अमृत@सिंह"), ValidationResult::InvalidChars);
        assert_eq!(validate_username("भारत#"), ValidationResult::InvalidChars);
    }

    #[test]
    fn test_validated_username() {
        assert_eq!(validated_username("  john_doe  "), Ok("john_doe"));
        assert_eq!(validated_username("你好"), Ok("你好"));
        assert!(validated_username("").is_err());
        assert!(validated_username("john@doe").is_err());
    }

    #[test]
    fn test_case_fold() {
        assert_eq!(case_fold("HELLO"), Some("hello".to_string()));
        assert_eq!(case_fold("John"), Some("john".to_string()));
        assert_eq!(case_fold("USER_123"), Some("user_123".to_string()));

        assert_eq!(case_fold("Straße"), Some("strasse".to_string()));

        assert_eq!(case_fold("ΑΒΓΔ"), Some("αβγδ".to_string()));

        assert_eq!(case_fold("你好"), Some("你好".to_string()));

        assert_eq!(case_fold("अमृत"), Some("अमृत".to_string()));

        assert_eq!(case_fold(""), Some(String::new()));
    }

    #[test]
    fn test_usernames_equal_ignore_case() {
        assert!(usernames_equal_ignore_case("John", "john"));
        assert!(usernames_equal_ignore_case("ALICE", "alice"));
        assert!(usernames_equal_ignore_case("User_123", "USER_123"));

        assert!(usernames_equal_ignore_case("Straße", "STRASSE"));
        assert!(usernames_equal_ignore_case("straße", "Strasse"));

        assert!(!usernames_equal_ignore_case("alice", "bob"));
        assert!(!usernames_equal_ignore_case("你好", "再见"));

        assert!(usernames_equal_ignore_case("你好", "你好"));
        assert!(usernames_equal_ignore_case("ਸਿੰਘ", "ਸਿੰਘ"));
    }
}

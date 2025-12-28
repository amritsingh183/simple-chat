use stringzilla::sz;

use crate::chat::user::{User, Username};

#[derive(Debug, Clone)]
// ChatMessage alive as long as User
pub struct ChatMessage<'a> {
    pub sender: &'a User,
    pub content: String,
}

impl<'a> ChatMessage<'a> {
    pub const fn new(sender: &'a User, content: String) -> Self {
        Self { sender, content }
    }

    // channel ingress
    pub fn serialize(&self) -> String {
        format!("{}:{}", self.sender.get_username(), self.content)
    }

    // channel egress
    pub fn deserialize(s: &str) -> Option<(Username, String)> {
        let idx = sz::find(s, ":")?;
        let sender_str = s.get(..idx)?;
        let sender = Username::new(sender_str).ok()?;
        let content = s.get(idx.saturating_add(1)..)?.to_string();
        Some((sender, content))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crossbeam::channel;

    use super::*;
    use crate::chat::user::UserRegistry;

    fn create_test_registry_and_user(name: &str) -> (UserRegistry, crate::chat::user::User) {
        let registry = UserRegistry::new();
        let (tx, _rx) = channel::unbounded();
        let username = Username::new(name).unwrap();
        let user = registry.register(&username, tx).unwrap();
        (registry, user)
    }

    #[test]
    fn test_chat_message_new() {
        let (_registry, user) = create_test_registry_and_user("alice");
        let message = ChatMessage::new(&user, "hello world".to_string());

        assert_eq!(message.content, "hello world");
        assert_eq!(message.sender.get_username().to_string(), "alice");
    }

    #[test]
    fn test_chat_message_serialize() {
        let (_registry, user) = create_test_registry_and_user("bob");
        let message = ChatMessage::new(&user, "test message".to_string());

        assert_eq!(message.serialize(), "bob:test message");
    }

    #[test]
    fn test_chat_message_serialize_empty_content() {
        let (_registry, user) = create_test_registry_and_user("charlie");
        let message = ChatMessage::new(&user, String::new());

        assert_eq!(message.serialize(), "charlie:");
    }

    #[test]
    fn test_chat_message_serialize_with_colons_in_content() {
        let (_registry, user) = create_test_registry_and_user("dave");
        let message = ChatMessage::new(&user, "time: 12:30:00".to_string());

        assert_eq!(message.serialize(), "dave:time: 12:30:00");
    }

    #[test]
    fn test_chat_message_deserialize_valid() {
        let result = ChatMessage::deserialize("alice:hello world");

        assert!(result.is_some());
        let (username, content) = result.unwrap();
        assert_eq!(username.to_string(), "alice");
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_chat_message_deserialize_empty_content() {
        let result = ChatMessage::deserialize("bob:");

        assert!(result.is_some());
        let (username, content) = result.unwrap();
        assert_eq!(username.to_string(), "bob");
        assert_eq!(content, "");
    }

    #[test]
    fn test_chat_message_deserialize_colons_in_content() {
        let result = ChatMessage::deserialize("charlie:time: 12:30:00");

        assert!(result.is_some());
        let (username, content) = result.unwrap();
        assert_eq!(username.to_string(), "charlie");
        assert_eq!(content, "time: 12:30:00");
    }

    #[test]
    fn test_chat_message_deserialize_no_colon() {
        let result = ChatMessage::deserialize("invalid message");
        assert!(result.is_none());
    }

    #[test]
    fn test_chat_message_deserialize_empty_username() {
        let result = ChatMessage::deserialize(":message");
        assert!(result.is_none());
    }

    #[test]
    fn test_chat_message_deserialize_invalid_username() {
        let result = ChatMessage::deserialize("user@invalid:message");
        assert!(result.is_none());
    }

    #[test]
    fn test_chat_message_deserialize_empty_string() {
        let result = ChatMessage::deserialize("");
        assert!(result.is_none());
    }

    #[test]
    fn test_chat_message_clone() {
        let (_registry, user) = create_test_registry_and_user("echo");
        let message = ChatMessage::new(&user, "clone test".to_string());
        let cloned = message.clone();

        assert_eq!(cloned.content, message.content);
        assert_eq!(
            cloned.sender.get_username().to_string(),
            message.sender.get_username().to_string()
        );
    }
}

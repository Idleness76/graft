#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_construction() {
        let msg = Message {
            role: "user".to_string(),
            content: "hello".to_string(),
        };
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "hello");
    }

    #[test]
    fn test_message_cloning() {
        let msg1 = Message {
            role: "system".to_string(),
            content: "foo".to_string(),
        };
        let msg2 = msg1.clone();
        assert_eq!(msg1, msg2);
        // Changing the clone does not affect the original
        let mut msg2 = msg2;
        msg2.content = "bar".to_string();
        assert_ne!(msg1, msg2);
    }

    #[test]
    fn test_message_equality() {
        let m1 = Message {
            role: "user".to_string(),
            content: "hi".to_string(),
        };
        let m2 = Message {
            role: "user".to_string(),
            content: "hi".to_string(),
        };
        let m3 = Message {
            role: "assistant".to_string(),
            content: "hi".to_string(),
        };
        let m4 = Message {
            role: "user".to_string(),
            content: "bye".to_string(),
        };
        assert_eq!(m1, m2);
        assert_ne!(m1, m3);
        assert_ne!(m1, m4);
    }
}

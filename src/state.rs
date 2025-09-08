use rustc_hash::FxHashMap;
use serde_json::Value;

use crate::{
    channels::{Channel, ExtrasChannel, MessagesChannel},
    message::Message,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Versioned<T> {
    pub value: T,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionedState {
    pub messages: MessagesChannel,
    pub extra: ExtrasChannel,
}

#[derive(Clone, Debug)]
pub struct StateSnapshot {
    pub messages: Vec<Message>,
    pub messages_version: u32,
    pub extra: FxHashMap<String, Value>,
    pub extra_version: u32,
}

impl VersionedState {
    pub fn new_with_user_message(user_text: &str) -> Self {
        let messages = vec![Message {
            role: "user".into(),
            content: user_text.into(),
        }];
        Self {
            messages: MessagesChannel::new(messages, 1),
            extra: ExtrasChannel::default(),
        }
    }

    pub fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            messages: self.messages.snapshot(),
            messages_version: self.messages.version(),
            extra: self.extra.snapshot(),
            extra_version: self.extra.version(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_with_user_message_initializes_fields() {
        let s = VersionedState::new_with_user_message("hello");
        let snap = s.snapshot();
        assert_eq!(snap.messages.len(), 1);
        assert_eq!(snap.messages[0].role, "user");
        assert_eq!(snap.messages[0].content, "hello");
        assert_eq!(snap.messages_version, 1);
        assert!(snap.extra.is_empty());
        assert_eq!(snap.extra_version, 1);
    }

    #[test]
    fn test_snapshot_is_deep_copy() {
        let mut s = VersionedState::new_with_user_message("x");
        let snap = s.snapshot();
        // mutate original
        s.messages.get_mut()[0].content = "changed".into();
        s.extra
            .get_mut()
            .insert("k".into(), Value::String("v".into()));
        // snapshot should be unaffected
        assert_eq!(snap.messages[0].content, "x");
        assert!(snap.extra.get("k").is_none());
    }

    #[test]
    fn test_extra_flexible_types() {
        use serde_json::json;
        let mut s = VersionedState::new_with_user_message("y");
        s.extra.get_mut().insert("number".into(), json!(123));
        s.extra.get_mut().insert("text".into(), json!("abc"));
        s.extra.get_mut().insert("array".into(), json!([1, 2, 3]));
        let snap = s.snapshot();
        assert_eq!(snap.extra["number"], json!(123));
        assert_eq!(snap.extra["text"], json!("abc"));
        assert_eq!(snap.extra["array"], json!([1, 2, 3]));
    }

    #[test]
    fn test_clone_is_deep() {
        let mut s = VersionedState::new_with_user_message("msg");
        s.extra
            .get_mut()
            .insert("k1".into(), Value::String("v1".into()));
        let cloned = s.clone();
        // mutate original
        s.messages.get_mut()[0].content = "changed".into();
        s.extra
            .get_mut()
            .insert("k2".into(), Value::String("v2".into()));
        // cloned should differ now
        assert_ne!(cloned.messages.snapshot(), s.messages.snapshot());
        assert_ne!(cloned.extra.snapshot(), s.extra.snapshot());
        // original initial data remains in clone
        assert_eq!(cloned.messages.snapshot()[0].content, "msg");
        assert_eq!(
            cloned.extra.snapshot().get("k1"),
            Some(&Value::String("v1".into()))
        );
        assert!(cloned.extra.snapshot().get("k2").is_none());
    }
}

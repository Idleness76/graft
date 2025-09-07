use crate::message::*;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Versioned<T> {
    pub value: T,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionedState {
    pub messages: Versioned<Vec<Message>>,
    pub outputs: Versioned<Vec<String>>,
    pub meta: Versioned<HashMap<String, String>>,
}

impl VersionedState {
    pub fn new_with_user_message(user_text: &str) -> Self {
        Self {
            messages: Versioned {
                value: vec![Message {
                    role: "user".into(),
                    content: user_text.into(),
                }],
                version: 1,
            },
            outputs: Versioned {
                value: Vec::new(),
                version: 1,
            },
            meta: Versioned {
                value: HashMap::new(),
                version: 1,
            },
        }
    }

    pub fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            messages: self.messages.value.clone(),
            messages_version: self.messages.version,
            outputs: self.outputs.value.clone(),
            outputs_version: self.outputs.version,
            meta: self.meta.value.clone(),
            meta_version: self.meta.version,
        }
    }
}

#[derive(Clone, Debug)]
pub struct StateSnapshot {
    pub messages: Vec<Message>,
    pub messages_version: u64,
    pub outputs: Vec<String>,
    pub outputs_version: u64,
    pub meta: HashMap<String, String>,
    pub meta_version: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    /// Verifies that VersionedState::new_with_user_message initializes all fields correctly with a user message.
    fn test_new_with_user_message() {
        let user_text = "Hello, world!";
        let state = VersionedState::new_with_user_message(user_text);
        assert_eq!(state.messages.value.len(), 1);
        let msg = &state.messages.value[0];
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, user_text);
        assert_eq!(state.messages.version, 1);
        assert!(state.outputs.value.is_empty());
        assert_eq!(state.outputs.version, 1);
        assert!(state.meta.value.is_empty());
        assert_eq!(state.meta.version, 1);
    }

    #[test]
    /// Checks that snapshot() produces a StateSnapshot matching the current state values and versions.
    fn test_snapshot_matches_state() {
        let user_text = "Test message";
        let mut state = VersionedState::new_with_user_message(user_text);
        state.outputs.value.push("output1".to_string());
        state.outputs.version = 2;
        state
            .meta
            .value
            .insert("key".to_string(), "value".to_string());
        state.meta.version = 3;
        let snap = state.snapshot();
        assert_eq!(snap.messages, state.messages.value);
        assert_eq!(snap.messages_version, state.messages.version);
        assert_eq!(snap.outputs, state.outputs.value);
        assert_eq!(snap.outputs_version, state.outputs.version);
        assert_eq!(snap.meta, state.meta.value);
        assert_eq!(snap.meta_version, state.meta.version);
    }

    #[test]
    /// Ensures that StateSnapshot is a deep copy and remains unchanged when the original state is mutated after snapshotting.
    fn test_snapshot_is_deep_copy() {
        let mut state = VersionedState::new_with_user_message("msg");
        let snap = state.snapshot();
        // Mutate state after snapshot
        state.messages.value[0].content = "changed".to_string();
        state.outputs.value.push("new_output".to_string());
        state
            .meta
            .value
            .insert("foo".to_string(), "bar".to_string());
        // Snapshot should remain unchanged
        assert_eq!(snap.messages[0].content, "msg");
        assert!(snap.outputs.is_empty());
        assert!(snap.meta.is_empty());
    }

    #[test]
    /// Validates that the Versioned struct stores values and versions correctly for different types.
    fn test_versioned_struct() {
        let v = Versioned {
            value: vec![1, 2, 3],
            version: 42,
        };
        assert_eq!(v.value, vec![1, 2, 3]);
        assert_eq!(v.version, 42);
        let v2 = Versioned {
            value: "abc".to_string(),
            version: 7,
        };
        assert_eq!(v2.value, "abc");
        assert_eq!(v2.version, 7);
        let mut map = HashMap::new();
        map.insert("k".to_string(), "v".to_string());
        let v3 = Versioned {
            value: map.clone(),
            version: 99,
        };
        assert_eq!(v3.value, map);
        assert_eq!(v3.version, 99);
    }

    #[test]
    /// Checks that cloning VersionedState produces a deep copy, and mutations to the original do not affect the clone.
    fn test_cloning_versioned_state() {
        let mut state = VersionedState::new_with_user_message("clone me");
        state.outputs.value.push("out".to_string());
        state.meta.value.insert("x".to_string(), "y".to_string());
        let cloned = state.clone();
        assert_eq!(cloned.messages.value, state.messages.value);
        assert_eq!(cloned.outputs.value, state.outputs.value);
        assert_eq!(cloned.meta.value, state.meta.value);
        // Mutate original, cloned should not change
        state.messages.value[0].content = "changed".to_string();
        state.outputs.value.push("new".to_string());
        state
            .meta
            .value
            .insert("foo".to_string(), "bar".to_string());
        assert_ne!(cloned.messages.value, state.messages.value);
        assert_ne!(cloned.outputs.value, state.outputs.value);
        assert_ne!(cloned.meta.value, state.meta.value);
    }
}

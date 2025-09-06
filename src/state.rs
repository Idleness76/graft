use crate::message::*;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Versioned<T> {
    pub value: T,
    pub version: u64,
}

#[derive(Clone, Debug)]
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

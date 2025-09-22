use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Avoid depending on serde for NodeKind by using encoded string form for kind.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorEvent {
    pub when: DateTime<Utc>,
    pub scope: ErrorScope,
    pub error: LadderError,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub context: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum ErrorScope {
    Node { kind: String, step: u64 },
    Scheduler { step: u64 },
    Runner { session: String, step: u64 },
    App,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LadderError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Box<LadderError>>,
    #[serde(default)]
    pub details: serde_json::Value,
}

impl LadderError {
    pub fn msg<M: Into<String>>(m: M) -> Self {
        Self { message: m.into(), cause: None, details: serde_json::Value::Null }
    }
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }
    pub fn with_cause(mut self, cause: LadderError) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
}

pub fn pretty_print(events: &[ErrorEvent]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for (i, e) in events.iter().enumerate() {
        let _ = writeln!(out, "[{}] {} | {:?}", i, e.when, e.scope);
        let _ = writeln!(out, "  error: {}", e.error.message);
        if let Some(cause) = &e.error.cause {
            let _ = writeln!(out, "  cause: {}", cause.message);
        }
        if !e.tags.is_empty() {
            let _ = writeln!(out, "  tags: {:?}", e.tags);
        }
        if !e.context.is_null() {
            let _ = writeln!(out, "  context: {}", e.context);
        }
    }
    out
}

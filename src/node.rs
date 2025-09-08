use crate::message::*;
use crate::state::*;
use async_trait::async_trait;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct NodeContext {
    pub node_id: String,
    pub step: u64,
}

#[derive(Clone, Debug, Default)]
pub struct NodePartial {
    // Per-channel partials (all optional)
    // TODO replace outputs and meta with Extra
    pub messages: Option<Vec<Message>>,
    pub outputs: Option<Vec<String>>,
    pub meta: Option<HashMap<String, String>>,
}

#[async_trait]
pub trait Node: Send + Sync {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> NodePartial;
}

/****************
  Node examples - will be created by users
*****************/

pub struct NodeA;
#[async_trait]
impl Node for NodeA {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> NodePartial {
        let seen_msgs = snapshot.messages.len();
        let content = format!("A saw {} msgs at step {}", seen_msgs, ctx.step);

        // NodeA writes to messages and meta, but not outputs
        let mut meta = HashMap::new();
        meta.insert("source".into(), "A".into());
        meta.insert("hint".into(), "alpha".into());

        NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content,
            }]),
            outputs: None,
            meta: Some(meta),
        }
    }
}

pub struct NodeB;
#[async_trait]
impl Node for NodeB {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> NodePartial {
        let seen_outs = snapshot.outputs.len();
        let content = format!("B adding output, prior outs={}", seen_outs);

        // NodeB writes to outputs and meta, and also a small message
        let mut meta = HashMap::new();
        meta.insert("tag".into(), "beta".into());

        NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: format!("B ran at step {}", ctx.step),
            }]),
            outputs: Some(vec![content]),
            meta: Some(meta),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(step: u64) -> NodeContext {
        NodeContext {
            node_id: "n1".to_string(),
            step,
        }
    }

    #[tokio::test]
    /// Verifies NodeA's behavior when run with an empty StateSnapshot: should produce a message and meta, but no outputs.
    async fn test_node_a_run_empty_snapshot() {
        let node = NodeA;
        let mut extra = HashMap::new();
        extra.insert("test_key".to_string(), serde_json::json!("test_value"));
        let snap = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            outputs: vec![],
            outputs_version: 1,
            meta: HashMap::new(),
            meta_version: 1,
            extra: extra.clone(),
            extra_version: 1,
        };
        let ctx = make_ctx(5);
        let result = node.run(snap.clone(), ctx.clone()).await;
        assert!(result.outputs.is_none());
        assert!(result.messages.is_some());
        let msg = &result.messages.as_ref().unwrap()[0];
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "A saw 0 msgs at step 5");
        assert!(result.meta.is_some());
        let meta = result.meta.as_ref().unwrap();
        assert_eq!(meta.get("source"), Some(&"A".to_string()));
        assert_eq!(meta.get("hint"), Some(&"alpha".to_string()));
        // Assert extra field is preserved in snapshot
        assert_eq!(
            snap.extra.get("test_key"),
            Some(&serde_json::json!("test_value"))
        );
    }

    #[tokio::test]
    /// Checks NodeA's output when run with a non-empty messages list in StateSnapshot.
    async fn test_node_a_run_nonempty_snapshot() {
        let node = NodeA;
        let mut extra = HashMap::new();
        extra.insert("foo".to_string(), serde_json::json!([1, 2, 3]));
        let snap = StateSnapshot {
            messages: vec![Message {
                role: "user".into(),
                content: "hi".into(),
            }],
            messages_version: 2,
            outputs: vec![],
            outputs_version: 1,
            meta: HashMap::new(),
            meta_version: 1,
            extra: extra.clone(),
            extra_version: 1,
        };
        let ctx = make_ctx(7);
        let result = node.run(snap.clone(), ctx.clone()).await;
        let msg = &result.messages.as_ref().unwrap()[0];
        assert_eq!(msg.content, "A saw 1 msgs at step 7");
        // Assert extra field is preserved in snapshot
        assert_eq!(snap.extra.get("foo"), Some(&serde_json::json!([1, 2, 3])));
    }

    #[tokio::test]
    /// Verifies NodeB's behavior with an empty StateSnapshot: should produce a message, output, and meta.
    async fn test_node_b_run_empty_snapshot() {
        let node = NodeB;
        let mut extra = HashMap::new();
        extra.insert("bar".to_string(), serde_json::json!({"baz": true}));
        let snap = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            outputs: vec![],
            outputs_version: 1,
            meta: HashMap::new(),
            meta_version: 1,
            extra: extra.clone(),
            extra_version: 1,
        };
        let ctx = make_ctx(3);
        let result = node.run(snap.clone(), ctx.clone()).await;
        assert!(result.messages.is_some());
        let msg = &result.messages.as_ref().unwrap()[0];
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "B ran at step 3");
        assert!(result.outputs.is_some());
        let out = &result.outputs.as_ref().unwrap()[0];
        assert_eq!(out, "B adding output, prior outs=0");
        assert!(result.meta.is_some());
        let meta = result.meta.as_ref().unwrap();
        assert_eq!(meta.get("tag"), Some(&"beta".to_string()));
        // Assert extra field is preserved in snapshot
        assert_eq!(
            snap.extra.get("bar"),
            Some(&serde_json::json!({"baz": true}))
        );
    }

    #[tokio::test]
    /// Checks NodeB's output when run with a non-empty outputs list in StateSnapshot.
    async fn test_node_b_run_nonempty_snapshot() {
        let node = NodeB;
        let mut extra = HashMap::new();
        extra.insert("arr".to_string(), serde_json::json!(["x", "y"]));
        let snap = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            outputs: vec!["foo".to_string(), "bar".to_string()],
            outputs_version: 2,
            meta: HashMap::new(),
            meta_version: 1,
            extra: extra.clone(),
            extra_version: 1,
        };
        let ctx = make_ctx(8);
        let result = node.run(snap.clone(), ctx.clone()).await;
        let out = &result.outputs.as_ref().unwrap()[0];
        assert_eq!(out, "B adding output, prior outs=2");
        // Assert extra field is preserved in snapshot
        assert_eq!(snap.extra.get("arr"), Some(&serde_json::json!(["x", "y"])));
    }

    #[test]
    /// Verifies that the extra field in StateSnapshot can store and retrieve flexible data types
    /// using serde_json::Value, including numbers, strings, arrays, and objects. This ensures that
    /// the node state model supports arbitrary extension data as required by the project specification.
    fn test_snapshot_extra_flexible_types() {
        use serde_json::json;
        let mut extra = HashMap::new();
        extra.insert("number".to_string(), json!(123));
        extra.insert("text".to_string(), json!("abc"));
        extra.insert("array".to_string(), json!([1, 2, 3]));
        extra.insert("obj".to_string(), json!({"foo": "bar"}));
        let snap = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            outputs: vec![],
            outputs_version: 1,
            meta: HashMap::new(),
            meta_version: 1,
            extra: extra.clone(),
            extra_version: 1,
        };
        assert_eq!(snap.extra["number"], json!(123));
        assert_eq!(snap.extra["text"], json!("abc"));
        assert_eq!(snap.extra["array"], json!([1, 2, 3]));
        assert_eq!(snap.extra["obj"], json!({"foo": "bar"}));
    }

    #[test]
    /// Ensures NodePartial::default() initializes all fields to None.
    fn test_node_partial_default() {
        let np = NodePartial::default();
        assert!(np.messages.is_none());
        assert!(np.outputs.is_none());
        assert!(np.meta.is_none());
    }
}

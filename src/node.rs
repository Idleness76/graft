use crate::message::*;
use crate::state::*;
use async_trait::async_trait;
use rustc_hash::FxHashMap;
use serde_json::json;
use miette::Diagnostic;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct NodeContext {
    pub node_id: String,
    pub step: u64,
}

#[derive(Clone, Debug, Default)]
pub struct NodePartial {
    /// Optional list of new messages to append.
    pub messages: Option<Vec<Message>>,
    /// Optional map of arbitrary extension data (JSON values).
    pub extra: Option<FxHashMap<String, serde_json::Value>>,
}

#[async_trait]
pub trait Node: Send + Sync {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, NodeError>;
}

#[derive(Debug, Error, Diagnostic)]
pub enum NodeError {
    #[error("missing expected input: {what}")]
    #[diagnostic(code(graft::node::missing_input), help("Check that the previous node produced the required data."))]
    MissingInput { what: &'static str },

    #[error("provider error ({provider}): {message}")]
    #[diagnostic(code(graft::node::provider))]
    Provider { provider: &'static str, message: String },

    #[error(transparent)]
    #[diagnostic(code(graft::node::serde_json))]
    Serde(#[from] serde_json::Error),
}

/****************
  Example nodes
*****************/

pub struct NodeA;

#[async_trait]
impl Node for NodeA {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, NodeError> {
        let seen_msgs = snapshot.messages.len();
        let content = format!("A saw {} msgs at step {}", seen_msgs, ctx.step);

        let mut extra = FxHashMap::default();
        extra.insert("source".into(), json!("A"));
        extra.insert("hint".into(), json!("alpha"));
        extra.insert("step".into(), json!(ctx.step));

        Ok(NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content,
            }]),
            extra: Some(extra),
        })
    }
}

pub struct NodeB;

#[async_trait]
impl Node for NodeB {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, NodeError> {
        let content = format!(
            "B ran at step {} (prev msgs={})",
            ctx.step,
            snapshot.messages.len()
        );
        // Write a couple of extra keys to exercise merge & overwrite behavior.
        let mut extra = FxHashMap::default();
        extra.insert("tag".into(), json!("beta"));
        extra.insert("last_step".into(), json!(ctx.step));
        Ok(NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content,
            }]),
            extra: Some(extra),
        })
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
    async fn test_node_a_run_empty_snapshot() {
        let node = NodeA;
        let snap = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            extra: FxHashMap::default(),
            extra_version: 1,
        };
        let ctx = make_ctx(5);
    let result = node.run(snap.clone(), ctx).await.unwrap();
        assert!(result.messages.is_some());
        let msg = &result.messages.as_ref().unwrap()[0];
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "A saw 0 msgs at step 5");
        let ex = result.extra.as_ref().unwrap();
        assert_eq!(ex.get("source").unwrap(), "A");
        assert_eq!(ex.get("hint").unwrap(), "alpha");
        assert_eq!(snap.extra.len(), 0);
    }

    #[tokio::test]
    async fn test_node_a_run_nonempty_snapshot() {
        let node = NodeA;
        let snap = StateSnapshot {
            messages: vec![Message {
                role: "user".into(),
                content: "hi".into(),
            }],
            messages_version: 2,
            extra: FxHashMap::default(),
            extra_version: 1,
        };
        let ctx = make_ctx(7);
    let result = node.run(snap.clone(), ctx).await.unwrap();
        let msg = &result.messages.as_ref().unwrap()[0];
        assert_eq!(msg.content, "A saw 1 msgs at step 7");
    }

    #[tokio::test]
    async fn test_node_b_run_empty_snapshot() {
        let node = NodeB;
        let snap = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            extra: FxHashMap::default(),
            extra_version: 1,
        };
        let ctx = make_ctx(3);
    let result = node.run(snap.clone(), ctx).await.unwrap();
        assert!(result.messages.is_some());
        let msg = &result.messages.as_ref().unwrap()[0];
        assert_eq!(msg.content, "B ran at step 3 (prev msgs=0)");
        let ex = result.extra.as_ref().unwrap();
        assert_eq!(ex.get("tag").unwrap(), "beta");
    }

    #[tokio::test]
    async fn test_node_b_run_nonempty_snapshot() {
        let node = NodeB;
        let snap = StateSnapshot {
            messages: vec![Message {
                role: "user".into(),
                content: "intro".into(),
            }],
            messages_version: 5,
            extra: FxHashMap::default(),
            extra_version: 1,
        };
        let ctx = make_ctx(8);
    let result = node.run(snap.clone(), ctx).await.unwrap();
        let msg = &result.messages.as_ref().unwrap()[0];
        assert_eq!(msg.content, "B ran at step 8 (prev msgs=1)");
    }

    #[test]
    fn test_snapshot_extra_flexible_types() {
        use serde_json::json;
        let mut extra = FxHashMap::default();
        extra.insert("number".into(), json!(123));
        extra.insert("text".into(), json!("abc"));
        extra.insert("array".into(), json!([1, 2, 3]));
        extra.insert("obj".into(), json!({"foo": "bar"}));
        let snap = StateSnapshot {
            messages: vec![],
            messages_version: 1,
            extra: extra.clone(),
            extra_version: 1,
        };
        assert_eq!(snap.extra["number"], json!(123));
        assert_eq!(snap.extra["text"], json!("abc"));
        assert_eq!(snap.extra["array"], json!([1, 2, 3]));
        assert_eq!(snap.extra["obj"], json!({"foo": "bar"}));
    }

    #[test]
    fn test_node_partial_default() {
        let np = NodePartial::default();
        assert!(np.messages.is_none());
        assert!(np.extra.is_none());
    }
}

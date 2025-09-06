use async_trait::async_trait;
use std::collections::HashMap;

/*****************
  Node abstraction
******************/

#[derive(Clone, Debug)]
pub struct NodeContext {
    node_id: String,
    step: u64,
}

#[derive(Clone, Debug, Default)]
pub struct NodePartial {
    // Per-channel partials (all optional)
    messages: Option<Vec<Message>>,
    outputs: Option<Vec<String>>,
    meta: Option<HashMap<String, String>>,
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

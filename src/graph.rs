use std::collections::HashMap;
use std::sync::Arc;

use crate::app::*;
use crate::node::*;
use crate::reducer::*;
use crate::types::*;

pub struct GraphBuilder {
    pub nodes: HashMap<NodeKind, Arc<dyn Node>>,
    pub edges: HashMap<NodeKind, Vec<NodeKind>>,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
        }
    }

    pub fn add_node(mut self, id: NodeKind, node: impl Node + 'static) -> Self {
        self.nodes.insert(id, Arc::new(node));
        self
    }

    pub fn add_edge(mut self, from: NodeKind, to: NodeKind) -> Self {
        self.edges.entry(from).or_default().push(to);
        self
    }

    pub fn compile(self) -> App {
        App {
            nodes: self.nodes,
            edges: self.edges,
            add_messages: Arc::new(AddMessages),
            append_outputs: Arc::new(AppendVec::<String>),
            map_merge: Arc::new(MapMerge),
        }
    }
}

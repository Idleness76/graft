use rustc_hash::FxHashMap;
use std::sync::Arc;

#[derive(Debug)]
pub enum GraphCompileError {
    MissingEntry,
    EntryNotRegistered(NodeKind),
}

use crate::app::*;
use crate::node::*;
use crate::types::*;

pub struct GraphBuilder {
    pub nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
    pub edges: FxHashMap<NodeKind, Vec<NodeKind>>,
    pub entry: Option<NodeKind>,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self {
            nodes: FxHashMap::default(),
            edges: FxHashMap::default(),
            entry: None,
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

    pub fn set_entry(mut self, entry: NodeKind) -> Self {
        self.entry = Some(entry);
        self
    }

    pub fn compile(self) -> Result<App, GraphCompileError> {
        let entry = self.entry.as_ref().ok_or(GraphCompileError::MissingEntry)?;
        if !self.nodes.contains_key(entry) {
            return Err(GraphCompileError::EntryNotRegistered(entry.clone()));
        }
        Ok(App::from_parts(self.nodes, self.edges))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Verifies that a new GraphBuilder is initialized with empty nodes and edges.
    fn test_graph_builder_new() {
        let gb = GraphBuilder::new();
        assert!(gb.nodes.is_empty());
        assert!(gb.edges.is_empty());
    }

    #[test]
    /// Checks that nodes can be added to the GraphBuilder and are stored correctly.
    fn test_add_node() {
        let gb = GraphBuilder::new()
            .add_node(NodeKind::Start, NodeA)
            .add_node(NodeKind::End, NodeB);
        assert_eq!(gb.nodes.len(), 2);
        assert!(gb.nodes.contains_key(&NodeKind::Start));
        assert!(gb.nodes.contains_key(&NodeKind::End));
    }

    #[test]
    /// Ensures edges can be added between nodes and are tracked properly in the builder.
    fn test_add_edge() {
        let gb = GraphBuilder::new()
            .add_edge(NodeKind::Start, NodeKind::End)
            .add_edge(NodeKind::Start, NodeKind::Other("C".to_string()));
        assert_eq!(gb.edges.len(), 1);
        let edges = gb.edges.get(&NodeKind::Start).unwrap();
        assert_eq!(edges.len(), 2);
        assert!(edges.contains(&NodeKind::End));
        assert!(edges.contains(&NodeKind::Other("C".to_string())));
    }

    #[test]
    /// Validates that compiling a GraphBuilder produces an App with correct nodes, edges, and reducer references.
    fn test_compile() {
        let gb = GraphBuilder::new()
            .add_node(NodeKind::Start, NodeA)
            .add_node(NodeKind::End, NodeB)
            .add_edge(NodeKind::Start, NodeKind::End)
            .set_entry(NodeKind::Start);
        let app = gb.compile().unwrap();
        assert_eq!(app.nodes().len(), 2);
        assert!(app.nodes().contains_key(&NodeKind::Start));
        assert!(app.nodes().contains_key(&NodeKind::End));
        assert_eq!(app.edges().len(), 1);
        assert!(
            app.edges()
                .get(&NodeKind::Start)
                .unwrap()
                .contains(&NodeKind::End)
        );
    }

    #[test]
    /// Compiling without setting entry should return MissingEntry error.
    fn test_compile_missing_entry() {
        let gb = GraphBuilder::new()
            .add_node(NodeKind::Start, NodeA)
            .add_node(NodeKind::End, NodeB)
            .add_edge(NodeKind::Start, NodeKind::End);
        let result = gb.compile();
        match result {
            Err(GraphCompileError::MissingEntry) => (),
            _ => panic!("Expected MissingEntry error"),
        }
    }

    #[test]
    /// Compiling with entry set to a node that is not registered should return EntryNotRegistered error.
    fn test_compile_entry_not_registered() {
        let gb = GraphBuilder::new()
            .add_node(NodeKind::Start, NodeA)
            .add_node(NodeKind::End, NodeB)
            .add_edge(NodeKind::Start, NodeKind::End)
            .set_entry(NodeKind::Other("NotRegistered".to_string()));
        let result = gb.compile();
        match result {
            Err(GraphCompileError::EntryNotRegistered(NodeKind::Other(ref s)))
                if s == "NotRegistered" =>
            {
                ()
            }
            _ => panic!("Expected EntryNotRegistered error for 'NotRegistered'"),
        }
    }

    #[test]
    /// Tests equality and inequality for NodeKind::Other variant with different string values.
    fn test_nodekind_other_variant() {
        let k1 = NodeKind::Other("foo".to_string());
        let k2 = NodeKind::Other("foo".to_string());
        let k3 = NodeKind::Other("bar".to_string());
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    /// Checks that duplicate edges between the same nodes are allowed and counted correctly.
    fn test_duplicate_edges() {
        let gb = GraphBuilder::new()
            .add_edge(NodeKind::Start, NodeKind::End)
            .add_edge(NodeKind::Start, NodeKind::End);
        let edges = gb.edges.get(&NodeKind::Start).unwrap();
        // Both edges should be present (duplicates allowed)
        let count = edges.iter().filter(|k| **k == NodeKind::End).count();
        assert_eq!(count, 2);
    }
}

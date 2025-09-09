use crate::node::{Node, NodeContext, NodePartial};
use crate::state::StateSnapshot;
use crate::types::NodeKind;
use futures::stream::{self, StreamExt};
use rustc_hash::FxHashMap;
use std::sync::Arc;

/// Result of running a superstep over the frontier.
#[derive(Debug, Clone)]
pub struct StepRunResult {
    /// Node IDs (as strings) that were executed this step, in the order they were scheduled.
    pub ran_nodes: Vec<String>,
    /// Node IDs (as strings) that were skipped this step (End nodes or no new versions seen).
    pub skipped_nodes: Vec<String>,
    /// Outputs from nodes that ran: (node_id_string, NodePartial)
    pub outputs: Vec<(String, NodePartial)>,
}

/// Frontier scheduler with version gating and bounded concurrency.
#[derive(Debug, Default)]
pub struct Scheduler {
    pub concurrency_limit: usize,
    /// versions_seen[node_id][channel_name] = last version observed when the node ran
    pub versions_seen: FxHashMap<String, FxHashMap<String, u64>>,
}

impl Scheduler {
    pub fn new(concurrency_limit: usize) -> Self {
        Self {
            concurrency_limit: if concurrency_limit == 0 {
                1
            } else {
                concurrency_limit
            },
            versions_seen: FxHashMap::default(),
        }
    }

    /// Helper to expose channel versions as generic (name, version) pairs.
    #[inline]
    fn channel_versions(snap: &StateSnapshot) -> [(&'static str, u64); 2] {
        [
            ("messages", snap.messages_version as u64),
            ("extra", snap.extra_version as u64),
        ]
    }

    /// Decide if a node should run given the pre-barrier snapshot.
    /// Returns true if any channel version increased since this node last ran.
    pub fn should_run(&self, node_id: &str, snap: &StateSnapshot) -> bool {
        let channels = Self::channel_versions(snap);
        self.should_run_with(node_id, &channels)
    }

    /// Generic form of should_run: decide based on provided (channel_name, version) pairs.
    pub fn should_run_with(&self, node_id: &str, channels: &[(&str, u64)]) -> bool {
        let seen = match self.versions_seen.get(node_id) {
            Some(v) => v,
            None => return true, // never ran -> run
        };
        for (name, ver) in channels.iter() {
            let last = seen.get::<str>(name).copied().unwrap_or(0);
            if *ver > last {
                return true;
            }
        }
        false
    }

    /// Record the versions seen for a node at the start of its execution (pre-barrier snapshot).
    pub fn record_seen(&mut self, node_id: &str, snap: &StateSnapshot) {
        let channels = Self::channel_versions(snap);
        self.record_seen_with(node_id, &channels);
    }

    /// Generic form of record_seen: store versions for provided (channel_name, version) pairs.
    pub fn record_seen_with(&mut self, node_id: &str, channels: &[(&str, u64)]) {
        let entry = self
            .versions_seen
            .entry(node_id.to_string())
            .or_insert_with(FxHashMap::default);
        for (name, ver) in channels.iter() {
            entry.insert((*name).to_string(), *ver);
        }
    }

    /// Run one superstep over a frontier with bounded concurrency. End nodes are skipped.
    pub async fn superstep(
        &mut self,
        nodes: &FxHashMap<NodeKind, Arc<dyn Node>>, // registry
        frontier: Vec<NodeKind>,                    // frontier for this step
        snap: StateSnapshot,                        // pre-barrier snapshot
        step: u64,
    ) -> StepRunResult {
        // Partition frontier into to_run vs skipped using a skip predicate and version gating.
        let channels = Self::channel_versions(&snap);
        let skip_predicate = |k: &NodeKind| matches!(k, NodeKind::End);
        let mut to_run: Vec<NodeKind> = Vec::new();
        let mut skipped_kinds: Vec<NodeKind> = Vec::new();
        for k in frontier.into_iter() {
            if skip_predicate(&k) {
                skipped_kinds.push(k);
                continue;
            }
            let id_str = format!("{:?}", k);
            if self.should_run_with(&id_str, &channels) {
                to_run.push(k);
            } else {
                skipped_kinds.push(k);
            }
        }

        // Build tasks for the nodes to run.
        let to_run_ids: Vec<String> = to_run.iter().map(|k| format!("{:?}", k)).collect();
        let tasks = to_run_ids.iter().zip(to_run.iter()).map(|(id_str, kind)| {
            let node = nodes.get(kind).expect("node in frontier not found").clone();
            let ctx = NodeContext {
                node_id: id_str.clone(),
                step,
            };
            let s = snap.clone();
            let id = id_str.clone();
            async move {
                let out = node.run(s, ctx).await;
                (id, out)
            }
        });

        // Execute with bounded concurrency; completion order may differ.
        let outputs: Vec<(String, NodePartial)> = stream::iter(tasks)
            .buffer_unordered(self.concurrency_limit)
            .collect()
            .await;

        // Record versions seen for nodes that ran.
        for id in &to_run_ids {
            self.record_seen_with(id, &channels);
        }

        let skipped_nodes = skipped_kinds
            .into_iter()
            .map(|k| format!("{:?}", k))
            .collect();

        StepRunResult {
            ran_nodes: to_run_ids,
            skipped_nodes,
            outputs,
        }
    }
}

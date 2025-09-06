use futures::future::join_all;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::message::*;
use crate::node::*;
use crate::reducer::*;
use crate::state::*;
use crate::types::*;

pub struct App {
    pub nodes: HashMap<NodeKind, Arc<dyn Node>>,
    pub edges: HashMap<NodeKind, Vec<NodeKind>>,
    // Reducers per channel
    pub add_messages: Arc<AddMessages>,
    pub append_outputs: Arc<AppendVec<String>>,
    pub map_merge: Arc<MapMerge>,
}

impl App {
    pub async fn invoke(&self, initial_state: VersionedState) -> Result<VersionedState> {
        let state = Arc::new(RwLock::new(initial_state));
        let mut step: u64 = 0;

        // Run A and B in parallel from START, both go to END
        let mut frontier: Vec<NodeKind> = self
            .edges
            .get(&NodeKind::Start)
            .cloned()
            .unwrap_or_default();
        if frontier.is_empty() {
            return Err("No nodes to run from START".into());
        }

        println!("== Begin run ==");

        loop {
            step += 1;

            // Stop if only END remains
            let only_end = frontier.iter().all(|n| *n == NodeKind::End);
            if only_end {
                println!("Reached END at step {}", step);
                break;
            }

            // Snapshot
            let snapshot = { state.read().await.snapshot() };
            println!(
                "\n-- Superstep {} --\nmsgs={} v{}; outs={} v{}; meta_keys={} v{}",
                step,
                snapshot.messages.len(),
                snapshot.messages_version,
                snapshot.outputs.len(),
                snapshot.outputs_version,
                snapshot.meta.len(),
                snapshot.meta_version
            );

            // Execute frontier (parallel)
            let mut run_ids: Vec<NodeKind> = Vec::new();
            let mut futs = Vec::new();
            for node_id in frontier.iter() {
                if *node_id == NodeKind::End {
                    continue;
                }
                let id = node_id.clone();
                run_ids.push(id.clone());
                let node = self.nodes.get(&id).unwrap().clone();
                let ctx = NodeContext {
                    node_id: format!("{:?}", id),
                    step,
                };
                let snap = snapshot.clone();
                futs.push(async move { node.run(snap, ctx).await });
            }

            let node_partials: Vec<NodePartial> = join_all(futs).await;

            // Barrier: bucket updates per channel and merge deterministically
            let updated_channels = self.apply_barrier(&state, &run_ids, node_partials).await?;

            // Compute next frontier
            let mut next = Vec::<NodeKind>::new();
            for id in run_ids.iter() {
                if let Some(dests) = self.edges.get(id) {
                    for d in dests {
                        if !next.contains(d) {
                            next.push(d.clone());
                        }
                    }
                }
            }

            println!("Updated channels this step: {:?}", updated_channels);
            println!("Next frontier: {:?}", next);

            if next.is_empty() {
                println!("No outgoing edges; terminating.");
                break;
            }

            frontier = next;
        }

        println!("\n== Final state ==");
        let final_state = Arc::try_unwrap(state).expect("borrowed").into_inner();

        for (i, m) in final_state.messages.value.iter().enumerate() {
            println!("#{:02} [{}] {}", i, m.role, m.content);
        }
        println!("messages.version = {}", final_state.messages.version);

        println!("outputs (v {}):", final_state.outputs.version);
        for (i, o) in final_state.outputs.value.iter().enumerate() {
            println!("  - {:02}: {}", i, o);
        }

        println!(
            "meta (v {}): {:?}",
            final_state.meta.version, final_state.meta.value
        );

        Ok(final_state)
    }

    pub async fn apply_barrier(
        &self,
        state: &Arc<RwLock<VersionedState>>,
        run_ids: &[NodeKind],
        node_partials: Vec<NodePartial>,
    ) -> Result<Vec<&'static str>> {
        // Aggregate per-channel updates first (efficient and deterministic).
        let mut msgs_all: Vec<Message> = Vec::new();
        let mut outs_all: Vec<String> = Vec::new();
        let mut meta_all: HashMap<String, String> = HashMap::new();

        for (i, p) in node_partials.iter().enumerate() {
            let nid = run_ids.get(i).unwrap_or(&NodeKind::Other("?".to_string()));
            if let Some(ms) = &p.messages {
                println!("  {:?} -> messages: +{}", nid, ms.len());
                msgs_all.extend(ms.clone());
            }
            if let Some(os) = &p.outputs {
                println!("  {:?} -> outputs: +{}", nid, os.len());
                outs_all.extend(os.clone());
            }
            if let Some(mm) = &p.meta {
                println!("  {:?} -> meta: +{} keys", nid, mm.len());
                for (k, v) in mm {
                    meta_all.insert(k.clone(), v.clone());
                }
            }
        }

        // Apply per-channel reducers and bump versions if changed.
        let mut updated = Vec::<&'static str>::new();
        let mut s = state.write().await;

        // messages
        if !msgs_all.is_empty() {
            let before_len = s.messages.value.len();
            let before_ver = s.messages.version;
            self.add_messages.apply(&mut s.messages.value, msgs_all);
            let changed = s.messages.value.len() != before_len;
            if changed {
                s.messages.version = s.messages.version.saturating_add(1);
                println!(
                    "  barrier: messages len {} -> {}, v {} -> {}",
                    before_len,
                    s.messages.value.len(),
                    before_ver,
                    s.messages.version
                );
                updated.push("messages");
            }
        }

        // outputs
        if !outs_all.is_empty() {
            let before_len = s.outputs.value.len();
            let before_ver = s.outputs.version;
            self.append_outputs.apply(&mut s.outputs.value, outs_all);
            let changed = s.outputs.value.len() != before_len;
            if changed {
                s.outputs.version = s.outputs.version.saturating_add(1);
                println!(
                    "  barrier: outputs len {} -> {}, v {} -> {}",
                    before_len,
                    s.outputs.value.len(),
                    before_ver,
                    s.outputs.version
                );
                updated.push("outputs");
            }
        }

        // meta (map merge)
        if !meta_all.is_empty() {
            let before_keys = s.meta.value.len();
            let before_ver = s.meta.version;
            self.map_merge.apply(&mut s.meta.value, meta_all);
            let changed = s.meta.value.len() != before_keys;
            if changed {
                s.meta.version = s.meta.version.saturating_add(1);
                println!(
                    "  barrier: meta keys {} -> {}, v {} -> {}",
                    before_keys,
                    s.meta.value.len(),
                    before_ver,
                    s.meta.version
                );
                updated.push("meta");
            }
        }

        Ok(updated)
    }
}

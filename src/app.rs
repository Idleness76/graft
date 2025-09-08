use futures::future::join_all;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::channels::Channel;
use crate::message::*;
use crate::node::*;
use crate::reducers::ReducerRegistery;
use crate::state::*;
use crate::types::*;

pub struct App {
    nodes: HashMap<NodeKind, Arc<dyn Node>>,
    edges: HashMap<NodeKind, Vec<NodeKind>>,
    reducer_registery: ReducerRegistery,
}

impl App {
    // Internal (crate) factory to build an App while keeping nodes/edges private.
    pub(crate) fn from_parts(
        nodes: HashMap<NodeKind, Arc<dyn Node>>,
        edges: HashMap<NodeKind, Vec<NodeKind>>,
    ) -> Self {
        App {
            nodes,
            edges,
            reducer_registery: ReducerRegistery::default(),
        }
    }

    pub fn nodes(&self) -> &HashMap<NodeKind, Arc<dyn Node>> {
        &self.nodes
    }

    pub fn edges(&self) -> &HashMap<NodeKind, Vec<NodeKind>> {
        &self.edges
    }
    pub async fn invoke(
        &self,
        initial_state: VersionedState,
    ) -> Result<VersionedState, Box<dyn std::error::Error + Send + Sync>> {
        let state = Arc::new(RwLock::new(initial_state));
        let mut step: u64 = 0;

        // Run A and B in parallel from START, both go to END
        let mut frontier: Vec<NodeKind> = self
            .edges()
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
                let node = self.nodes().get(&id).unwrap().clone();
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
                if let Some(dests) = self.edges().get(id) {
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

        for (i, m) in final_state.messages.snapshot().iter().enumerate() {
            println!("#{:02} [{}] {}", i, m.role, m.content);
        }
        println!("messages.version = {}", final_state.messages.version());

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
    ) -> Result<Vec<&'static str>, Box<dyn std::error::Error + Send + Sync>> {
        // Aggregate per-channel updates first (efficient and deterministic).
        let mut msgs_all: Vec<Message> = Vec::new();
        let mut outs_all: Vec<String> = Vec::new();
        let mut meta_all: HashMap<String, String> = HashMap::new();

        for (i, p) in node_partials.iter().enumerate() {
            let fallback = NodeKind::Other("?".to_string());
            let nid = run_ids.get(i).unwrap_or(&fallback);
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
        let merged_updates = NodePartial {
            messages: Some(msgs_all),
            outputs: Some(outs_all),
            meta: Some(meta_all),
        };

        let mut s = state.write().await;

        //TODO need to make this whole thing prettier
        let msgs_before_len = s.messages.len();
        let msgs_before_ver = s.messages.version();

        // Track updated channels we will return (only messages for now; extend for outputs/meta when reducers added)
        let mut updated: Vec<&'static str> = Vec::new();
        self.reducer_registery.apply_all(&mut *s, &merged_updates)?;

        let msgs_changed = s.messages.len() != msgs_before_len;
        if msgs_changed {
            s.messages.set_version(msgs_before_ver.saturating_add(1));
            println!(
                "  barrier: messages len {} -> {}, v {} -> {}",
                msgs_before_len,
                s.messages.len(),
                msgs_before_ver,
                s.messages.version()
            );
        }

        if msgs_changed {
            updated.push("messages");
        }
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state() -> VersionedState {
        VersionedState::new_with_user_message("hi")
    }

    fn make_app() -> App {
        App {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            reducer_registery: ReducerRegistery::default(),
        }
    }

    /// Verifies that messages are appended and the version is incremented when a NodePartial with messages is applied.
    #[tokio::test]
    async fn test_apply_barrier_messages_update() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));
        let run_ids = vec![NodeKind::Start];
        let partial = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "foo".into(),
            }]),
            outputs: None,
            meta: None,
        };
        let updated = app
            .apply_barrier(&state, &run_ids, vec![partial])
            .await
            .unwrap();
        let s = state.read().await;
        assert!(updated.contains(&"messages"));
        assert_eq!(s.messages.snapshot().last().unwrap().content, "foo");
        assert_eq!(s.messages.version(), 2);
    }

    //TODO add test for Extras channel once that is implemented

    /// Verifies that no changes or version bumps occur when a NodePartial with no updates is applied.
    #[tokio::test]
    async fn test_apply_barrier_no_update() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));
        let run_ids = vec![NodeKind::Start];
        let partial = NodePartial {
            messages: None,
            outputs: None,
            meta: None,
        };
        let updated = app
            .apply_barrier(&state, &run_ids, vec![partial])
            .await
            .unwrap();
        let s = state.read().await;
        assert!(updated.is_empty());
        assert_eq!(s.messages.version(), 1);
        assert_eq!(s.outputs.version, 1);
        assert_eq!(s.meta.version, 1);
    }

    /// Verifies that the version does not overflow when incrementing from u64::MAX (saturating_add).
    #[tokio::test]
    async fn test_apply_barrier_saturating_version() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));
        {
            let mut s = state.write().await;
            s.messages.set_version(u32::MAX);
        }
        let run_ids = vec![NodeKind::Start];
        let partial = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "foo".into(),
            }]),
            outputs: None,
            meta: None,
        };
        let _updated = app
            .apply_barrier(&state, &run_ids, vec![partial])
            .await
            .unwrap();
        let s = state.read().await;
        assert_eq!(s.messages.version(), u32::MAX); // saturating_add
    }

    /// Verifies that multiple NodePartial updates for the same channel are merged correctly.
    #[tokio::test]
    async fn test_apply_barrier_multiple_updates() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));
        let run_ids = vec![NodeKind::Start, NodeKind::End];
        let partial1 = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "foo".into(),
            }]),
            outputs: None,
            meta: None,
        };
        let partial2 = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "bar".into(),
            }]),
            outputs: None,
            meta: None,
        };
        let updated = app
            .apply_barrier(&state, &run_ids, vec![partial1, partial2])
            .await
            .unwrap();
        let s = state.read().await;
        let messages_snapshot = s.messages.snapshot();
        assert!(updated.contains(&"messages"));
        assert_eq!(
            messages_snapshot[messages_snapshot.len() - 2].content,
            "foo"
        );
        assert_eq!(
            messages_snapshot[messages_snapshot.len() - 1].content,
            "bar"
        );
        assert_eq!(s.messages.version(), 2);
    }

    /// Verifies that empty vectors and maps do not cause any changes or version bumps.
    #[tokio::test]
    async fn test_apply_barrier_empty_vectors_and_maps() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));
        let run_ids = vec![NodeKind::Start];
        let partial = NodePartial {
            messages: Some(vec![]),
            outputs: Some(vec![]),
            meta: Some(HashMap::new()),
        };
        let updated = app
            .apply_barrier(&state, &run_ids, vec![partial])
            .await
            .unwrap();
        let s = state.read().await;
        assert!(updated.is_empty());
        assert_eq!(s.messages.version(), 1);
        assert_eq!(s.outputs.version, 1);
        assert_eq!(s.meta.version, 1);
    }
}

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
    // Reducers per channel (shared stateless singletons)
    pub add_messages: &'static AddMessages,
    pub append_outputs: &'static AppendVec,
    pub map_merge: &'static MapMerge,
}

impl App {
    pub async fn invoke(
        &self,
        initial_state: VersionedState,
    ) -> Result<VersionedState, Box<dyn std::error::Error + Send + Sync>> {
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
            add_messages: &ADD_MESSAGES,
            append_outputs: &APPEND_VEC,
            map_merge: &MAP_MERGE,
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
        assert_eq!(s.messages.value.last().unwrap().content, "foo");
        assert_eq!(s.messages.version, 2);
    }

    /// Verifies that outputs are appended and the version is incremented when a NodePartial with outputs is applied.
    #[tokio::test]
    async fn test_apply_barrier_outputs_update() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));
        let run_ids = vec![NodeKind::Start];
        let partial = NodePartial {
            messages: None,
            outputs: Some(vec!["out1".to_string()]),
            meta: None,
        };
        let updated = app
            .apply_barrier(&state, &run_ids, vec![partial])
            .await
            .unwrap();
        let s = state.read().await;
        assert!(updated.contains(&"outputs"));
        assert_eq!(s.outputs.value.last().unwrap(), "out1");
        assert_eq!(s.outputs.version, 2);
    }

    /// Verifies that meta keys are added and overwritten, and the version is incremented when a NodePartial with meta is applied.
    #[tokio::test]
    async fn test_apply_barrier_meta_update_and_overwrite() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));
        {
            let mut s = state.write().await;
            s.meta.value.insert("key".to_string(), "old".to_string());
        }
        let run_ids = vec![NodeKind::Start];
        let partial = NodePartial {
            messages: None,
            outputs: None,
            meta: Some(HashMap::from([
                ("key".to_string(), "new".to_string()),
                ("another".to_string(), "val".to_string()),
            ])),
        };
        let updated = app
            .apply_barrier(&state, &run_ids, vec![partial])
            .await
            .unwrap();
        let s = state.read().await;
        assert!(updated.contains(&"meta"));
        assert_eq!(s.meta.value.get("key"), Some(&"new".to_string()));
        assert_eq!(s.meta.value.get("another"), Some(&"val".to_string()));
        assert_eq!(s.meta.version, 2);
    }

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
        assert_eq!(s.messages.version, 1);
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
            s.messages.version = u64::MAX;
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
        assert_eq!(s.messages.version, u64::MAX); // saturating_add
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
        assert!(updated.contains(&"messages"));
        assert_eq!(s.messages.value[s.messages.value.len() - 2].content, "foo");
        assert_eq!(s.messages.value[s.messages.value.len() - 1].content, "bar");
        assert_eq!(s.messages.version, 2);
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
        assert_eq!(s.messages.version, 1);
        assert_eq!(s.outputs.version, 1);
        assert_eq!(s.meta.version, 1);
    }
}

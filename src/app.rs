use rustc_hash::FxHashMap;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::channels::Channel;
use crate::message::*;
use crate::node::*;
use crate::reducers::ReducerRegistery;
use crate::schedulers::Scheduler;
use crate::state::*;
use crate::types::*;

/// Orchestrates graph execution and applies reducers at barriers.
pub struct App {
    nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
    edges: FxHashMap<NodeKind, Vec<NodeKind>>,
    reducer_registery: ReducerRegistery,
}

impl App {
    /// Internal (crate) factory to build an App while keeping nodes/edges private.
    pub(crate) fn from_parts(
        nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
        edges: FxHashMap<NodeKind, Vec<NodeKind>>,
    ) -> Self {
        App {
            nodes,
            edges,
            reducer_registery: ReducerRegistery::default(),
        }
    }

    pub fn nodes(&self) -> &FxHashMap<NodeKind, Arc<dyn Node>> {
        &self.nodes
    }

    pub fn edges(&self) -> &FxHashMap<NodeKind, Vec<NodeKind>> {
        &self.edges
    }

    /// Execute until End (or no frontier). Applies reducers after each superstep.
    pub async fn invoke(
        &self,
        initial_state: VersionedState,
    ) -> Result<VersionedState, Box<dyn std::error::Error + Send + Sync>> {
        let state = Arc::new(RwLock::new(initial_state));
        let mut step: u64 = 0;

        // Initial frontier = successors of Start
        let mut frontier: Vec<NodeKind> = self
            .edges()
            .get(&NodeKind::Start)
            .cloned()
            .unwrap_or_default();
        if frontier.is_empty() {
            return Err("No nodes to run from START".into());
        }

        println!("== Begin run ==");
        // Initialize scheduler with a sensible default concurrency limit.
        let default_limit = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let mut scheduler = Scheduler::new(default_limit);

        loop {
            step += 1;

            // Stop if ONLY End nodes remain
            if frontier.iter().all(|n| *n == NodeKind::End) {
                println!("Reached END at step {}", step);
                break;
            }

            // Snapshot for this superstep (consistent view)
            let snapshot = { state.read().await.snapshot() };
            println!(
                "\n-- Superstep {} --\nmsgs={} v{}; extra_keys={} v{}",
                step,
                snapshot.messages.len(),
                snapshot.messages_version,
                snapshot.extra.len(),
                snapshot.extra_version
            );

            // Execute via scheduler with bounded concurrency; it decides run vs skip
            let step_result = scheduler
                .superstep(self.nodes(), frontier.clone(), snapshot.clone(), step)
                .await;

            // Reorder outputs to match ran_nodes order expected by the barrier
            let mut by_kind: FxHashMap<NodeKind, NodePartial> = FxHashMap::default();
            for (kind, part) in step_result.outputs {
                by_kind.insert(kind, part);
            }
            let run_ids: Vec<NodeKind> = step_result.ran_nodes.clone();
            let node_partials: Vec<NodePartial> = run_ids
                .iter()
                .cloned()
                .filter_map(|k| by_kind.remove(&k))
                .collect();

            // Barrier: merge all NodePartials per channel then apply reducers
            let updated_channels = self.apply_barrier(&state, &run_ids, node_partials).await?;

            // Compute next frontier
            let mut next: Vec<NodeKind> = Vec::new();
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
        let final_state = Arc::try_unwrap(state)
            .expect("state still borrowed")
            .into_inner();

        for (i, m) in final_state.messages.snapshot().iter().enumerate() {
            println!("#{:02} [{}] {}", i, m.role, m.content);
        }
        println!("messages.version = {}", final_state.messages.version());

        let extra_snapshot = final_state.extra.snapshot();
        println!(
            "extra (v {}) keys={}",
            final_state.extra.version(),
            extra_snapshot.len()
        );
        for (k, v) in extra_snapshot.iter() {
            println!("  {k}: {v}");
        }

        Ok(final_state)
    }

    /// Merge NodePartial updates, invoke reducers, bump versions if content changed.
    pub async fn apply_barrier(
        &self,
        state: &Arc<RwLock<VersionedState>>,
        run_ids: &[NodeKind],
        node_partials: Vec<NodePartial>,
    ) -> Result<Vec<&'static str>, Box<dyn std::error::Error + Send + Sync>> {
        // Aggregate per-channel updates (deterministically).
        let mut msgs_all: Vec<Message> = Vec::new();
        let mut extra_all: FxHashMap<String, Value> = FxHashMap::default();

        for (i, p) in node_partials.iter().enumerate() {
            let fallback = NodeKind::Other("?".to_string());
            let nid = run_ids.get(i).unwrap_or(&fallback);

            if let Some(ms) = &p.messages {
                if !ms.is_empty() {
                    println!("  {:?} -> messages: +{}", nid, ms.len());
                    msgs_all.extend(ms.clone());
                }
            }

            if let Some(ex) = &p.extra {
                if !ex.is_empty() {
                    println!("  {:?} -> extra: +{} keys", nid, ex.len());
                    for (k, v) in ex {
                        extra_all.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        let merged_updates = NodePartial {
            messages: if msgs_all.is_empty() {
                None
            } else {
                Some(msgs_all)
            },
            extra: if extra_all.is_empty() {
                None
            } else {
                Some(extra_all)
            },
        };

        let mut s = state.write().await;

        // Record before-states for version bump decisions
        let msgs_before_len = s.messages.len();
        let msgs_before_ver = s.messages.version();
        let extra_before = s.extra.snapshot();
        let extra_before_ver = s.extra.version();

        // Apply reducers (they do NOT bump versions)
        self.reducer_registery.apply_all(&mut *s, &merged_updates)?;

        // Detect changes & bump versions responsibly
        let mut updated: Vec<&'static str> = Vec::new();

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
            updated.push("messages");
        }

        let extra_after = s.extra.snapshot();
        let extra_changed = extra_after != extra_before;
        if extra_changed {
            s.extra.set_version(extra_before_ver.saturating_add(1));
            println!(
                "  barrier: extra keys {} -> {}, v {} -> {}",
                extra_before.len(),
                extra_after.len(),
                extra_before_ver,
                s.extra.version()
            );
            updated.push("extra");
        }

        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashMap;

    fn make_state() -> VersionedState {
        VersionedState::new_with_user_message("hi")
    }

    fn make_app() -> App {
        App {
            nodes: FxHashMap::default(),
            edges: FxHashMap::default(),
            reducer_registery: ReducerRegistery::default(),
        }
    }

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
            extra: None,
        };
        let updated = app
            .apply_barrier(&state, &run_ids, vec![partial])
            .await
            .unwrap();
        let s = state.read().await;
        assert!(updated.contains(&"messages"));
        assert_eq!(s.messages.snapshot().last().unwrap().content, "foo");
        assert_eq!(s.messages.version(), 2);
        assert_eq!(s.extra.version(), 1);
    }

    #[tokio::test]
    async fn test_apply_barrier_no_update() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));
        let run_ids = vec![NodeKind::Start];
        let partial = NodePartial {
            messages: None,
            extra: None,
        };
        let updated = app
            .apply_barrier(&state, &run_ids, vec![partial])
            .await
            .unwrap();
        let s = state.read().await;
        assert!(updated.is_empty());
        assert_eq!(s.messages.version(), 1);
        assert_eq!(s.extra.version(), 1);
    }

    #[tokio::test]
    async fn test_apply_barrier_saturating_version() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));
        {
            let mut s = state.write().await;
            s.messages.set_version(u32::MAX);
        }
        let partial = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "x".into(),
            }]),
            extra: None,
        };
        app.apply_barrier(&state, &[NodeKind::Start], vec![partial])
            .await
            .unwrap();
        let s = state.read().await;
        // saturating_add => stays at MAX
        assert_eq!(s.messages.version(), u32::MAX);
    }

    #[tokio::test]
    async fn test_apply_barrier_multiple_updates() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));
        let partial1 = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "foo".into(),
            }]),
            extra: None,
        };
        let partial2 = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "bar".into(),
            }]),
            extra: None,
        };
        let updated = app
            .apply_barrier(
                &state,
                &[NodeKind::Start, NodeKind::End],
                vec![partial1, partial2],
            )
            .await
            .unwrap();
        let s = state.read().await;
        let snap = s.messages.snapshot();
        assert!(updated.contains(&"messages"));
        assert_eq!(snap[snap.len() - 2].content, "foo");
        assert_eq!(snap[snap.len() - 1].content, "bar");
        assert_eq!(s.messages.version(), 2);
    }

    #[tokio::test]
    async fn test_apply_barrier_empty_vectors_and_maps() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));
        // Empty messages vector -> Some(vec![]) should be treated as no-op by guard
        let empty_msgs = NodePartial {
            messages: Some(vec![]),
            extra: None,
        };
        // Empty extra map -> Some(empty) should be treated as no-op by guard
        let empty_extra = NodePartial {
            messages: None,
            extra: Some(FxHashMap::default()),
        };
        let updated = app
            .apply_barrier(
                &state,
                &[NodeKind::Start, NodeKind::End],
                vec![empty_msgs, empty_extra],
            )
            .await
            .unwrap();
        let s = state.read().await;
        assert!(updated.is_empty());
        assert_eq!(s.messages.version(), 1);
        assert_eq!(s.extra.version(), 1);
    }

    #[tokio::test]
    async fn test_apply_barrier_extra_merge_and_version() {
        let app = make_app();
        let state = Arc::new(RwLock::new(make_state()));

        let mut m1 = FxHashMap::default();
        m1.insert("k1".into(), Value::String("v1".into()));
        let mut m2 = FxHashMap::default();
        m2.insert("k2".into(), Value::String("v2".into()));
        // Overwrite k1 in second partial to test key overwrite still counts as change
        m2.insert("k1".into(), Value::String("v3".into()));

        let p1 = NodePartial {
            messages: None,
            extra: Some(m1),
        };
        let p2 = NodePartial {
            messages: None,
            extra: Some(m2),
        };

        let updated = app
            .apply_barrier(&state, &[NodeKind::Start, NodeKind::End], vec![p1, p2])
            .await
            .unwrap();
        let s = state.read().await;
        assert!(updated.contains(&"extra"));
        let snap = s.extra.snapshot();
        assert_eq!(snap.get("k1"), Some(&Value::String("v3".into())));
        assert_eq!(snap.get("k2"), Some(&Value::String("v2".into())));
        assert_eq!(s.extra.version(), 2);
    }
}

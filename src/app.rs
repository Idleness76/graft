use rustc_hash::FxHashMap;
use serde_json::Value;
use std::sync::Arc;

use crate::channels::Channel;
use crate::message::*;
use crate::node::*;
use crate::reducers::ReducerRegistery;
use crate::state::*;
use crate::types::*;

/// Orchestrates graph execution and applies reducers at barriers.
#[derive(Clone)]
pub struct App {
    nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
    edges: FxHashMap<NodeKind, Vec<NodeKind>>,
    conditional_edges: Vec<crate::graph::ConditionalEdge>,
    reducer_registery: ReducerRegistery,
}

impl App {
    /// Internal (crate) factory to build an App while keeping nodes/edges private.
    pub(crate) fn from_parts(
        nodes: FxHashMap<NodeKind, Arc<dyn Node>>,
        edges: FxHashMap<NodeKind, Vec<NodeKind>>,
        conditional_edges: Vec<crate::graph::ConditionalEdge>,
    ) -> Self {
        App {
            nodes,
            edges,
            conditional_edges,
            reducer_registery: ReducerRegistery::default(),
        }
    }
    pub fn conditional_edges(&self) -> &Vec<crate::graph::ConditionalEdge> {
        &self.conditional_edges
    }

    pub fn nodes(&self) -> &FxHashMap<NodeKind, Arc<dyn Node>> {
        &self.nodes
    }

    pub fn edges(&self) -> &FxHashMap<NodeKind, Vec<NodeKind>> {
        &self.edges
    }

    /// Execute until End (or no frontier). Applies reducers after each superstep.
    /// This is now a convenience wrapper around AppRunner.
    pub async fn invoke(
        &self,
        initial_state: VersionedState,
    ) -> Result<VersionedState, Box<dyn std::error::Error + Send + Sync>> {
        use crate::runtimes::AppRunner;

        // Create a temporary runner and session using clone
        let mut runner = AppRunner::new(self.clone());
        let session_id = "temp_invoke_session".to_string();

        runner.create_session(session_id.clone(), initial_state)?;
        runner.run_until_complete(&session_id).await
    }

    /// Merge NodePartial updates, invoke reducers, bump versions if content changed.
    pub async fn apply_barrier(
        &self,
        state: &mut VersionedState,
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

        // Record before-states for version bump decisions
        let msgs_before_len = state.messages.len();
        let msgs_before_ver = state.messages.version();
        let extra_before = state.extra.snapshot();
        let extra_before_ver = state.extra.version();

        // Apply reducers (they do NOT bump versions)
        self.reducer_registery
            .apply_all(&mut *state, &merged_updates)?;

        // Detect changes & bump versions responsibly
        let mut updated: Vec<&'static str> = Vec::new();

        let msgs_changed = state.messages.len() != msgs_before_len;
        if msgs_changed {
            state
                .messages
                .set_version(msgs_before_ver.saturating_add(1));
            println!(
                "  barrier: messages len {} -> {}, v {} -> {}",
                msgs_before_len,
                state.messages.len(),
                msgs_before_ver,
                state.messages.version()
            );
            updated.push("messages");
        }

        let extra_after = state.extra.snapshot();
        let extra_changed = extra_after != extra_before;
        if extra_changed {
            state.extra.set_version(extra_before_ver.saturating_add(1));
            println!(
                "  barrier: extra keys {} -> {}, v {} -> {}",
                extra_before.len(),
                extra_after.len(),
                extra_before_ver,
                state.extra.version()
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
            conditional_edges: Vec::new(),
            reducer_registery: ReducerRegistery::default(),
        }
    }

    #[tokio::test]
    async fn test_apply_barrier_messages_update() {
        let app = make_app();
        let state = &mut make_state();
        let run_ids = vec![NodeKind::Start];
        let partial = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "foo".into(),
            }]),
            extra: None,
        };
        let updated = app
            .apply_barrier(state, &run_ids, vec![partial])
            .await
            .unwrap();
        assert!(updated.contains(&"messages"));
        assert_eq!(state.messages.snapshot().last().unwrap().content, "foo");
        assert_eq!(state.messages.version(), 2);
        assert_eq!(state.extra.version(), 1);
    }

    #[tokio::test]
    async fn test_apply_barrier_no_update() {
        let app = make_app();
        let state = &mut make_state();
        let run_ids = vec![NodeKind::Start];
        let partial = NodePartial {
            messages: None,
            extra: None,
        };
        let updated = app
            .apply_barrier(state, &run_ids, vec![partial])
            .await
            .unwrap();
        assert!(updated.is_empty());
        assert_eq!(state.messages.version(), 1);
        assert_eq!(state.extra.version(), 1);
    }

    #[tokio::test]
    async fn test_apply_barrier_saturating_version() {
        let app = make_app();
        let state = &mut make_state();
        {
            state.messages.set_version(u32::MAX);
        }
        let partial = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "x".into(),
            }]),
            extra: None,
        };
        app.apply_barrier(state, &[NodeKind::Start], vec![partial])
            .await
            .unwrap();
        // saturating_add => stays at MAX
        assert_eq!(state.messages.version(), u32::MAX);
    }

    #[tokio::test]
    async fn test_apply_barrier_multiple_updates() {
        let app = make_app();
        let state = &mut make_state();
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
                state,
                &[NodeKind::Start, NodeKind::End],
                vec![partial1, partial2],
            )
            .await
            .unwrap();
        let snap = state.messages.snapshot();
        assert!(updated.contains(&"messages"));
        assert_eq!(snap[snap.len() - 2].content, "foo");
        assert_eq!(snap[snap.len() - 1].content, "bar");
        assert_eq!(state.messages.version(), 2);
    }

    #[tokio::test]
    async fn test_apply_barrier_empty_vectors_and_maps() {
        let app = make_app();
        let state = &mut make_state();
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
                state,
                &[NodeKind::Start, NodeKind::End],
                vec![empty_msgs, empty_extra],
            )
            .await
            .unwrap();
        assert!(updated.is_empty());
        assert_eq!(state.messages.version(), 1);
        assert_eq!(state.extra.version(), 1);
    }

    #[tokio::test]
    async fn test_apply_barrier_extra_merge_and_version() {
        let app = make_app();
        let state = &mut make_state();

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
            .apply_barrier(state, &[NodeKind::Start, NodeKind::End], vec![p1, p2])
            .await
            .unwrap();
        assert!(updated.contains(&"extra"));
        let snap = state.extra.snapshot();
        assert_eq!(snap.get("k1"), Some(&Value::String("v3".into())));
        assert_eq!(snap.get("k2"), Some(&Value::String("v2".into())));
        assert_eq!(state.extra.version(), 2);
    }
}

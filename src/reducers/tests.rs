use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        channels::Channel,
        message::Message,
        node::NodePartial,
        reducers::{AddMessages, MapMerge, Reducer, ReducerRegistery, ReducerType},
        state::VersionedState,
        types::ChannelType,
    };

    // Helper to build a fresh VersionedState with a single user message.
    fn base_state() -> VersionedState {
        VersionedState::new_with_user_message("a")
    }

    // Guard function prototype (mirrors what we'd add to runtime code)
    fn channel_guard(channel: ChannelType, partial: &NodePartial) -> bool {
        match channel {
            ChannelType::Message => partial
                .messages
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false),
            ChannelType::Extra => partial
                .meta
                .as_ref()
                .map(|m| !m.is_empty())
                .unwrap_or(false),
            ChannelType::Error => false, // not handled yet
        }
    }

    #[test]
    fn test_add_messages_appends_state() {
        let reducer = AddMessages;
        let mut state = base_state();
        let initial_version = state.messages.version();

        let partial = NodePartial {
            messages: Some(vec![Message {
                role: "system".into(),
                content: "b".into(),
            }]),
            outputs: None,
            meta: None,
        };

        reducer.apply(&mut state, &partial);

        let messages_snapshot = state.messages.snapshot();
        assert_eq!(messages_snapshot.len(), 2);
        assert_eq!(messages_snapshot[0].role, "user");
        assert_eq!(messages_snapshot[1].role, "system");
        // Version bump is now the barrier's responsibility (reducers no longer bump versions)
        assert_eq!(state.messages.version(), initial_version);
    }

    #[test]
    fn test_add_messages_empty_partial_noop() {
        let reducer = AddMessages;
        let mut state = base_state();
        let initial_len = state.messages.snapshot().len();
        let initial_version = state.messages.version();

        let partial = NodePartial {
            messages: Some(vec![]),
            outputs: None,
            meta: None,
        };

        reducer.apply(&mut state, &partial);

        let messages_snapshot = state.messages.snapshot();
        assert_eq!(messages_snapshot.len(), initial_len);
        assert_eq!(state.messages.version(), initial_version);
    }

    #[test]
    fn test_map_merge_merges_and_overwrites_state() {
        let reducer = MapMerge;
        let mut state = base_state();
        // Seed meta
        state.meta.value.insert("k1".into(), "v1".into());
        let initial_version = state.meta.version;

        let mut meta_update = HashMap::new();
        meta_update.insert("k2".into(), "v2".into());
        meta_update.insert("k1".into(), "v3".into()); // overwrite

        let partial = NodePartial {
            messages: None,
            outputs: None,
            meta: Some(meta_update),
        };

        reducer.apply(&mut state, &partial);

        assert_eq!(state.meta.value.get("k1"), Some(&"v3".to_string()));
        assert_eq!(state.meta.value.get("k2"), Some(&"v2".to_string()));
        // Version bump is now the barrier's responsibility
        assert_eq!(state.meta.version, initial_version);
    }

    #[test]
    fn test_map_merge_empty_partial_noop() {
        let reducer = MapMerge;
        let mut state = base_state();
        state.meta.value.insert("k1".into(), "v1".into());
        let initial_version = state.meta.version;
        let initial_len = state.meta.value.len();

        let partial = NodePartial {
            messages: None,
            outputs: None,
            meta: Some(HashMap::new()),
        };

        reducer.apply(&mut state, &partial);

        assert_eq!(state.meta.value.len(), initial_len);
        assert_eq!(state.meta.version, initial_version);
    }

    #[test]
    fn test_enum_wrapper_dispatch() {
        // Build a small registry-like vector manually
        let reducers = vec![
            ReducerType::AddMessages(AddMessages),
            ReducerType::JsonShallowMerge(MapMerge),
        ];

        let mut state = base_state();
        // Add a seed meta key to observe overwrite behavior
        state.meta.value.insert("seed".into(), "x".into());

        let partial = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "hi".into(),
            }]),
            outputs: None,
            meta: Some({
                let mut m = HashMap::new();
                m.insert("seed".into(), "y".into());
                m
            }),
        };

        for r in &reducers {
            r.apply(&mut state, &partial);
        }

        assert_eq!(state.messages.snapshot().len(), 2);
        assert_eq!(state.meta.value.get("seed"), Some(&"y".to_string()));
    }

    #[test]
    fn test_channel_guard_logic() {
        let empty = NodePartial::default();
        assert!(!channel_guard(ChannelType::Message, &empty));
        assert!(!channel_guard(ChannelType::Extra, &empty));

        let msg_partial = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "m".into(),
            }]),
            ..Default::default()
        };
        assert!(channel_guard(ChannelType::Message, &msg_partial));
        assert!(!channel_guard(ChannelType::Extra, &msg_partial));

        let meta_partial = NodePartial {
            meta: Some({
                let mut m = HashMap::new();
                m.insert("k".into(), "v".into());
                m
            }),
            ..Default::default()
        };
        assert!(channel_guard(ChannelType::Extra, &meta_partial));
    }

    #[test]
    fn test_registry_integration_like_flow() {
        // Simulate registry behavior (using actual ReducerRegistery if present)
        let registry = ReducerRegistery::default();

        let mut state = base_state();
        let partial = NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "from node".into(),
            }]),
            meta: Some({
                let mut m = HashMap::new();
                m.insert("origin".into(), "node".into());
                m
            }),
            outputs: None,
        };

        // Only invoke reducers whose guard passes
        for channel in [ChannelType::Message, ChannelType::Extra] {
            if channel_guard(channel.clone(), &partial) {
                // ignore errors for test simplicity
                let _ = registry.try_update(channel.clone(), &mut state, &partial);
            }
        }

        assert!(
            state
                .messages
                .snapshot()
                .iter()
                .any(|m| m.content == "from node")
        );
        assert_eq!(state.meta.value.get("origin"), Some(&"node".to_string()));
    }
}

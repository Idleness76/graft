use rustc_hash::FxHashMap;

use crate::{
    node::NodePartial,
    reducers::{AddMessages, MapMerge, Reducer},
    state::VersionedState,
    types::ChannelType,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReducerType {
    AddMessages(AddMessages),
    JsonShallowMerge(MapMerge),
}

impl ReducerType {
    pub fn apply(&self, state: &mut VersionedState, update: &NodePartial) {
        match self {
            ReducerType::AddMessages(r) => r.apply(state, update),
            ReducerType::JsonShallowMerge(r) => r.apply(state, update),
        }
    }
}

#[derive(Clone)]
pub struct ReducerRegistry {
    reducer_map: FxHashMap<ChannelType, Vec<ReducerType>>,
}

/// Guard that checks whether a NodePartial actually has meaningful data
/// for the specified channel. This lets the registry skip invoking
/// reducers when there is nothing to do.
fn channel_guard(channel: &ChannelType, partial: &NodePartial) -> bool {
    match channel {
        ChannelType::Message => partial
            .messages
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        ChannelType::Extra => partial
            .extra
            .as_ref()
            .map(|m| !m.is_empty())
            .unwrap_or(false),
        ChannelType::Error => false, // not yet supported
    }
}

impl Default for ReducerRegistry {
    fn default() -> Self {
        let mut reducer_map: FxHashMap<ChannelType, Vec<ReducerType>> = FxHashMap::default();

        reducer_map.insert(
            ChannelType::Message,
            vec![ReducerType::AddMessages(AddMessages)],
        );

        reducer_map.insert(
            ChannelType::Extra,
            vec![ReducerType::JsonShallowMerge(MapMerge)],
        );

        ReducerRegistry { reducer_map }
    }
}

impl ReducerRegistry {
    pub fn try_update(
        &self,
        channel_type: ChannelType,
        state: &mut VersionedState,
        to_update: &NodePartial,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Skip if the partial has no applicable data for this channel.
        if !channel_guard(&channel_type, to_update) {
            return Ok(());
        }

        if let Some(reducers) = self.reducer_map.get(&channel_type) {
            for r in reducers {
                r.apply(state, to_update);
            }
            Ok(())
        } else {
            Err(Box::new(std::io::Error::other(format!(
                "could not find reducers for given channel: {:?}",
                channel_type
            ))))
        }
    }

    pub fn apply_all(
        &self,
        state: &mut VersionedState,
        merged_updates: &NodePartial,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Iterate all registered channels; try_update will skip via guard if no data.
        for channel in self.reducer_map.keys() {
            self.try_update(channel.clone(), state, merged_updates)?;
        }
        Ok(())
    }
}

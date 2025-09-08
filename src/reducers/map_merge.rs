use super::Reducer;
use crate::{node::NodePartial, state::VersionedState};
use rustc_hash::FxHashMap as HashMap;

#[derive(Debug, PartialEq, Clone, Hash, Eq)]
pub struct MapMerge;
impl Reducer for MapMerge {
    fn apply(&self, state: &mut VersionedState, update: &NodePartial) {
        if let Some(meta_update) = &update.meta {
            if !meta_update.is_empty() {
                for (k, v) in meta_update {
                    state.meta.value.insert(k.clone(), v.clone());
                }
            }
        }
    }
}

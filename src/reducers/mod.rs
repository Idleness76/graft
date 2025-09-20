mod add_messages;
mod append_vec;
mod map_merge;
mod reducer_registry;

pub use add_messages::AddMessages;
/* AppendVec temporarily disabled (implementation commented out in append_vec.rs)
pub use append_vec::AppendVec;
*/
pub use map_merge::MapMerge;
pub use reducer_registry::*;

use crate::node::NodePartial;
use crate::state::VersionedState;

/// Unified reducer trait: every reducer mutates VersionedState using a NodePartial delta.
/// Channels currently implemented: messages (append) and extra (shallow JSON map merge).
pub trait Reducer: Send + Sync {
    fn apply(&self, state: &mut VersionedState, update: &NodePartial);
}

#[cfg(test)]
mod tests;

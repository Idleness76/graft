//! Utilities module for optional tooling.
//! ## Submodules (using various crates...)
//!
//! - `id_generator`: For generating run, step, and node IDs.
//! - `deterministic_rng`: For deterministic random number generation in tests.
//! - `json_ext`: Extensions for JSON operations like deep merge. Just placeholder...
//! - `merge_inspector`: For debugging merge traces. Just placeholder...
//! - `clock`: Injectable time source for checkpoints.
//! - `type_guards`: Validation helpers for state shapes. Just placeholder...
//! - `message_id_helpers`: Helpers for generating unique message or tool-call IDs.  Just placeholder...

pub mod clock;
pub mod deterministic_rng;
pub mod id_generator;
pub mod json_ext;
pub mod merge_inspector;
pub mod message_id_helpers;
pub mod type_guards;

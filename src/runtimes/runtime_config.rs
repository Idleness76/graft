use crate::{runtimes::checkpointer, utils::id_generator};

use super::CheckpointerType;

pub struct RuntimeConfig {
    pub session_id: Option<String>,
    pub checkpointer: Option<CheckpointerType>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            session_id: Some(id_generator::IdGenerator::new().generate_run_id()),
            checkpointer: Some(CheckpointerType::InMemory),
        }
    }
}

impl RuntimeConfig {
    pub fn new(session_id: Option<String>, checkpointer: Option<CheckpointerType>) -> Self {
        Self {
            session_id,
            checkpointer,
        }
    }
}

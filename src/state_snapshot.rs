use crate::message::Message;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct StateSnapshot {
    pub messages: Vec<Message>,
    pub messages_version: u64,
    pub outputs: Vec<String>,
    pub outputs_version: u64,
    pub meta: HashMap<String, String>,
    pub meta_version: u64,
}

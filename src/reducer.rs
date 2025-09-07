use std::collections::HashMap;

use crate::message::*;

// Stateless singletons (ZSTs) exposed as &'static for zero allocation reuse
pub static ADD_MESSAGES: AddMessages = AddMessages;
pub static APPEND_VEC: AppendVec = AppendVec;
pub static MAP_MERGE: MapMerge = MapMerge;

pub trait Reducer<V, U>: Send + Sync {
    fn apply(&self, value: &mut V, update: U);
}

// 1) Append messages
pub struct AddMessages;
impl Reducer<Vec<Message>, Vec<Message>> for AddMessages {
    fn apply(&self, value: &mut Vec<Message>, update: Vec<Message>) {
        value.extend(update);
    }
}

// 2) Append vector for outputs (zero-sized; generic only in impl)
pub struct AppendVec;
impl<T: Clone + Send + Sync + 'static> Reducer<Vec<T>, Vec<T>> for AppendVec {
    fn apply(&self, value: &mut Vec<T>, update: Vec<T>) {
        if !update.is_empty() {
            value.extend(update);
        }
    }
}

// 3) Shallow map merge for meta
pub struct MapMerge;
impl Reducer<HashMap<String, String>, HashMap<String, String>> for MapMerge {
    fn apply(&self, value: &mut HashMap<String, String>, update: HashMap<String, String>) {
        for (k, v) in update {
            value.insert(k, v);
        }
    }
}

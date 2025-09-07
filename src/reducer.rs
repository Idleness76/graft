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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Verifies that ADD_MESSAGES appends new messages to the existing vector.
    fn test_add_messages_appends() {
        let mut v = vec![Message {
            role: "user".into(),
            content: "a".into(),
        }];
        let update = vec![Message {
            role: "system".into(),
            content: "b".into(),
        }];
        ADD_MESSAGES.apply(&mut v, update);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].role, "user");
        assert_eq!(v[1].role, "system");
    }

    #[test]
    /// Checks that ADD_MESSAGES does nothing when the update vector is empty.
    fn test_add_messages_empty_update() {
        let mut v = vec![Message {
            role: "user".into(),
            content: "a".into(),
        }];
        let update: Vec<Message> = vec![];
        ADD_MESSAGES.apply(&mut v, update);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].role, "user");
    }

    #[test]
    /// Verifies that APPEND_VEC appends elements to the existing vector.
    fn test_append_vec_appends() {
        let mut v = vec![1, 2];
        let update = vec![3, 4];
        APPEND_VEC.apply(&mut v, update);
        assert_eq!(v, vec![1, 2, 3, 4]);
    }

    #[test]
    /// Checks that APPEND_VEC does nothing when the update vector is empty.
    fn test_append_vec_empty_update() {
        let mut v = vec!["a".to_string()];
        let update: Vec<String> = vec![];
        APPEND_VEC.apply(&mut v, update);
        assert_eq!(v, vec!["a"]);
    }

    #[test]
    /// Verifies that MAP_MERGE merges two maps and overwrites existing keys with new values.
    fn test_map_merge_merges_and_overwrites() {
        let mut map = HashMap::new();
        map.insert("k1".to_string(), "v1".to_string());
        let mut update = HashMap::new();
        update.insert("k2".to_string(), "v2".to_string());
        update.insert("k1".to_string(), "v3".to_string()); // overwrite
        MAP_MERGE.apply(&mut map, update);
        assert_eq!(map.get("k1"), Some(&"v3".to_string()));
        assert_eq!(map.get("k2"), Some(&"v2".to_string()));
    }

    #[test]
    /// Checks that MAP_MERGE does nothing when the update map is empty.
    fn test_map_merge_empty_update() {
        let mut map = HashMap::new();
        map.insert("k1".to_string(), "v1".to_string());
        let update: HashMap<String, String> = HashMap::new();
        MAP_MERGE.apply(&mut map, update);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("k1"), Some(&"v1".to_string()));
    }
}

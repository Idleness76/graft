//! Collection utilities and common patterns for the graft crate.

use rustc_hash::FxHashMap;
use serde_json::Value;

/// Creates a new `FxHashMap` for string keys and JSON values.
///
/// This is a common pattern throughout the codebase for extra data storage.
///
/// # Examples
///
/// ```rust
/// use graft::utils::collections::new_extra_map;
///
/// let mut extra = new_extra_map();
/// extra.insert("key".to_string(), serde_json::json!("value"));
/// ```
#[inline]
pub fn new_extra_map() -> FxHashMap<String, Value> {
    FxHashMap::default()
}

/// Creates a new `FxHashMap` with the specified capacity.
///
/// Useful when you know the approximate size of the map ahead of time.
///
/// # Parameters
/// * `capacity` - Initial capacity hint for the map
///
/// # Returns
/// A new `FxHashMap` with the specified capacity.
#[inline]
pub fn new_extra_map_with_capacity(capacity: usize) -> FxHashMap<String, Value> {
    FxHashMap::with_capacity_and_hasher(capacity, Default::default())
}

/// Extension trait for working with extra data maps.
pub trait ExtraMapExt {
    /// Insert a string value into the extra map.
    fn insert_string(&mut self, key: impl Into<String>, value: impl Into<String>);

    /// Insert a numeric value into the extra map.
    fn insert_number(&mut self, key: impl Into<String>, value: impl Into<serde_json::Number>);

    /// Insert a boolean value into the extra map.
    fn insert_bool(&mut self, key: impl Into<String>, value: bool);
}

impl ExtraMapExt for FxHashMap<String, Value> {
    fn insert_string(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.insert(key.into(), Value::String(value.into()));
    }

    fn insert_number(&mut self, key: impl Into<String>, value: impl Into<serde_json::Number>) {
        self.insert(key.into(), Value::Number(value.into()));
    }

    fn insert_bool(&mut self, key: impl Into<String>, value: bool) {
        self.insert(key.into(), Value::Bool(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_extra_map() {
        let map = new_extra_map();
        assert!(map.is_empty());
    }

    #[test]
    fn test_extra_map_ext() {
        let mut map = new_extra_map();
        map.insert_string("name", "test");
        map.insert_number("count", 42);
        map.insert_bool("enabled", true);

        assert_eq!(map.len(), 3);
        assert_eq!(map["name"], Value::String("test".to_string()));
        assert_eq!(map["count"], Value::Number(42.into()));
        assert_eq!(map["enabled"], Value::Bool(true));
    }
}

/*!
Helper utilities for SQLite checkpointer to reduce code duplication.
*/

use serde_json::Value;
use crate::runtimes::checkpointer::CheckpointerError;
use crate::utils::json_ext::{serialize_with_context, deserialize_with_context};

/// Helper for JSON serialization with consistent error formatting.
pub(super) fn serialize_json<T: serde::Serialize>(
    value: &T,
    context: &'static str,
) -> Result<String, CheckpointerError> {
    serialize_with_context(value, context, |e, ctx| {
        CheckpointerError::Other { 
            message: format!("{ctx} serialize: {e}")
        }
    })
}

/// Helper for JSON deserialization with consistent error formatting.
pub(super) fn deserialize_json<T: serde::de::DeserializeOwned>(
    json: &str,
    context: &'static str,
) -> Result<T, CheckpointerError> {
    deserialize_with_context(json, context, |e, ctx| {
        CheckpointerError::Other { 
            message: format!("{ctx} parse: {e}")
        }
    })
}

/// Helper for JSON value deserialization with consistent error formatting.
pub(super) fn deserialize_json_value<T: serde::de::DeserializeOwned>(
    value: Value,
    context: &'static str,
) -> Result<T, CheckpointerError> {
    serde_json::from_value(value)
        .map_err(|e| CheckpointerError::Other { 
            message: format!("{context} parse (serde): {e}")
        })
}

/// Helper for extracting required JSON fields with consistent error formatting.
pub(super) fn require_json_field(
    field: Option<String>,
    field_name: &'static str,
) -> Result<String, CheckpointerError> {
    field.ok_or_else(|| {
        CheckpointerError::Other { 
            message: format!("missing {field_name} for persisted checkpoint")
        }
    })
}
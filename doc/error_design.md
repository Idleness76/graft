# Error Handling and Telemetry Design

This document outlines the unified error handling design: typed `RunnerError` for control flow and user-facing diagnostics, plus structured `ErrorEvent` telemetry persisted via the dedicated errors channel.

## TL;DR

- Runner surfaces `RunnerError` and, on failure, emits an `ErrorEvent` into the dedicated errors channel via `NodePartial.errors`.
- Errors are tracked in a single location: the `ErrorsChannel` with proper reducer support.
- The errors channel uses the same persistence model as other channels (messages, extra).
- No dual-tracking complexity - clean, single-path error handling.

## Key concepts

- `RunnerError` — Control-flow error for immediate failure handling. Plays well with `?` and `miette` diagnostics.
- `ErrorEvent` — Structured telemetry breadcrumb with scope (Node/Scheduler/Runner), timestamp, tags, and context.
- `ErrorsChannel` — Persisted channel for structured error events, handled by the `AddErrors` reducer.
- `NodePartial.errors` — The clean interface for nodes to emit error events via the standard barrier/reducer flow.

## Why this design works

### Control flow vs. telemetry
- `RunnerError` is for control flow. It bubbles via `?`, enabling `miette` to render diagnostics (exit codes, failing tests, etc.).
- `ErrorEvent` is telemetry. It's a durable, structured breadcrumb you can inspect, aggregate, or display in UIs.

### Unified persistence model
- The `ErrorsChannel` follows the same persistence pattern as `MessagesChannel` and `ExtrasChannel`.
- Error events are persisted via checkpoints and survive process restarts consistently with other state.
- The `AddErrors` reducer handles error aggregation using the same barrier mechanics as other channels.

### Clean separation of concerns
- Nodes emit errors via `NodePartial.errors: Option<Vec<ErrorEvent>>` - same pattern as messages.
- The `AddErrors` reducer appends error events to the `ErrorsChannel` - no special JSON handling needed.
- Demos and external tools read from `state.errors.snapshot()` for pretty printing and analysis.

## When to use which

- Use `RunnerError` for immediate failure handling:
  - Return `Result<_, RunnerError>` to let `miette` render diagnostics and keep control flow unambiguous.
- Use the errors channel for structured error tracking:
  - Nodes emit `ErrorEvent`s via `NodePartial.errors` when they want to record telemetry without failing.
  - End-of-run pretty print via `state.errors.snapshot()` for CLI UX and dashboards.
  - Error events persist across checkpoints and restarts like other channel data.

## Implementation details

### Reducer pattern
- `AddErrors` implements the `Reducer` trait, handling `NodePartial.errors: Option<Vec<ErrorEvent>>`.
- Follows the same pattern as `AddMessages`: append new events to the channel's vector.
- Registered in `ReducerRegistry` for `ChannelType::Error` with proper channel guards.

### Error injection flow
- Runner creates `ErrorEvent` with appropriate scope (Node/Scheduler/Runner) and context.
- Builds `NodePartial { errors: Some(vec![event]), .. }` and injects via `App::apply_barrier`.
- `AddErrors` reducer appends events to `state.errors` during barrier processing.

### Channel guard
- `channel_guard` checks `partial.errors.as_ref().map(|v| !v.is_empty())` for efficient no-op detection.
- Only invokes reducer when there are actual error events to process.

## Bottom line

The unified error handling design provides:
- Typed `RunnerError` for control flow and user diagnostics.
- Structured `ErrorEvent` persisted via the dedicated `ErrorsChannel` with proper reducer support.
- Clean, single-path design that's consistent with the rest of the channel architecture.

## Testing error persistence

The implementation includes comprehensive tests to verify that error events are properly persisted to the database and restored correctly. These tests serve as both verification and documentation of the error persistence capabilities.

### Running the error persistence tests

To run just the error persistence tests:
```bash
cargo test test_error_persistence
```

To run the tests with console output (showing success messages and details):
```bash
cargo test test_error_persistence -- --nocapture
```

### What the tests verify

The persistence tests include:

1. **`test_error_persistence_roundtrip()`** - End-to-end persistence verification:
   - Creates error events with different scopes (Node, App, Scheduler)
   - Tests error details, tags, context data, and cause chains
   - Saves to SQLite database via checkpointing
   - Loads from database and verifies all data is preserved

2. **`test_error_serialization_format()`** - Format compatibility verification:
   - Tests complex nested JSON data in error details and context
   - Verifies error cause chains with nested details
   - Ensures serialization format stability for cross-version compatibility

When run with `--nocapture`, the tests output detailed success information:
```
✅ Error persistence test completed successfully!
   - Saved 3 error events to database
   - Restored 3 error events from database
   - All error details, scopes, tags, and context preserved
   - Error cause chains correctly serialized/deserialized

✅ Error serialization format test passed!
   - Complex nested JSON data preserved
   - Error cause chains with details preserved
   - All context and metadata intact
```

These tests serve as proof that the unified error handling system works end-to-end and can be used by anyone to verify the implementation.

# Error Handling and Telemetry Design

This document outlines a dual-path design for errors: typed `RunnerError` for control flow and user-facing diagnostics, plus structured `ErrorEvent` telemetry persisted via `extra["errors"]` and mirrored to an in-memory errors channel.

## TL;DR

- Runner surfaces `RunnerError` and, on failure, emits an `ErrorEvent` into `extra["errors"]` and the in-memory errors channel.
- No DB/persistence schema change is required.
- Keep the errors channel in-memory only for now; also write to `extra["errors"]` to align with current reducer and checkpoint model.
- Keeping both is intentional: they serve different purposes and have different lifetimes.

## Key concepts

- `RunnerError` — Control-flow error for immediate failure handling. Plays well with `?` and `miette` diagnostics.
- `ErrorEvent` — Structured telemetry breadcrumb with scope (Node/Scheduler/Runner), timestamp, tags, and context.
- errors channel — First-class in-memory stream for ergonomic access and pretty printing.
- `extra["errors"]` — Persisted array of `ErrorEvent`s; survives restarts and is visible in checkpoints.

## Why this design works

### Control flow vs. telemetry
- `RunnerError` is for control flow. It bubbles via `?`, enabling `miette` to render diagnostics (exit codes, failing tests, etc.).
- `ErrorEvent` is telemetry. It’s a durable, structured breadcrumb you can inspect, aggregate, or display in UIs.

### Durability without schema churn
- `extra["errors"]` lives on the persisted `extra` channel, so it survives process restarts and appears in checkpoints now, without changing the DB schema.
- The errors channel is in-memory for ergonomic access and pretty-print at runtime. Using `extra["errors"]` as the persistence rail keeps breadcrumbs available across sessions today.

### Separation of concerns
- Runner emits the event (single place that sees all failure modes and has session context), injects via a synthetic `NodePartial`, and lets reducers merge it in—consistent with the barrier model.
- Demos read from the in-memory errors channel for pretty print, while the same events exist in `extra["errors"]` for persistence or external tooling.

## When to use which

- Use `RunnerError` for immediate failure handling:
  - Return `Result<_, RunnerError>` to let `miette` render diagnostics and keep control flow unambiguous.
- Use the errors channel for live inspection and reporting:
  - End-of-run pretty print is fast and structured; good for CLI UX and dashboards.
- Use `extra["errors"]` when you need persistence today:
  - Since the errors channel isn’t persisted, `extra["errors"]` ensures error trails survive restarts and can be reloaded from checkpoints.

## Trade-offs and future cleanup

- Duplication (intentional):
  - Same events live in both `extra["errors"]` (persisted) and errors (in-memory).
  - If/when we add dedicated persistence for the errors channel:
    - Option A: stop writing to `extra["errors"]`.
    - Option B: keep it for compatibility behind a feature flag or config.
- Size growth of `extra["errors"]`:
  - For long-running sessions, the JSON array can grow.
  - Mitigations: cap length, rotate/summarize, or migrate to dedicated persisted storage for errors.
- Version gating:
  - Injecting errors bumps `extra.version`, which can trigger downstream gating.
  - Document `extra["errors"]` as a reserved/system key so nodes can ignore it if needed.

## Bottom line

Don’t switch to “only errors channel” yet—you’d lose durable breadcrumbs across restarts because the errors channel isn’t persisted.

The current dual-path design is sound:
- Typed `RunnerError` for control flow and user diagnostics.
- Structured `ErrorEvent` persisted via `extra["errors"]` and mirrored to the in-memory errors channel for runtime UX.

Later, once we have explicit persistence for errors, we can remove or de-emphasize `extra["errors"]` to avoid duplication.
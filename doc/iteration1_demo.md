# Graft Demo Overview

> Status: Draft  (Initial documentation pass for `src/demo.rs` scaffold)

This document explains the architecture illustrated by `src/demo.rs`, sets shared terminology, and lays out an evolutionary path toward an MVP Rust framework analogous in spirit to Python's LangGraph (graphs of stateful, concurrent, tool‑using agents). It is intentionally explicit so future refactors can align with a documented mental model.

---
## 1. High-Level Intent
The demo showcases a minimal execution engine that:
- Represents **application state as versioned, channelized data** (messages, outputs, meta) with per-channel reducers.
- Executes **nodes (user-defined units of work)** in supersteps over a **directed graph**.
- Performs **parallel node execution per frontier**, then a **barrier merge** that deterministically folds all node partial updates into global state and bumps version counters only when a channel truly changes.
- Provides the seed of a **dataflow + actor hybrid** suitable for LLM / tool orchestration workflows.

Think of it as: "small, explicit, deterministic, parallelizable graph runtime with pluggable state reducers".

---
## 2. Core Concepts (Demo Vocabulary)
| Concept | Demo Representation | Purpose |
|---------|--------------------|---------|
| Channel | messages / outputs / meta | Independent streams / maps of state updated by nodes. |
| Versioned<T> | Wrapper with `value` & `version` | Change detection, optimistic concurrency hooks later. |
| Reducer | Trait `Reducer<V,U>` | Canonical merge logic per channel (append, merge, etc.). |
| Node | `async fn run(snapshot, ctx)` | Pure(ish) function from snapshot → partial updates. |
| NodePartial | Optional per-channel deltas | Sparse update representation. |
| Graph | `nodes` + `edges` | Topology controlling execution order. |
| Frontier | Vec<node_id> | Set of nodes runnable in current superstep. |
| Barrier | `apply_barrier` | Deterministic fold of all partials → global state. |
| App | Compiled graph + reducers | Orchestrates invoke lifecycle. |

### 2.1 Channels
Currently hard-coded (messages, outputs, meta). Long term: a registry of typed channels with (value type, reducer strategy, conflict policy, retention policy, serialization format).

### 2.2 Versioned State & Snapshots
`VersionedState` groups channels. `snapshot()` captures immutable copies for node execution. Version increments only when a channel's content length (or key count) truly changes — a minimal heuristic now; can evolve to hash or structural equality.

### 2.3 Reducers
Reducer trait isolates merge semantics from execution logic. Present reducers:
- AddMessages: append vector of `Message`.
- AppendVec<T>: generic append for outputs.
- MapMerge: shallow `HashMap<String,String>` insertion.
Future: CRDT-inspired merges, conflict resolution policies, semantic reducers (e.g., keep last N, deduplicate by id, top-K scoring).

### 2.4 Nodes
Nodes are async units reading a snapshot, never mutating global state directly. They emit a sparse `NodePartial`. This isolates side effects and enables parallelism & deterministic merges.
Future node enhancements: cancellation, deadlines, tracing spans, structured errors, capability injection (LLM client, vector store, tool registry), streaming yields, incremental partial emission.

### 2.5 Graph Compilation
`GraphBuilder` collects node instances + adjacency (edges) → `App`. Compilation step is where future validation (cycle detection, channel capability checks, static scheduling optimization) should live.

### 2.6 Execution Model (Invoke Lifecycle)
1. Initialize state (with an initial user message).
2. Seed frontier from `START` edges.
3. Loop supersteps:
   - If only `END` remains → terminate.
   - Take consistent snapshot (cheap clone of owned data) for this step.
   - Spawn all frontier nodes (excluding `END`) concurrently.
   - Await all futures, gathering `NodePartial`s.
   - Barrier merge: bucket by channel → apply reducers → bump versions.
   - Build next frontier via outgoing edges of executed nodes.
4. Return final `VersionedState`.

Determinism: Because channel merges are aggregated before application and append order currently follows frontier iteration order, results are stable for a fixed frontier ordering. (Longer term: define explicit ordering or commutative reducers.)

### 2.7 Barrier Semantics
The barrier isolates write phase to ensure:
- No interleaving writes mid-superstep.
- Deterministic version increments (only if changed).
- Single place to enforce invariants (size limits, validation, metrics, audit logs, diff emission).
Future: pluggable barrier phases (pre-merge hooks, conflict detection, derived channel materialization, subscription notifications, persistent snapshot commit).

---
## 3. Design Principles Emerging
- Explicit channels > opaque monolithic state.
- Reducer-driven merges > ad hoc mutation.
- Snapshot in, partial out → purity boundary → easier testing & caching.
- Deterministic superstep barrier → reproducibility, tracing, replay.
- Minimal trait surface now to keep iteration velocity high.

---
## 4. Extensibility Roadmap (Incremental Evolution)
### 4.1 Short-Term (Prototype → MVP)
1. Dynamic channel registry (enum or trait-object based) instead of hard-coded 3.
2. Error model overhaul: structured error enum + per-node failure policy (retry, skip, abort graph, fallback edge).
3. Logging & tracing (instrument with `tracing` spans: node_id, step, versions touched).
4. Deterministic ordering guarantee (stable frontier ordering; optional topological pre-schedule).
5. Graph validation: cycles (allow vs prevent), unreachable node detection, channel capability checks.
6. Node configuration inputs (constants, parameters, secrets) via dependency injection context.
7. Basic persistence hooks (serialize final state; maybe snapshot per superstep for debugging).
8. CLI demo & README examples with real-world flavored nodes (LLM call stub, tool invocation stub).

### 4.2 Intermediate (MVP → Practitioner Usability)
1. Pluggable storage backends (in-memory, sled, sqlite) for state durability.
2. Streaming outputs: channel variant that supports progressive emission (e.g., token stream with flush semantics).
3. Concurrency controls: max parallelism, priority, rate limiting per node.
4. Conditional routing: edges with predicates over snapshot (dynamic frontier filtering).
5. Observability surfaces: metrics (per node latency, channel growth), event log, debug web dashboard.
6. Deterministic replay: record input snapshot + partial + RNG seeds.
7. Cancellation & deadlines: per invoke or per node execution budget.
8. Tool / model abstraction: traits for LLM, embeddings, retrievers; resource pooling.

### 4.3 Advanced (Post-MVP → LangGraph Parity & Differentiators)
1. Branching & joins with state isolation (fork snapshots; CRDT merges / custom resolution).
2. Checkpoint & resume (persist versioned channels and frontier cursor).
3. Multi-tenant graph instances; compiled plan caching.
4. Policy layer: guardrails / moderation / authorization filters around node execution.
5. DSL or macro-based declarative graph definition (possible `#[node]` procedural macro deriving boilerplate).
6. Deterministic distributed execution (shard frontier across executors; barrier coordination over network).
7. Adaptive scheduling (feedback-driven reordering, speculative execution).
8. Formal verification hooks: static analysis of channel access patterns.

---
## 5. Comparison to LangGraph (Intent)
| Aspect | LangGraph (Python) | Target (Rust Graft) |
|--------|--------------------|---------------------|
| Language | Python dynamic | Rust static, zero-cost abstractions |
| Concurrency | Async / threads via Python event loop | Native async w/ Tokio; fine-grained parallel node execution |
| State | Usually dict-like, dynamic | Typed, channelized, versioned |
| Determinism | Depends on user code / ordering | Barrier + reducers + ordering contracts |
| Performance | GIL / interpreter overhead | Native speed, better for high-frequency orchestration |
| Extensibility | Ecosystem-first, dynamic patching | Trait-based pluggability, compile-time guarantees |

---
## 6. Potential Interfaces (Futures)
```rust
// Sketches only
trait Channel: Send + Sync {
    type Value: Clone + Send + Sync + 'static;
    type Delta; // partial update type
    fn apply(&self, current: &mut Self::Value, delta: Self::Delta) -> bool; // returns changed?
}

struct Registry { /* channel id -> dyn Channel */ }

#[async_trait]
trait Node {
    async fn run(&self, snap: &SnapshotView, ctx: &NodeCtx) -> NodeDeltas; // NodeDeltas: map channel_id -> Box<dyn Any>
}
```

---
## 7. Testing Strategy (Planned)
- Unit tests per reducer (idempotence where expected, append behavior, version increments only on change).
- Node tests with synthetic snapshots (no need to stand up full app).
- Graph execution golden tests: fixed random seed, assert final channel contents & version history.
- Property tests: repeated merges produce associative results for commutative reducers.

---
## 8. Open Questions (Need Clarification)
Please review & answer to guide next documentation & implementation steps:
1. Channel Generalization: Should channels be first-class (dynamic registry) in MVP, or is hard-coded trio acceptable initially?
2. Persistence Scope: Is early MVP in-memory only, or should we design storage traits now?
3. Failure Semantics: On single node error—abort whole invoke, or mark failed and continue if others succeed?
4. Determinism Requirements: Is bit-for-bit reproducibility a goal (ordering guarantees), or soft functional determinism sufficient?
5. External Integrations: Which come first—LLM API, vector store, tool execution, HTTP calls?
6. DSL / Declarative Graph: Prioritize now (macros / builder ergonomics) or defer behind core runtime solidity?
7. Licensing & Positioning: Is the intent to be a drop-in alternative to LangGraph or a differentiated runtime with performance focus?
8. Observability Depth: How important early (structured logs only vs metrics dashboard)?
9. State Size Constraints: Expect large message histories / outputs requiring pruning policies soon?
10. Concurrency Limits: Need configurable per-node parallelism controls immediately?
11. Security / Sandboxing: Any near-term need for resource isolation (untrusted node code)?
12. Config Management: How will secrets / API keys reach nodes (env, vault, injection)?
13. Streaming Outputs: Required for first LLM-focused milestone or optional later?
14. Deterministic Replay: Mission-critical for debugging from day one?
15. Naming: Keep `App` or rename to `GraphRuntime` / `Engine` for clarity?

---
## 9. Immediate Next Documentation Steps (After Clarifications)
- Flesh out architecture diagrams (mermaid graph + barrier flow).
- Document lifecycle hooks & future trait surfaces.
- Add a richer example: LLM call simulation node + conditional edge.
- Create CONTRIBUTING.md with design tenets.
- Expand README to link to this DEMO and highlight roadmap slice.

---
## 10. Quick Start (Current Demo)
```bash
cargo run --bin demo   # or adjust if binary target renamed
```
(At present `demo.rs` is a standalone file; consider moving to `main.rs` or providing example binary target in Cargo.toml.)

---
## 11. Glossary (Growing)
- Superstep: A synchronous phase where all frontier nodes run against the same snapshot.
- Frontier: Set of node IDs runnable in the current superstep.
- Barrier: Merge boundary ensuring deterministic state advancement.
- Channel: A logical partition of global state with a dedicated reducer.

---
## 12. Appendix: Identified Enhancements From Code Review
| Area | Observation | Action Idea |
|------|-------------|------------|
| Error Handling | Uses boxed error & `?` | Introduce `thiserror` enum for core runtime errors. |
| Frontier Build | Linear `contains` checks | Use `HashSet` for next frontier de-dup & order stabilization layer. |
| Determinism | Append order depends on iteration order of `frontier` Vec | Explicitly sort node IDs or record insertion order spec. |
| Version Strategy | Based only on length/key count | Switch to content hash to detect in-place unchanged appends (future). |
| Logging | `println!` macros | Swap in `tracing` with feature-gated subscriber. |
| GraphBuilder | No validation | Add: missing node targets, duplicate edges, cycle policy. |
| NodePartial -> Barrier | Clones accumulate | Optimize via small vec, reserve, or zero-copy deltas. |

---
Feedback on the open questions will drive the next iteration of both code and docs.

# Graft × LangGraph Port (Rig-powered) — Aligned With Our Barrier Model

Status: Updated plan (v2). This version explicitly centers Graft’s Node → Partial → Barrier → Reducers execution model and maps LangGraph concepts onto it. No Task/NextAction API; no FanOut construct (we already run the frontier in parallel). The plan focuses on durable execution, interrupts, conditional routing, LLM/tool nodes via Rig, streaming, and RAG — implemented the LangGraph way while preserving our deterministic barrier semantics.

## 0) Executive Summary

Graft today:
- Nodes read an immutable `StateSnapshot` and return a sparse `NodePartial`.
- A scheduler runs the current frontier concurrently; a barrier merges all partials via a reducer registry; versions bump only when content changes; the next frontier is built from edges of nodes that ran.

What we’ll add to match LangGraph production usage — without changing the Node/Barrier contract:
- Durable execution (checkpointer) with resumable sessions across supersteps.
- Human-in-the-loop interrupts: pause before/after configured nodes or per-step; resume later after state edits.
- Conditional edges: routing decided by predicates over the snapshot at the barrier, supporting loops.
- LLM/tool nodes via Rig that write their results as `NodePartial` updates; chat history lives in the messages channel.
- Streaming: emit real-time execution and token events while preserving barrier determinism for committed state.
- RAG components: smart chunkers, embeddings, and a pluggable vector store.

We keep superstep parallelism (no separate FanOut API) and our reducers as core differentiators for determinism and throughput.

---

## 1) How Graft Works (today)

- `Node::run(snapshot, ctx) -> NodePartial` where:
  - `snapshot` is a deep copy of `VersionedState` channels (messages, extra, ...).
  - `NodePartial` can append messages and merge extra keys (sparse updates).
- Scheduler picks the next frontier, runs nodes in parallel (excluding End), and records versions seen per node (gating reruns).
- Barrier aggregates all NodePartials deterministically and applies reducers; versions bump only if content changed.
- Next frontier = union of outgoing edges for nodes that ran (cycles are supported; version gating prevents tight loops from thrashing).

This mirrors LangGraph’s “function of state → state update” per node, with deterministic merges and explicit channels.

---

## 2) How LangGraph Works (relevant features)

- Stateful graphs: Nodes transform shared state; edges can be conditional; cycles are normal.
- Durable execution via checkpointers; resume mid-run.
- Human‑in‑the‑loop via “interrupts” before/after nodes; resume after external input.
- Memory: chat messages are first‑class in state.
- Streaming: graph emits step/node/update events and can stream model tokens.

We will port these features using our Node/Barrier model, not by changing Node semantics.

---

## 3) Gaps To Close (LangGraph parity)

- Checkpointer & sessions: persist and resume (state, step, frontier, versions_seen).
- Interrupts: pause points before/after nodes or per-step; resume with state edits.
- Conditional routing: edge predicates evaluated against the snapshot (post-barrier) to compute the next frontier.
- LLM & tools via Rig: nodes that call models/tools and write to channels (messages/extra), with chat history support.
- Streaming: event bus emitting execution/partial/token events; barrier remains the commit point.
- RAG: chunkers, embeddings, simple vector store, and a demonstration graph.
- Observability & error handling: structured tracing, error channel, retry policies.

---

## 4) Architecture Additions

4.1 Checkpointer + Sessions
- Add `checkpointer` module with trait:
  - `save(session_id, checkpoint)` and `load(session_id)` and `list(session_id)`.
- Checkpoint content (serde): `VersionedState`, current `frontier`, `step`, scheduler `versions_seen`, optional metadata.
- Implement `InMemoryCheckpointer` and `PostgresCheckpointer` (SQLx). Save after each barrier by default.

4.2 Session Runner (stepwise)
- Add `AppRunner` that wraps `App` and executes one superstep at a time:
  - `run_step(session_id, options) -> StepReport` loads checkpoint → runs exactly one barriered superstep → saves checkpoint → returns details (ran nodes, skipped, updated channels, next frontier).
  - `run_until(condition)` for batch modes (optional).

4.3 Interrupts (human-in-the-loop)
- Runner options: `interrupt_before: Vec<NodeKind>`, `interrupt_after: Vec<NodeKind>`, `interrupt_each_step: bool`.
- Behavior:
  - If `interrupt_before` contains any node in the current frontier, return `Paused(before=that_node)` without running.
  - After a step, if any `interrupt_after` node was in `ran_nodes`, return `Paused(after=that_node)`.
  - If `interrupt_each_step`, always return `Paused(step=step_no)` after saving.
- Resumption: user edits state or adds messages; call `run_step` again.

4.4 Conditional Edges
- Extend `GraphBuilder` to support conditional edges:
  - `add_conditional_edge(from, yes, no, predicate: fn(&StateSnapshot) -> bool + Send + Sync + 'static)`.
- Apply after a barrier when computing the next frontier:
  - For each executed node, evaluate predicates against the fresh snapshot; push `yes` or `no` destination; de-duplicate.
- Keep plain `add_edge` for unconditional edges.

4.5 LLM & Tools via Rig (nodes stay pure)
- Add an `llm` adapter that turns messages in `snapshot.messages` into Rig message types.
- Create an example LLM node (e.g., `LlmChatNode`) that:
  - Builds the prompt from prior `messages` and optional `extra` context.
  - Calls Rig; the assistant’s reply becomes a `Message` appended in `NodePartial.messages`.
  - Writes any tool or trace outputs into `extra` under namespaced keys (e.g., `llm.*`).
- Tools: define nodes that call external APIs and write results to `extra` (JSON), leaving control to the graph.

4.6 Streaming (events + tokens)
- Introduce an internal event bus interface:
  - Events: `NodeStart`, `TokenChunk{node}`, `NodeFinish`, `BarrierApplied{updated_channels}`, `Paused`, etc.
  - Provide default async channel sink (bounded) with backpressure policy; allow consumers (SSE/WebSocket) to subscribe.
- For LLM nodes with streaming, publish token chunks as transient events; final committed assistant message is appended at barrier via `NodePartial`.

4.7 Error Handling & Observability
- Add `ChannelType::Error` and a reducer to append structured errors; barrier decides version bump.
- Runner options: retry policy per node (count/backoff) and optional `on_error: fn(node, error, snapshot) -> Option<NodeKind>` to route to fallback nodes.
- Instrument `App` and runner with `tracing`: per-node timings, step summaries, versions seen, updated channels.

Note: We do NOT add FanOut. Supersteps already execute the frontier concurrently, aligning with LangGraph patterns and avoiding extra APIs.

---

## 5) Detailed 3‑Week Plan (aligned to Node/Barrier)

### Week 1 — Sessions, Checkpointer (in-memory), Conditional Edges (MVP), LLM Node (non-streaming)

Goals:
- Add `AppRunner` and `Checkpointer` with in-memory backend.
- Introduce conditional edges evaluated post‑barrier.
- RuntimeConfig

Tasks:
- Add `src/runtime/runner.rs` (`AppRunner`) with:
  - `run_step(session_id, options)` loads checkpoint or seeds from initial state + start frontier; executes one superstep via existing `App` internals factored into a `run_one_superstep` helper (non-breaking); saves checkpoint; returns `StepReport` (ran/skipped/updated/frontier/step/state_versions).
  - Interrupt options (`interrupt_before/after/each_step`) enforced around superstep execution.
- Add `src/runtime/checkpointer.rs` with trait and `InMemoryCheckpointer`.
- Extend `GraphBuilder` with conditional edges (stored alongside existing adjacency), and extend the next‑frontier computation to evaluate conditions against the new snapshot.
- Add `src/llm/rig_adapter.rs` and an example `LlmChatNode` writing to `messages`.
- RuntimeConfig goes in tandem with the checkpointer, we want to be able to pass at least a session id to be saved for each session row.
- add RuntimeConfig, pass along to builder (or something like app.withConfig(RuntimeConfig) before invoke) - should contain just a session_id for now (set by the user before execution of graph)
- Read session_id when initalizing checkpointer and adding that as a column to the SQLx SQLite migration.

Outputs:
- Session stepping without changing Node/Barrier; in-memory checkpoints; conditional routing MVP; basic Rig LLM node.

### Week 2 — SQLite Checkpointer, Interrupts, Streaming Events, Error Channel

Goals:
- Add `SQLiteCheckpointer` (SQLx, feature `SQLite`), with migration for a `checkpoints` table.
- Implement interrupts thoroughly (before/after/per-step) with persisted pause reasons.
- Add event bus and stream node/step events; integrate token streaming from Rig when available.
- Add `ChannelType::Error` + reducer; runner retry policy hooks.
And then to be able to resume from checkpoints by reusing that session id

Tasks:
- `src/runtime/checkpointer_SQLite.rs` (feature `SQLite`) with `connect(url)`, `save/load/list`, and schema migration.
- Interrupt semantics in `AppRunner` (return `Paused` report with context: step, frontier, node name, reason).
- `src/runtime/events.rs`: event types + default sink; publish during scheduling and barrier.
- Extend `LlmChatNode` to stream tokens to events; final append to `messages` at barrier remains via `NodePartial`.
- Add error channel + reducer; wire `Result<NodePartial>` from node execution to capture and reduce errors if a node fails; expose retry policy via runner options.
- Example: `examples/interrupts_and_streaming.rs` demonstrating `interrupt_before` and token streaming to a simple print sink.

Outputs:
- Durable DB checkpoints; ergonomic interrupts; streaming events; error recording and basic retry hooks.

### Week 3 — RAG Track: Chunkers, Embeddings, Vector Store, Example

Goals:
- Smart HTML/Markdown chunkers; embedder abstraction (Rig); in‑memory vector store.
- RAG graph: Query → Retrieve → LLM answer (with streaming) → End.

Tasks:
- `src/rag/chunk/html.rs` and `markdown.rs` with unit tests (code+text adjacency preserved, token‑budget heuristic).
- `src/rag/embed.rs` with a simple embed API (choose model/provider via Rig) and `src/rag/vector.rs` for an in‑memory cosine store.
- Nodes:
  - `EmbedDocNode`: chunk → embed → store; writes summaries/ids to `extra`.
  - `RetrieveNode`: embed query → search → write `retrieved_docs` to `extra`.
  - Reuse `LlmChatNode` to answer using retrieved docs included in prompt.
- Example: `examples/rag_demo.rs` ingest + query; shows streaming tokens and barrier‑committed final answer.

Outputs:
- Usable RAG pipeline consistent with our Node/Barrier model.

---

## 6) Minimal API/Code Touchpoints

- Keep `src/app.rs`, `src/schedulers/*`, `src/reducers/*`, `src/channels/*`, `src/node.rs` intact for core semantics; factor out a `run_one_superstep` helper if needed for `AppRunner`.
- Add under `src/runtime/`: `runner.rs`, `checkpointer.rs`, `checkpointer_postgres.rs` (feat `postgres`), `events.rs`.
- Add under `src/llm/`: `rig_adapter.rs`, `nodes/llm_chat.rs`.
- Add under `src/rag/`: `chunk/{html,markdown}.rs`, `embed.rs`, `vector.rs`, `nodes/{embed_doc,retrieve}.rs`.
- Extend `GraphBuilder` and `App` to support conditional edges (no breaking changes to existing demos).

---

## 7) Validation

- Unit tests: conditional edges, in‑memory checkpoints, Postgres checkpoints (feature‑gated), error channel reducer, chunkers.
- Examples: basic chat, interrupts+streaming, RAG demo — all stepwise via `AppRunner`.
- Tracing output to verify node timings and updated channels; (optional) export graph to Mermaid for docs.

---

## 8) Light Competitive Note (context only)

We intentionally avoid mutating shared state inside nodes or introducing Task/NextAction. Our barriered reducer model yields deterministic merges and parallel supersteps — closer to LangGraph’s spirit. Where others use immediate state mutation or per‑task control signals, we keep control in the runner and commit via barrier, which scales better and is easier to reason about.

---

## 9) Quick “Missing Features” Checklist

- Checkpointer (in‑memory + Postgres), sessions, `AppRunner` for stepwise execution
- Interrupts before/after nodes and per‑step
- Conditional edges
- Rig LLM node (with streaming), tool nodes
- Event bus for streaming updates
- Error channel + retry hooks
- RAG: chunkers, embeddings, vector store, example

This plan keeps Graft’s core intact and implements LangGraph’s most‑used production features in a way that fits our model and raises determinism and observability as first‑class differentiators.

---

## 10) LangGraph Alignment: AppRunner Semantics


- LangGraph checkpoints at node boundaries and supports interrupts before/after specific nodes. Execution effectively progresses node-by-node, withdurability at those boundaries.
- Graft’s barrier model executes a frontier (potentially multiple nodes) then commits via a single barrier. To align with LangGraph, we keep barrie‑level checkpoints by default and add node‑level interrupt controls that gate scheduling so we can pause/resume around a particular node whendesired.
- AppRunner therefore supports two aligned modes without changing Node semantics:
  1) Superstep stepping (default): maximal parallelism; checkpoint after barrier (closest to today’s engine).
  2) Node‑focused stepping (optional): when an interrupt is configured for a frontier node, the runner can gate the scheduler to run just that node(serialize a chosen node) while deferring others; this yields LangGraph‑like node granularity where needed.
- Both modes retain the core property that state commits only at barriers; node‑level pausing happens by controlling which nodes are allowed toexecute in the next superstep.

Result: we remain faithful to LangGraph’s human‑in‑the‑loop semantics while preserving Graft’s deterministic barrier model.

---

## 11) Conditional Edges: Detailed Design

Goals: mirror LangGraph’s `add_conditional_edges` while keeping deterministic frontier computation and barrier semantics.

- API options (all can coexist):
  - Boolean form (yes/no):
    `add_conditional_edge(from, yes, no, predicate: Arc<dyn Fn(&StateSnapshot) -> bool + Send + Sync + 'static>)`
  - Direct target form:
    `add_router_node(from, router: Arc<dyn Fn(&StateSnapshot) -> NodeKind + Send + Sync + 'static>)`

- Implementation: extend internal adjacency with a `Vec<ConditionalEdge>` alongside unconditional `Vec<NodeKind>`. Preserve insertion order to ensure deterministic evaluation.

- Evaluation point: after barrier application, when building the next frontier for nodes that ran:
  For each executed node, in stable order:
  - push all unconditional edges;
  - evaluate conditional entries against the fresh `StateSnapshot`:
    - Bool: choose yes/no;
    - Direct: use returned `NodeKind`.
  De‑duplicate while preserving first occurrence order. Cycles are allowed.

- Determinism: insertion order of conditionals is preserved; final frontier is order‑stable and unique.

- Safety: predicates/routers are `'static + Send + Sync` so they can be stored and used across async boundaries. Capture config via owned clones; avoid borrowing non‑`'static` state.

- Tests: yes/no branching, multiple conditionals on one node, deterministic ordering, cycles, and de‑dup.

This matches LangGraph’s conditional routing semantics while integrating cleanly with barriered next‑frontier computation.

---

## 12) Interrupts: Detailed Design (LangGraph‑matching)

Semantics to mirror LangGraph’s `interrupt_before` / `interrupt_after` and general human‑in‑the‑loop:

- Configuration on AppRunner:
  - `interrupt_before: Vec<NodeKind>` — if any appear in the current frontier, pause before executing.
  - `interrupt_after: Vec<NodeKind>` — if any executed in the last step, pause after executing.
  - `interrupt_each_step: bool` — always pause after a barrier (step‑wise human review).
  - Optional `serial_nodes: Vec<NodeKind>` — if specified, when such a node is in the frontier, schedule only that node (gate others) to approximate node‑granular stepping around interrupts.

- Behavior and ordering:
  1) Load checkpoint; compute current frontier.
  2) If any `interrupt_before` node ∈ frontier, return `Paused { kind: BeforeNode, node, frontier, step }` without executing; save checkpoint (idempotent).
  3) Otherwise run one superstep (possibly gated by `serial_nodes`).
  4) Apply barrier; compute next frontier; save checkpoint.
  5) If any `interrupt_after` node ∈ ran_nodes, return `Paused { kind: AfterNode, node, step }`.
  6) If `interrupt_each_step`, return `Paused { kind: StepBoundary }`.
  7) Else return `Continued { step, next_frontier, updated_channels }`.

- Human input path:
  - UI or caller edits state before resuming (e.g., append a user `Message` to the messages channel or adjust `extra`).
  - Resume by calling `run_step(session_id, ...)` again; runner uses the checkpoint to continue.

- Events:
  - Publish `Paused`, `NodeStart`, `NodeFinish`, `BarrierApplied`, and (for LLM nodes) `TokenChunk` to the event sink so UIs can react in real time.

- Determinism & atomicity:
  - Interrupts are recorded and persisted before returning; resumption continues from a known checkpoint.
  - Even with `serial_nodes` gating, committed state changes still occur only via barriers.

This gives us LangGraph‑like human‑in‑the‑loop control while honoring our superstep model and deterministic barrier commits.

---

## 13) Recommended Future Work (Beyond 3 Weeks)

- Subgraphs and hierarchy: named subgraphs, reusable components, nested barrier scopes, and subgraph inlining/expansion.
- Dynamic graphs: runtime edge/node enablement, router rewrites, and policy‑driven graph mutations.
- Typed and dynamic channels: register new channel kinds at runtime; CRDT reducers; dedup/top‑K reducers; retention (TTL, capacity‑based, snapshot compaction).
- Distributed scheduling: pluggable queue backends (SQS/Kafka/NATS/Redis) with idempotent node execution; worker pools; shardable sessions.
- Resource policies: per‑node budgets, rate limits, concurrency caps, and backoff strategies; cancellation and deadlines.
- Advanced checkpointing: event‑sourced logs, differential checkpoints, encryption at rest, multi‑tenancy isolation, and snapshot pinning for audits.
- Observability: OpenTelemetry tracing/metrics/logs; timeline exports; DOT/Mermaid/interactive visualizer; LangSmith‑style adapters.
- Safety & tool calling: JSON‑schema tool I/O validation; strict function calling; Pydantic/Rust type bridges; red‑team guardrails; content filtering reducers.
- Agentic patterns: reflection/critic loops; plan/act/reflect nodes expressed via messages+reducers rather than control codes.
- Memory systems: episodic/summarized memory nodes; memory TTL and summarization reducers; knowledge graph channels.
- Caching: snapshot‑keyed and prompt‑keyed caches; partial result memoization.
- Multi‑user sessions: collaborative editing with merge policies; optimistic locking at channel level.
- SDKs and tooling: CLI to run/inspect/resume flows; codegen for graphs from DOT/Mermaid; studio UI.

These items deepen parity with LangGraph’s ecosystem and differentiate Graft with stronger determinism, scale, and developer ergonomics.

# Weavegraph

- Bounded-concurrency scheduler with version gating (superstep barrier model)
- Strong, typed error propagation across nodes, scheduler, and runner (thiserror + miette)
- Rich tracing spans (`tracing`) and pretty diagnostics (`miette`)
- Optional checkpointing via SQLite (or in-memory)

## Repo layout (key modules)

- `src/run_demo{1,2,3}.rs` – Demos showing increasing sophistication
- `src/main.rs` – CLI to select which demo to run

## Running the demos (CLI)
+ use weavegraph::graph::GraphBuilder;
Select a demo at runtime (default is `demo3`):

```bash
cargo run -- demo3
```

What the demos showcase:

- `demo1`: Basic graph execution via `App::invoke`, manual barrier examples, version bumps
- `demo2`: Direct scheduler usage (`superstep`), ran/skipped nodes, barrier application each step
- `demo3`: LLM-style nodes using Rig/Ollama, conditional edges, SQLite checkpointing, and error pretty-printing

Notes for `demo3`:

- Provider: Expects Ollama running at `http://localhost:11434` and the referenced models (e.g., `gemma3`, `gemma3:270m`). If unavailable, the demo will fail gracefully and emit a structured error; you’ll see a pretty-printed error ladder.
- Checkpointing: Uses SQLite by default. The DB URL is resolved in this order:
  - `WEAVEGRAPH_SQLITE_URL` (e.g., `sqlite://weavegraph.db`)
  - `sqlite_db_name` set in code
  - `SQLITE_DB_NAME` env var (filename only)
  - Fallback: `sqlite://weavegraph.db`

  ## Examples (standalone)

  Run the small example that prints prettified error events to the CLI:

  ```bash
  cargo run --example errors_pretty -q
  ```

## Build, test, and logs

Build and test:

```bash
cargo build
cargo test --all -- --nocapture
```

Tracing and logs:

- The default filter is `info,weavegraph=debug`. Override with `RUST_LOG`:

```bash
RUST_LOG=debug cargo run -- demo2
```

## Event bus streaming

- `AppRunner` wires an event bus that streams node/lifecycle messages to stdout; `listen_for_events()` spins the default listener.
- To plug in custom sinks (e.g., telemetry dashboards), construct an `EventBus`, configure listeners yourself, and pass it into `AppRunner::with_options_and_bus` (or the `*_arc` variant). Set `start_listener = false` if you want to manage listener tasks manually.
- Use helpers like `EventBus::with_sink(MemorySink::new())` for test harnesses or `EventBus::with_sink_and_formatter` to pair a custom sink with a formatter; the default `StdOutSink` flushes each line for responsive streaming.
- Call `EventBus::stop_listener().await` when shutting down long-lived services to flush outstanding messages and end the background task cleanly.

## Error handling and diagnostics

- Nodes return `Result<NodePartial, NodeError>`
- Scheduler returns `Result<StepRunResult, SchedulerError>`; node failures include context (`kind`, `step`)
- Runner surfaces `RunnerError` and, on failure, emits an `ErrorEvent` into the dedicated errors channel via `NodePartial.errors`
- Demos print a human-friendly error ladder via `errors::pretty_print` at the end
- Binaries return `miette::Result`, so typed errors render nicely without extra glue code

## Persistence

- In-memory and SQLite checkpointing are available; demos 1–2 run in-memory; demo 3 uses SQLite by default.
- The runner will attempt to create the SQLite file if it doesn’t exist.

## Contributing

Contributions are welcome! See `doc/` for design notes and plans. PRs with tests, tracing, or improved diagnostics are especially appreciated.

## License

MIT

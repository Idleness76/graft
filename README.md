# Graft

A Rust-based framework for building graph-driven, concurrent agent workflows—an alternative to Python’s LangGraph, with a focus on explicit state, deterministic execution, and extensibility.

## Overview

Graft provides a minimal, extensible runtime for orchestrating nodes (agents) over a directed graph, with versioned state channels and deterministic barrier merges. It’s designed for LLM and tool orchestration, but general enough for broader dataflow and actor-style applications.

Dream final state architectural flow chart
```mermaid

flowchart TB

subgraph Client
  user[Client App or UI]
end

subgraph Build
  gb[GraphBuilder]
end

subgraph Graph
  cg[CompiledGraph]
end

subgraph Runtime
  app[App]
  sched[Scheduler]
  router[Router Edges and Commands]
  barrier[Barrier Applier]
end

subgraph Nodes
  usernode[User Nodes]
  llmnode[LLM Node]
  toolnode[Tool Node]
end

subgraph State
  vstate[Versioned State]
  snap[State Snapshot]
end

subgraph Reducers
  redreg[Reducer Registry]
end

subgraph Checkpoint
  cpif[Checkpointer]
end

subgraph Rig
  rigad[Rig Adapter]
  llmprov[LLM Provider]
end

subgraph Tools
  toolreg[Tool Registry]
  exttools[External Tools]
end

subgraph Stream
  stream[Stream Controller]
end

subgraph Viz
  viz[Visualizer]
end


user --> gb
gb --> cg

user --> app
cg --> app

app --> sched
sched --> snap
vstate --> snap

sched --> usernode
sched --> llmnode
sched --> toolnode

usernode --> barrier
llmnode --> barrier
toolnode --> barrier
redreg --> barrier
barrier --> vstate

snap --> router
app --> router
router --> sched

llmnode --> rigad
rigad --> llmprov
llmprov --> rigad
rigad --> llmnode

toolnode --> toolreg
toolnode --> exttools
exttools --> toolnode

barrier --> cpif

app --> stream
stream --> user

cg --> viz

```

## Features

- **Versioned, channelized state** (messages, outputs, meta)
- **Pluggable reducers** for deterministic state merges
- **Parallel node execution** with superstep/barrier model
- **Extensible graph topology** (nodes, edges)
- **Async execution** via Tokio

## Quick Start

...coming

## Documentation

- See [`doc/iteration1_demo1.md`](doc/iteration1_demo1.md) for a detailed architecture overview, roadmap, and design principles.
- The documentation folder contains design notes, glossary, roadmap, and open questions to guide contributors and future development.
- Example code and usage patterns are described in the documentation, but are not runnable as-is.

## Roadmap

- Dynamic channel registry
- Structured error handling
- Logging & tracing
- Graph validation
- Real-world node examples (LLM, tools, etc.)

## Contributing

Contributions and feedback are welcome! Please review the open questions in the demo documentation and help shape the direction of the project.

## License

MIT

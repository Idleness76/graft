// src/main.rs
use async_trait::async_trait;
use futures::future::join_all;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

mod node;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, Error>;

/**************
  Domain types
**************/

#[derive(Clone, Debug)]
struct Message {
    role: String,
    content: String,
}

/************************
  Reducers and contracts
************************/

trait Reducer<V, U>: Send + Sync {
    fn apply(&self, value: &mut V, update: U);
}

// 1) Append messages
struct AddMessages;
impl Reducer<Vec<Message>, Vec<Message>> for AddMessages {
    fn apply(&self, value: &mut Vec<Message>, update: Vec<Message>) {
        value.extend(update);
    }
}

// 2) Append vector for outputs
struct AppendVec<T>(std::marker::PhantomData<T>);
impl<T: Clone + Send + Sync + 'static> Reducer<Vec<T>, Vec<T>> for AppendVec<T> {
    fn apply(&self, value: &mut Vec<T>, update: Vec<T>) {
        value.extend(update);
    }
}

// 3) Shallow map merge for meta
struct MapMerge;
impl Reducer<HashMap<String, String>, HashMap<String, String>> for MapMerge {
    fn apply(&self, value: &mut HashMap<String, String>, update: HashMap<String, String>) {
        for (k, v) in update {
            value.insert(k, v);
        }
    }
}

/***********************
  Versioned state model
***********************/

#[derive(Clone, Debug)]
struct Versioned<T> {
    value: T,
    version: u64,
}

#[derive(Clone, Debug)]
struct VersionedState {
    messages: Versioned<Vec<Message>>,
    outputs: Versioned<Vec<String>>,
    meta: Versioned<HashMap<String, String>>,
}

impl VersionedState {
    fn new_with_user_message(user_text: &str) -> Self {
        Self {
            messages: Versioned {
                value: vec![Message {
                    role: "user".into(),
                    content: user_text.into(),
                }],
                version: 1,
            },
            outputs: Versioned {
                value: Vec::new(),
                version: 1,
            },
            meta: Versioned {
                value: HashMap::new(),
                version: 1,
            },
        }
    }

    fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            messages: self.messages.value.clone(),
            messages_version: self.messages.version,
            outputs: self.outputs.value.clone(),
            outputs_version: self.outputs.version,
            meta: self.meta.value.clone(),
            meta_version: self.meta.version,
        }
    }
}

#[derive(Clone, Debug)]
struct StateSnapshot {
    messages: Vec<Message>,
    messages_version: u64,
    outputs: Vec<String>,
    outputs_version: u64,
    meta: HashMap<String, String>,
    meta_version: u64,
}

/*****************
  Node abstraction
******************/

#[derive(Clone, Debug)]
struct NodeContext {
    node_id: String,
    step: u64,
}

#[derive(Clone, Debug, Default)]
struct NodePartial {
    // Per-channel partials (all optional)
    messages: Option<Vec<Message>>,
    outputs: Option<Vec<String>>,
    meta: Option<HashMap<String, String>>,
}

#[async_trait]
trait Node: Send + Sync {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> NodePartial;
}

/****************
  Node examples - will be created by users
*****************/

struct NodeA;
#[async_trait]
impl Node for NodeA {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> NodePartial {
        let seen_msgs = snapshot.messages.len();
        let content = format!("A saw {} msgs at step {}", seen_msgs, ctx.step);

        // NodeA writes to messages and meta, but not outputs
        let mut meta = HashMap::new();
        meta.insert("source".into(), "A".into());
        meta.insert("hint".into(), "alpha".into());

        NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content,
            }]),
            outputs: None,
            meta: Some(meta),
        }
    }
}

struct NodeB;
#[async_trait]
impl Node for NodeB {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> NodePartial {
        let seen_outs = snapshot.outputs.len();
        let content = format!("B adding output, prior outs={}", seen_outs);

        // NodeB writes to outputs and meta, and also a small message
        let mut meta = HashMap::new();
        meta.insert("tag".into(), "beta".into());

        NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: format!("B ran at step {}", ctx.step),
            }]),
            outputs: Some(vec![content]),
            meta: Some(meta),
        }
    }
}

/************************
  Graph and compilation
************************/

const START: &str = "__start__";
const END: &str = "__end__";

struct GraphBuilder {
    nodes: HashMap<String, Arc<dyn Node>>,
    edges: HashMap<String, Vec<String>>,
}

impl GraphBuilder {
    fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
        }
    }

    fn add_node(mut self, id: &str, node: impl Node + 'static) -> Self {
        self.nodes.insert(id.to_string(), Arc::new(node));
        self
    }

    fn add_edge(mut self, from: &str, to: &str) -> Self {
        self.edges
            .entry(from.to_string())
            .or_default()
            .push(to.to_string());
        self
    }

    fn compile(self) -> App {
        App {
            nodes: self.nodes,
            edges: self.edges,
            add_messages: Arc::new(AddMessages),
            append_outputs: Arc::new(AppendVec::<String>(Default::default())),
            map_merge: Arc::new(MapMerge),
        }
    }
}

/***********
  The App
***********/

struct App {
    nodes: HashMap<String, Arc<dyn Node>>,
    edges: HashMap<String, Vec<String>>,
    // Reducers per channel
    add_messages: Arc<AddMessages>,
    append_outputs: Arc<AppendVec<String>>,
    map_merge: Arc<MapMerge>,
}

impl App {
    async fn invoke(&self, initial_state: VersionedState) -> Result<VersionedState> {
        let state = Arc::new(RwLock::new(initial_state));
        let mut step: u64 = 0;

        // Run A and B in parallel from START, both go to END
        let mut frontier: Vec<String> = self.edges.get(START).cloned().unwrap_or_default();
        if frontier.is_empty() {
            return Err("No nodes to run from START".into());
        }

        println!("== Begin run ==");

        loop {
            step += 1;

            // Stop if only END remains
            let only_end = frontier.iter().all(|n| n.as_str() == END);
            if only_end {
                println!("Reached END at step {}", step);
                break;
            }

            // Snapshot
            let snapshot = { state.read().await.snapshot() };
            println!(
                "\n-- Superstep {} --\nmsgs={} v{}; outs={} v{}; meta_keys={} v{}",
                step,
                snapshot.messages.len(),
                snapshot.messages_version,
                snapshot.outputs.len(),
                snapshot.outputs_version,
                snapshot.meta.len(),
                snapshot.meta_version
            );

            // Execute frontier (parallel)
            let mut run_ids: Vec<String> = Vec::new();
            let mut futs = Vec::new();
            for node_id in frontier.iter() {
                if node_id == END {
                    continue;
                }
                let id = node_id.clone();
                run_ids.push(id.clone());
                let node = self.nodes.get(&id).unwrap().clone();
                let ctx = NodeContext {
                    node_id: id.clone(),
                    step,
                };
                let snap = snapshot.clone();
                futs.push(async move { node.run(snap, ctx).await });
            }

            let node_partials: Vec<NodePartial> = join_all(futs).await;

            // Barrier: bucket updates per channel and merge deterministically
            let updated_channels = self.apply_barrier(&state, &run_ids, node_partials).await?;

            // Compute next frontier
            let mut next = Vec::<String>::new();
            for id in run_ids.iter() {
                if let Some(dests) = self.edges.get(id) {
                    for d in dests {
                        if !next.contains(d) {
                            next.push(d.clone());
                        }
                    }
                }
            }

            println!("Updated channels this step: {:?}", updated_channels);
            println!("Next frontier: {:?}", next);

            if next.is_empty() {
                println!("No outgoing edges; terminating.");
                break;
            }

            frontier = next;
        }

        println!("\n== Final state ==");
        let final_state = Arc::try_unwrap(state).expect("borrowed").into_inner();

        for (i, m) in final_state.messages.value.iter().enumerate() {
            println!("#{:02} [{}] {}", i, m.role, m.content);
        }
        println!("messages.version = {}", final_state.messages.version);

        println!("outputs (v {}):", final_state.outputs.version);
        for (i, o) in final_state.outputs.value.iter().enumerate() {
            println!("  - {:02}: {}", i, o);
        }

        println!(
            "meta (v {}): {:?}",
            final_state.meta.version, final_state.meta.value
        );

        Ok(final_state)
    }

    async fn apply_barrier(
        &self,
        state: &Arc<RwLock<VersionedState>>,
        run_ids: &[String],
        node_partials: Vec<NodePartial>,
    ) -> Result<Vec<&'static str>> {
        // Aggregate per-channel updates first (efficient and deterministic).
        let mut msgs_all: Vec<Message> = Vec::new();
        let mut outs_all: Vec<String> = Vec::new();
        let mut meta_all: HashMap<String, String> = HashMap::new();

        for (i, p) in node_partials.iter().enumerate() {
            let nid = run_ids.get(i).unwrap_or(&"?".into());
            if let Some(ms) = &p.messages {
                println!("  {} -> messages: +{}", nid, ms.len());
                msgs_all.extend(ms.clone());
            }
            if let Some(os) = &p.outputs {
                println!("  {} -> outputs: +{}", nid, os.len());
                outs_all.extend(os.clone());
            }
            if let Some(mm) = &p.meta {
                println!("  {} -> meta: +{} keys", nid, mm.len());
                for (k, v) in mm {
                    meta_all.insert(k.clone(), v.clone());
                }
            }
        }

        // Apply per-channel reducers and bump versions if changed.
        let mut updated = Vec::<&'static str>::new();
        let mut s = state.write().await;

        // messages
        if !msgs_all.is_empty() {
            let before_len = s.messages.value.len();
            let before_ver = s.messages.version;
            self.add_messages.apply(&mut s.messages.value, msgs_all);
            let changed = s.messages.value.len() != before_len;
            if changed {
                s.messages.version = s.messages.version.saturating_add(1);
                println!(
                    "  barrier: messages len {} -> {}, v {} -> {}",
                    before_len,
                    s.messages.value.len(),
                    before_ver,
                    s.messages.version
                );
                updated.push("messages");
            }
        }

        // outputs
        if !outs_all.is_empty() {
            let before_len = s.outputs.value.len();
            let before_ver = s.outputs.version;
            self.append_outputs.apply(&mut s.outputs.value, outs_all);
            let changed = s.outputs.value.len() != before_len;
            if changed {
                s.outputs.version = s.outputs.version.saturating_add(1);
                println!(
                    "  barrier: outputs len {} -> {}, v {} -> {}",
                    before_len,
                    s.outputs.value.len(),
                    before_ver,
                    s.outputs.version
                );
                updated.push("outputs");
            }
        }

        // meta (map merge)
        if !meta_all.is_empty() {
            let before_keys = s.meta.value.len();
            let before_ver = s.meta.version;
            self.map_merge.apply(&mut s.meta.value, meta_all);
            let changed = s.meta.value.len() != before_keys;
            if changed {
                s.meta.version = s.meta.version.saturating_add(1);
                println!(
                    "  barrier: meta keys {} -> {}, v {} -> {}",
                    before_keys,
                    s.meta.value.len(),
                    before_ver,
                    s.meta.version
                );
                updated.push("meta");
            }
        }

        Ok(updated)
    }
}

/*********
  Driver
**********/

#[tokio::main]
async fn main() -> Result<()> {
    // Graph:
    //   START -> A
    //   START -> B
    //   A -> END
    //   B -> END
    //
    // A and B run together in one superstep, each updating different
    // channels. The barrier merges per-channel updates via the proper
    // reducer and bumps versions only when values actually change.

    let app = GraphBuilder::new()
        .add_node("A", NodeA)
        .add_node("B", NodeB)
        .add_edge(START, "A")
        .add_edge(START, "B")
        .add_edge("A", END)
        .add_edge("B", END)
        .compile();

    let initial = VersionedState::new_with_user_message("Hello!");
    let _final_state = app.invoke(initial).await?;
    Ok(())
}

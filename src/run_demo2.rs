use rustc_hash::FxHashMap;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::channels::Channel;

use super::graph::GraphBuilder;
use super::node::{NodeA, NodeB, NodePartial};
use super::schedulers::{Scheduler, StepRunResult};
use super::state::VersionedState;
use super::types::NodeKind;

/// Demonstration run showcasing:
/// 1. Building and executing a small multi-step graph using Scheduler
/// 2. Inspecting StepRunResult (ran/skipped/outputs)
/// 3. Manual concurrency control and version gating
/// 4. Barrier application using StepRunResult outputs
pub async fn run_demo2() -> anyhow::Result<()> {
    println!("\n==============================");
    println!("== Demo2: Scheduler Runtime ==");
    println!("==============================\n");

    // 1. Initial state with a user message + seeded extra data
    let mut init = VersionedState::new_with_user_message("Hello Scheduler");
    init.extra
        .get_mut()
        .insert("numbers".into(), json!([4, 5, 6]));
    init.extra
        .get_mut()
        .insert("info".into(), json!({"stage": "init2"}));

    // 2. Build a richer graph with some fan-out and re-visits:
    //    Start -> A, Start -> B, A -> B, B -> End
    //    This ensures Step 1 runs both A and B concurrently; Step 2 runs B again due to A's output.
    let app = GraphBuilder::new()
        .add_node(NodeKind::Start, NodeA)
        .add_node(NodeKind::Other("A".into()), NodeA)
        .add_node(NodeKind::Other("B".into()), NodeB)
        .add_node(NodeKind::End, NodeB)
        .add_edge(NodeKind::Start, NodeKind::Other("A".into()))
        .add_edge(NodeKind::Start, NodeKind::Other("B".into()))
        .add_edge(NodeKind::Other("A".into()), NodeKind::Other("B".into()))
        .add_edge(NodeKind::Other("B".into()), NodeKind::End)
        .set_entry(NodeKind::Start)
        .compile()
        .map_err(|e| anyhow::Error::msg(format!("{:?}", e)))?;

    // 3. Prepare scheduler with explicit concurrency limit
    let mut scheduler = Scheduler::new(2); // Try changing to 1 for serial demo
    let state = Arc::new(RwLock::new(init));
    let mut step: u64 = 0;
    let mut frontier: Vec<NodeKind> = app
        .edges()
        .get(&NodeKind::Start)
        .cloned()
        .unwrap_or_default();

    // Tracking totals for a recap at the end
    let mut ran_counts: FxHashMap<String, u32> = FxHashMap::default();
    let mut skipped_counts: FxHashMap<String, u32> = FxHashMap::default();

    println!("== Begin scheduler run ==");
    println!("Using concurrency_limit = {}", scheduler.concurrency_limit);
    loop {
        step += 1;
        if frontier.iter().all(|n| *n == NodeKind::End) {
            println!("Reached END at step {}", step);
            break;
        }
        // Take a consistent view of state for the whole superstep.
        let snapshot = { state.read().await.snapshot() };

        // Pretty header for the superstep with snapshot versions.
        println!("\n────────────────────────────────");
        println!(
            "Superstep {:>2} | messages: {:>3} (v {:>3}) | extra keys: {:>3} (v {:>3})",
            step,
            snapshot.messages.len(),
            snapshot.messages_version,
            snapshot.extra.len(),
            snapshot.extra_version
        );
        println!("Current frontier: {:?}", frontier);

        // Decide which nodes to run BEFORE calling the scheduler (so gating isn't affected by record_seen).
        let run_ids_pre: Vec<NodeKind> = frontier
            .iter()
            .filter(|n| !matches!(n, &NodeKind::End))
            .cloned()
            .filter(|n| scheduler.should_run(&format!("{:?}", n), &snapshot))
            .collect();
        let run_id_strs: Vec<String> = run_ids_pre.iter().map(|k| format!("{:?}", k)).collect();
        println!("Planned to run (pre-gated): {:?}", run_id_strs);

        // Use scheduler to run the frontier
        let step_result: StepRunResult = scheduler
            .superstep(app.nodes(), frontier.clone(), snapshot.clone(), step)
            .await;
        // Update counters and print high-level result
        for id in &step_result.ran_nodes {
            *ran_counts.entry(id.clone()).or_insert(0) += 1;
        }
        for id in &step_result.skipped_nodes {
            *skipped_counts.entry(id.clone()).or_insert(0) += 1;
        }
        println!(
            "StepRunResult:\n  - ran_nodes:    {:?}\n  - skipped_nodes:{:?}",
            step_result.ran_nodes, step_result.skipped_nodes
        );

        // Explain skip reasons: End vs gated-by-versions
        let end_skips: Vec<String> = frontier
            .iter()
            .filter(|n| matches!(n, NodeKind::End))
            .map(|_| "End".to_string())
            .collect();
        let planned_set: std::collections::HashSet<_> = run_id_strs.iter().cloned().collect();
        let frontier_non_end: Vec<String> = frontier
            .iter()
            .filter(|n| !matches!(n, NodeKind::End))
            .map(|k| format!("{:?}", k))
            .collect();
        let gated_skips: Vec<String> = frontier_non_end
            .into_iter()
            .filter(|id| !planned_set.contains(id))
            .collect();
        if !end_skips.is_empty() || !gated_skips.is_empty() {
            println!("Skip reasons:");
            if !end_skips.is_empty() {
                println!("  - End nodes:    {:?}", end_skips);
            }
            if !gated_skips.is_empty() {
                println!("  - Version-gated: {:?}", gated_skips);
            }
        }
        if step_result.outputs.is_empty() {
            println!("No outputs this step.");
        } else {
            println!("Node outputs:");
            for (id, partial) in &step_result.outputs {
                let msg_count = partial.messages.as_ref().map(|v| v.len()).unwrap_or(0);
                let extra_count = partial.extra.as_ref().map(|m| m.len()).unwrap_or(0);
                println!(
                    "  - {:<12} | messages: {:>2} | extra keys: {:>2}",
                    id, msg_count, extra_count
                );
            }
        }

        // Apply barrier using precomputed run_ids and outputs (reordered to match run_ids).
        let mut by_id: FxHashMap<String, NodePartial> = FxHashMap::default();
        for (id, part) in step_result.outputs {
            by_id.insert(id, part);
        }
        let node_partials: Vec<NodePartial> = run_ids_pre
            .iter()
            .map(|k| format!("{:?}", k))
            .filter_map(|id| by_id.remove(&id))
            .collect();
        let updated_channels = app
            .apply_barrier(&state, &run_ids_pre, node_partials)
            .await
            .map_err(|e| anyhow::Error::msg(e.to_string()))?;
        println!("Barrier updated channels: {:?}", updated_channels);
        // Show versions_seen for nodes that ran (pre-barrier snapshot versions recorded by scheduler)
        if !run_ids_pre.is_empty() {
            println!("versions_seen after run:");
            for k in &run_ids_pre {
                let id = format!("{:?}", k);
                if let Some(map) = scheduler.versions_seen.get(&id) {
                    let mv = map.get("messages").copied().unwrap_or(0);
                    let ev = map.get("extra").copied().unwrap_or(0);
                    println!("  - {:<12} | messages v {:>3} | extra v {:>3}", id, mv, ev);
                } else {
                    println!("  - {:<12} | (no versions recorded)", id);
                }
            }
        }

        // Compute next frontier
        let mut next: Vec<NodeKind> = Vec::new();
        for id in run_ids_pre.iter() {
            if let Some(dests) = app.edges().get(id) {
                for d in dests {
                    if !next.contains(d) {
                        next.push(d.clone());
                    }
                }
            }
        }
        println!("Next frontier: {:?}", next);
        if next.is_empty() {
            println!("No outgoing edges; terminating.");
            break;
        }
        frontier = next;
    }

    println!("\n== Final state ==");
    let final_state = Arc::try_unwrap(state)
        .expect("state still borrowed")
        .into_inner();
    for (i, m) in final_state.messages.snapshot().iter().enumerate() {
        println!("#{:02} [{}] {}", i, m.role, m.content);
    }
    println!("messages.version = {}", final_state.messages.version());
    let extra_snapshot = final_state.extra.snapshot();
    println!(
        "extra (v {}) keys={}",
        final_state.extra.version(),
        extra_snapshot.len()
    );
    for (k, v) in extra_snapshot.iter() {
        println!("  {k}: {v}");
    }
    println!("== Demo2 complete ==");
    // Recap totals
    println!("\nRecap: node run/skip counts");
    // union of keys
    let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for k in ran_counts.keys() {
        keys.insert(k.clone());
    }
    for k in skipped_counts.keys() {
        keys.insert(k.clone());
    }
    for k in keys {
        let r = ran_counts.get(&k).copied().unwrap_or(0);
        let s = skipped_counts.get(&k).copied().unwrap_or(0);
        println!("  - {:<12} | ran {:>2} | skipped {:>2}", k, r, s);
    }
    Ok(())
}

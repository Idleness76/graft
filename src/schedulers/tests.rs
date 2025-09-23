use super::scheduler::{Scheduler, SchedulerState, StepRunResult};
use crate::node::{Node, NodeContext, NodeError, NodePartial};
use crate::state::StateSnapshot;
use crate::types::NodeKind;
use async_trait::async_trait;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tokio::time::{Duration, sleep};

// A minimal test node that returns a marker message to validate execution.
struct TestNode {
    name: &'static str,
}

#[async_trait]
impl Node for TestNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        Ok(NodePartial {
            messages: Some(vec![crate::message::Message {
                role: "assistant".into(),
                content: format!("ran:{}:step:{}", self.name, ctx.step),
            }]),
            extra: None,
            errors: None,
        })
    }
}

// A node that sleeps for a configured duration to induce completion reordering.
struct DelayedNode {
    name: &'static str,
    delay_ms: u64,
}

#[async_trait]
impl Node for DelayedNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        sleep(Duration::from_millis(self.delay_ms)).await;
        Ok(NodePartial {
            messages: Some(vec![crate::message::Message {
                role: "assistant".into(),
                content: format!("ran:{}:step:{}", self.name, ctx.step),
            }]),
            extra: None,
            errors: None,
        })
    }
}

fn make_registry() -> FxHashMap<NodeKind, Arc<dyn Node>> {
    let mut m: FxHashMap<NodeKind, Arc<dyn Node>> = FxHashMap::default();
    m.insert(
        NodeKind::Other("A".into()),
        Arc::new(TestNode { name: "A" }),
    );
    m.insert(
        NodeKind::Other("B".into()),
        Arc::new(TestNode { name: "B" }),
    );
    m.insert(NodeKind::End, Arc::new(TestNode { name: "END" }));
    m
}

// A node that fails immediately to test error propagation.
struct FailingNode;

#[async_trait]
impl Node for FailingNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        _ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        Err(NodeError::MissingInput { what: "test_key" })
    }
}

#[tokio::test]
async fn test_superstep_propagates_node_error() {
    let sched = Scheduler::new(4);
    let mut state = SchedulerState::default();
    let mut nodes: FxHashMap<NodeKind, Arc<dyn Node>> = FxHashMap::default();
    nodes.insert(NodeKind::Other("FAIL".into()), Arc::new(FailingNode));
    let frontier = vec![NodeKind::Other("FAIL".into())];
    let snap = snap_with_versions(1, 1);

    let res = sched.superstep(&mut state, &nodes, frontier, snap, 1).await;
    match res {
        Err(super::scheduler::SchedulerError::NodeRun {
            source: NodeError::MissingInput { what },
            ..
        }) => {
            assert_eq!(what, "test_key");
        }
        other => panic!(
            "expected SchedulerError::NodeRun(MissingInput), got: {:?}",
            other
        ),
    }
}

fn snap_with_versions(msgs_version: u32, extra_version: u32) -> StateSnapshot {
    StateSnapshot {
        messages: vec![],
        messages_version: msgs_version,
        extra: FxHashMap::default(),
        extra_version,
    }
}

#[test]
fn test_should_run_and_record_seen() {
    let sched = Scheduler::new(4);
    let mut state = SchedulerState::default();
    let id = "Other(\"A\")"; // format!("{:?}", NodeKind::Other("A".into()))

    // No record -> should run
    let snap1 = snap_with_versions(1, 1);
    assert!(sched.should_run(&state, id, &snap1));

    // Record seen -> no change -> should not run
    sched.record_seen(&mut state, id, &snap1);
    assert!(!sched.should_run(&state, id, &snap1));

    // Version bump on messages -> should run
    let snap2 = snap_with_versions(2, 1);
    assert!(sched.should_run(&state, id, &snap2));

    // Record bump, then only extra increases -> should run
    sched.record_seen(&mut state, id, &snap2);
    let snap3 = snap_with_versions(2, 3);
    assert!(sched.should_run(&state, id, &snap3));
}

#[tokio::test]
async fn test_superstep_skips_end_and_nochange() {
    let sched = Scheduler::new(8);
    let mut state = SchedulerState::default();
    let nodes = make_registry();
    let frontier = vec![
        NodeKind::Other("A".into()),
        NodeKind::End,
        NodeKind::Other("B".into()),
    ];

    // First run: nothing recorded, both A and B should run; End skipped.
    let snap = snap_with_versions(1, 1);
    let res1: StepRunResult = sched
        .superstep(&mut state, &nodes, frontier.clone(), snap.clone(), 1)
        .await
        .unwrap();
    // All ran except End
    let ran1: std::collections::HashSet<_> = res1.ran_nodes.iter().cloned().collect();
    assert!(ran1.contains(&NodeKind::Other("A".into())));
    assert!(ran1.contains(&NodeKind::Other("B".into())));
    assert!(!ran1.contains(&NodeKind::End));
    assert!(res1.skipped_nodes.contains(&NodeKind::End));
    assert_eq!(res1.outputs.len(), 2);

    // Record_seen happened inside superstep; with same snapshot, nothing should run now.
    let res2 = sched
        .superstep(&mut state, &nodes, frontier.clone(), snap.clone(), 2)
        .await
        .unwrap();
    assert!(res2.ran_nodes.is_empty());
    // Both A and B plus End appear in skipped (version-gated or End)
    let skipped2: std::collections::HashSet<_> = res2.skipped_nodes.iter().cloned().collect();
    assert!(skipped2.contains(&NodeKind::Other("A".into())));
    assert!(skipped2.contains(&NodeKind::Other("B".into())));
    assert!(skipped2.contains(&NodeKind::End));
    assert!(res2.outputs.is_empty());

    // Increase messages version -> A and B should run again
    let snap_bump = snap_with_versions(2, 1);
    let res3 = sched
        .superstep(&mut state, &nodes, frontier.clone(), snap_bump, 3)
        .await
        .unwrap();
    let ran3: std::collections::HashSet<_> = res3.ran_nodes.iter().cloned().collect();
    assert!(ran3.contains(&NodeKind::Other("A".into())));
    assert!(ran3.contains(&NodeKind::Other("B".into())));
    assert_eq!(res3.outputs.len(), 2);
}

#[tokio::test]
async fn test_superstep_outputs_order_agnostic() {
    // Build two nodes with different delays to encourage out-of-order completion.
    let mut nodes: FxHashMap<NodeKind, Arc<dyn Node>> = FxHashMap::default();
    nodes.insert(
        NodeKind::Other("A".into()),
        Arc::new(DelayedNode {
            name: "A",
            delay_ms: 30,
        }),
    );
    nodes.insert(
        NodeKind::Other("B".into()),
        Arc::new(DelayedNode {
            name: "B",
            delay_ms: 1,
        }),
    );

    let frontier = vec![NodeKind::Other("A".into()), NodeKind::Other("B".into())];
    let snap = snap_with_versions(1, 1);
    let sched = Scheduler::new(2);
    let mut state = SchedulerState::default();

    let res = sched
        .superstep(&mut state, &nodes, frontier.clone(), snap, 1)
        .await
        .unwrap();

    // ran_nodes preserves scheduling order (frontier order, after gating)
    assert_eq!(
        res.ran_nodes,
        vec![NodeKind::Other("A".into()), NodeKind::Other("B".into())]
    );

    // outputs may arrive in any order; validate by ID set, not sequence
    let ids: std::collections::HashSet<_> = res.outputs.iter().map(|(id, _)| id.clone()).collect();
    let expected: std::collections::HashSet<_> =
        [NodeKind::Other("A".into()), NodeKind::Other("B".into())]
            .into_iter()
            .collect();
    assert_eq!(ids, expected);
}

#[tokio::test]
async fn test_superstep_serialized_with_limit_1() {
    // Two nodes with different delays, but concurrency limit 1 forces serial execution.
    let mut nodes: FxHashMap<NodeKind, Arc<dyn Node>> = FxHashMap::default();
    nodes.insert(
        NodeKind::Other("A".into()),
        Arc::new(DelayedNode {
            name: "A",
            delay_ms: 30,
        }),
    );
    nodes.insert(
        NodeKind::Other("B".into()),
        Arc::new(DelayedNode {
            name: "B",
            delay_ms: 1,
        }),
    );

    let frontier = vec![NodeKind::Other("A".into()), NodeKind::Other("B".into())];
    let snap = snap_with_versions(1, 1);
    let sched = Scheduler::new(1); // force serial execution
    let mut state = SchedulerState::default();

    let res = sched
        .superstep(&mut state, &nodes, frontier.clone(), snap, 1)
        .await
        .unwrap();

    // ran_nodes preserves scheduling order
    assert_eq!(
        res.ran_nodes,
        vec![NodeKind::Other("A".into()), NodeKind::Other("B".into())]
    );

    // outputs must be in the same order as ran_nodes
    let output_ids: Vec<_> = res.outputs.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(output_ids, res.ran_nodes);
}

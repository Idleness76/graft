use crate::channels::Channel;
use rustc_hash::FxHashMap;
use serde_json::json;

use super::graph::GraphBuilder;
use super::message::Message;
use super::node::{NodeA, NodeB, NodePartial};
use super::state::VersionedState;
use super::types::NodeKind;

/// Demonstration run showcasing:
/// 1. Building and executing a small multi-step graph
/// 2. Inspecting final state (messages + extra)
/// 3. Manual barrier applications (mixed, no-op, saturating version)
/// 4. GraphBuilder error cases
pub async fn run_demo1() -> anyhow::Result<()> {
    println!("== Demo start ==");

    // 1. Initial state with a user message + seeded extra data
    let mut init = VersionedState::new_with_user_message("Hello world");
    init.extra
        .get_mut()
        .insert("numbers".into(), json!([1, 2, 3]));
    init.extra
        .get_mut()
        .insert("info".into(), json!({"stage": "init"}));

    // 2. Build multi-step graph: Start -> A -> B -> End (with a duplicate edge to A to show fan-out)
    let app = GraphBuilder::new()
        .add_node(NodeKind::Start, NodeA)
        .add_node(NodeKind::Other("A".into()), NodeA)
        .add_node(NodeKind::Other("B".into()), NodeB)
        .add_node(NodeKind::End, NodeB)
        .add_edge(NodeKind::Start, NodeKind::Other("A".into()))
        .add_edge(NodeKind::Start, NodeKind::Other("A".into())) // duplicate path
        .add_edge(NodeKind::Other("A".into()), NodeKind::Other("B".into()))
        .add_edge(NodeKind::Other("B".into()), NodeKind::End)
        .set_entry(NodeKind::Start)
        .compile()
        .map_err(|e| anyhow::Error::msg(format!("{:?}", e)))?;

    // 3. Invoke full app run
    let final_state = app
        .invoke(init)
        .await
        .map_err(|e| anyhow::Error::msg(format!("{:?}", e)))?;

    // 4. Snapshot & mutation demonstration (snapshots are deep copies)
    let snap_before = final_state.snapshot();
    println!(
        "Snapshot (pre-mutation): messages={}, extra_keys={}",
        snap_before.messages.len(),
        snap_before.extra.len()
    );

    let mut mutated = final_state.clone();
    mutated.messages.get_mut().push(Message {
        role: "assistant".into(),
        content: "post-run note".into(),
    });
    mutated
        .messages
        .set_version(snap_before.messages_version.saturating_add(1));
    mutated
        .extra
        .get_mut()
        .insert("post_mutation".into(), json!(true));
    let snap_after = mutated.snapshot();
    println!(
        "Snapshot (post-mutation clone): messages={}, version={}, extra_keys={}",
        snap_after.messages.len(),
        snap_after.messages_version,
        snap_after.extra.len()
    );

    // 5. Manual barrier scenarios
    let mut manual_state = final_state.clone();

    // a) Mixed updates: two partials adding messages and extra keys (with overwrite)
    let run_ids = vec![
        NodeKind::Other("ManualA".into()),
        NodeKind::Other("ManualB".into()),
    ];
    let mut extra1 = FxHashMap::default();
    extra1.insert("manual".into(), json!("yes"));
    extra1.insert("shared".into(), json!("v1"));
    let mut extra2 = FxHashMap::default();
    extra2.insert("shared".into(), json!("v2")); // overwrite
    extra2.insert("more".into(), json!(123));

    let partials_mixed = vec![
        NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "manual msg 1".into(),
            }]),
            extra: Some(extra1),
        },
        NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "manual msg 2".into(),
            }]),
            extra: Some(extra2),
        },
    ];

    let updated = app
        .apply_barrier(&mut manual_state, &run_ids, partials_mixed)
        .await
        .map_err(|e| anyhow::Error::msg(format!("{:?}", e)))?;
    println!("Barrier (mixed) updated channels: {:?}", updated);
    {
        let snap = manual_state.snapshot();
        println!(
            "After mixed barrier: messages={}, extra_keys={}",
            snap.messages.len(),
            snap.extra.len()
        );
        println!(
            "Extra merged shared key: {:?}",
            snap.extra.get("shared").unwrap()
        );
    }

    // b) No-op updates (empty vectors/maps should produce no version bumps)
    let partials_noop = vec![
        NodePartial {
            messages: Some(vec![]),
            extra: None,
        },
        NodePartial {
            messages: None,
            extra: Some(FxHashMap::default()),
        },
    ];
    let updated2 = app
        .apply_barrier(&mut manual_state, &[], partials_noop)
        .await
        .map_err(|e| anyhow::Error::msg(format!("{:?}", e)))?;
    println!("Barrier (no-op) updated channels: {:?}", updated2);

    // c) Saturating version test for messages.version
    {
        manual_state.messages.set_version(u32::MAX);
    }
    let saturated = NodePartial {
        messages: Some(vec![Message {
            role: "assistant".into(),
            content: "won't bump ver".into(),
        }]),
        extra: None,
    };
    let _ = app
        .apply_barrier(&mut manual_state, &[], vec![saturated])
        .await
        .map_err(|e| anyhow::Error::msg(format!("{:?}", e)))?;
    {
        println!(
            "Saturating test messages.version={} (should remain u32::MAX)",
            manual_state.messages.version()
        );
    }

    // 6. GraphBuilder error demonstrations
    match super::graph::GraphBuilder::new()
        .add_node(NodeKind::Start, NodeA)
        .add_node(NodeKind::End, NodeB)
        .add_edge(NodeKind::Start, NodeKind::End)
        .compile()
    {
        Err(e) => println!("Expected error (no entry set): {:?}", e),
        Ok(_) => println!("Unexpected success (missing entry)"),
    }

    match super::graph::GraphBuilder::new()
        .set_entry(NodeKind::Other("Unreg".into()))
        .compile()
    {
        Err(e) => println!("Expected error (entry not registered): {:?}", e),
        Ok(_) => println!("Unexpected success (unregistered entry)"),
    }

    println!("== Demo complete ==");
    Ok(())
}

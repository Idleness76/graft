use std::u32;

use crate::channels::Channel;

pub async fn run_demo1() -> anyhow::Result<()> {
    use serde_json::json;
    use std::collections::HashMap;

    use super::graph::GraphBuilder;
    use super::message::Message;
    use super::node::{NodeA, NodeB, NodePartial};
    use super::state::VersionedState;
    use super::types::NodeKind;

    println!("== Demo start ==");

    // 1. Initial rich state
    let mut init = VersionedState::new_with_user_message("Hello world");
    init.outputs.value.push("seed output".into());
    init.meta.value.insert("init_key".into(), "init_val".into());
    init.extra.value.insert("numbers".into(), json!([1, 2, 3]));

    // 2. Build multi-step graph
    let app = GraphBuilder::new()
        .add_node(NodeKind::Start, NodeA)
        .add_node(NodeKind::Other("A".into()), NodeA)
        .add_node(NodeKind::Other("B".into()), NodeB)
        .add_node(NodeKind::End, NodeB)
        .add_edge(NodeKind::Start, NodeKind::Other("A".into()))
        .add_edge(NodeKind::Start, NodeKind::Other("A".into()))
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

    // 4. Snapshot & mutation demonstration
    let snap_before = final_state.snapshot();
    println!(
        "Snapshot before manual mutation: messages={}, outputs={}, meta_keys={}",
        snap_before.messages.len(),
        snap_before.outputs.len(),
        snap_before.meta.len()
    );
    // Mutate clone (not affecting prior snapshot)
    let mut mutated = final_state.clone();
    mutated.messages.get_mut().push(Message {
        role: "assistant".into(),
        content: "post-run note".into(),
    });
    mutated
        .messages
        .set_version(snap_before.messages_version + 1);
    let snap_after = mutated.snapshot();
    println!(
        "Snapshot after mutation: messages={}, version={}",
        snap_after.messages.len(),
        snap_after.messages_version
    );

    // 5. (Removed) Direct reducer usage with legacy per-channel APIs.
    // Reducers are now applied via the ReducerRegistery inside barrier logic.
    // Proceed to manual barrier scenarios below.

    // 6. Manual barrier scenarios
    // a) Mixed updates
    let state_arc = std::sync::Arc::new(tokio::sync::RwLock::new(final_state.clone()));
    let run_ids = vec![
        NodeKind::Other("ManualA".into()),
        NodeKind::Other("ManualB".into()),
    ];
    let partials = vec![
        NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "manual msg 1".into(),
            }]),
            outputs: None,
            meta: Some(HashMap::from([("manual".into(), "yes".into())])),
        },
        NodePartial {
            messages: Some(vec![Message {
                role: "assistant".into(),
                content: "manual msg 2".into(),
            }]),
            outputs: Some(vec!["manual out".into()]),
            meta: None,
        },
    ];
    let updated = app
        .apply_barrier(&state_arc, &run_ids, partials)
        .await
        .map_err(|e| anyhow::Error::msg(format!("{:?}", e)))?;
    println!("Barrier (mixed) updated channels: {:?}", updated);

    // b) No-op updates (empty)
    let updated2 = app
        .apply_barrier(
            &state_arc,
            &[],
            vec![NodePartial {
                messages: Some(vec![]),
                outputs: Some(vec![]),
                meta: Some(HashMap::new()),
            }],
        )
        .await
        .map_err(|e| anyhow::Error::msg(format!("{:?}", e)))?;
    println!("Barrier (no-op) updated channels: {:?}", updated2);

    // c) Saturating version test
    {
        let mut lock = state_arc.write().await;
        lock.messages.set_version(u32::MAX);
    }
    let _ = app
        .apply_barrier(
            &state_arc,
            &[],
            vec![NodePartial {
                messages: Some(vec![Message {
                    role: "assistant".into(),
                    content: "won't bump ver".into(),
                }]),
                outputs: None,
                meta: None,
            }],
        )
        .await
        .map_err(|e| anyhow::Error::msg(format!("{:?}", e)))?;
    {
        let lock = state_arc.read().await;
        println!(
            "Saturating test messages.version={}",
            lock.messages.version()
        );
    }

    // 7. GraphBuilder error demonstrations
    match super::graph::GraphBuilder::new()
        .add_node(super::types::NodeKind::Start, super::node::NodeA)
        .add_node(super::types::NodeKind::End, super::node::NodeB)
        .add_edge(super::types::NodeKind::Start, super::types::NodeKind::End)
        .compile()
    {
        Err(e) => println!("Expected error (no entry set): {:?}", e),
        Ok(_) => println!("Unexpected success (missing entry)"),
    }

    match super::graph::GraphBuilder::new()
        .set_entry(super::types::NodeKind::Other("Unreg".into()))
        .compile()
    {
        Err(e) => println!("Expected error (entry not registered): {:?}", e),
        Ok(_) => println!("Unexpected success (unregistered entry)"),
    }

    println!("== Demo complete ==");
    Ok(())
}

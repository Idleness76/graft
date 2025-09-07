use graft::message::*;
use graft::node::*;
use graft::state::*;

use tokio; // Ensure tokio is in your dependencies

#[tokio::main]
async fn main() {
    // Create a sample StateSnapshot
    let snapshot = StateSnapshot {
        messages: vec![Message {
            role: "user".into(),
            content: "Hello!".into(),
        }],
        messages_version: 1,
        outputs: vec![],
        outputs_version: 1,
        meta: std::collections::HashMap::new(),
        meta_version: 1,
    };

    // Create a NodeContext
    let ctx = NodeContext {
        node_id: "A".into(),
        step: 1,
    };

    // Instantiate NodeA and run it
    let node = NodeA;
    let result = node.run(snapshot, ctx).await;

    println!("NodePartial: {:?}", result);
}

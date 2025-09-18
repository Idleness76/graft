use async_trait::async_trait;
use rig::client::CompletionClient;
use rig::completion::CompletionModelDyn;
use rig::{completion::Prompt, providers::ollama};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::u64;

use super::graph::GraphBuilder;
use super::node::{Node, NodeContext, NodePartial};
use super::schedulers::{Scheduler, SchedulerState, StepRunResult};
use super::state::{StateSnapshot, VersionedState};
use super::types::NodeKind;
use crate::channels::Channel;
use crate::message::*;

struct NodeA;

#[async_trait]
impl Node for NodeA {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> NodePartial {
        //this will be the first node to run, first message should be the user prompt
        // can validate ctx.step if necessary
        let user_prompt = snapshot.messages.last().unwrap();

        println!("initial prompt is: {}", user_prompt.content);
        let client = ollama::Client::new();
        let completion_model = client.completion_model("gemma3:270m");

        let completion_request = completion_model
            .completion_request(rig::completion::Message::user(user_prompt.content.clone()))
            .preamble("you are comedy writer AI assistant, deliver your response as a single succint line of text".to_owned())
            .temperature(0.5)
            .build();

        let response = completion_model
            .completion(completion_request)
            .await
            .unwrap();
        println!("model response is: {:?}", response);

        NodePartial {
            messages: Some(
                response
                    .choice
                    .into_iter()
                    .map(|assistant_content| Message {
                        content: serde_json::to_string(&assistant_content).unwrap(),
                        role: "assistant".into(),
                    })
                    .collect(),
            ),
            extra: None,
        }
    }
}

struct NodeB;

#[async_trait]
impl Node for NodeB {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> NodePartial {
        let joke_response = snapshot.messages.last().unwrap();
        let client = ollama::Client::new();
        let completion_model = client.completion_model("gemma3");

        println!("first joke run is: {}", joke_response.content);

        let completion_request = completion_model
            .completion_request(rig::completion::Message::user(format!(
                "here's my baby themed joke: {}. \n add a line about a cat and return the full new joke with the cat addition",
                joke_response.content.clone()
            )))
            .preamble("you are comedy writer AI assistant, do exactly as the prompt requires you to and deliver your response as a single succint line of text".to_owned())
            .temperature(0.8)
            .build();

        let response = completion_model
            .completion(completion_request)
            .await
            .unwrap();

        println!("model response is: {:?}", response);
        NodePartial {
            messages: Some(
                response
                    .choice
                    .into_iter()
                    .map(|assistant_content| Message {
                        content: serde_json::to_string(&assistant_content).unwrap(),
                        role: "assistant".into(),
                    })
                    .collect(),
            ),
            extra: None,
        }
    }
}

/// Demonstration run showcasing:
/// 1. Building and executing a small multi-step graph using Scheduler
/// 2. Inspecting StepRunResult (ran/skipped/outputs)
/// 3. Manual concurrency control and version gating
/// 4. Barrier application using StepRunResult outputs
pub async fn run_demo3() -> anyhow::Result<()> {
    println!("\n==============================");
    println!("== Demo3: Let's Rig this bad boy ==");
    println!("==============================\n");

    // 1. Initial state with a user message + seeded extra data
    let mut init = VersionedState::new_with_user_message("Write a joke about babies");

    // 2. Build a richer graph with some fan-out and re-visits:
    //    Start -> A, Start -> B, A -> B, B -> End
    //    This ensures Step 1 runs both A and B concurrently; Step 2 runs B again due to A's output.
    let app = GraphBuilder::new()
        .add_node(NodeKind::Start, NodeA)
        .add_node(NodeKind::Other("A".into()), NodeA)
        .add_node(NodeKind::Other("B".into()), NodeB)
        .add_node(NodeKind::End, NodeB)
        .add_edge(NodeKind::Start, NodeKind::Other("A".into()))
        .add_edge(NodeKind::Other("A".into()), NodeKind::Other("B".into()))
        .add_edge(NodeKind::Other("B".into()), NodeKind::End)
        .set_entry(NodeKind::Start)
        .compile()
        .map_err(|e| anyhow::Error::msg(format!("{:?}", e)))?;

    let final_state = app.invoke(init).await.unwrap();

    println!("\n== Final state ==");
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
    println!("== Demo3 complete ==");
    // Recap totals

    Ok(())
}

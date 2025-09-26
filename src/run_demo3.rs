use super::graph::GraphBuilder;
use super::node::{Node, NodeContext, NodeError, NodePartial};

use super::state::{StateSnapshot, VersionedState};
use super::types::NodeKind;
use crate::channels::Channel;
use crate::channels::errors::pretty_print;
use crate::message::*;
use crate::runtimes::{CheckpointerType, RuntimeConfig};
use async_trait::async_trait;
use miette::Result;
use rig::client::CompletionClient;
use rig::completion::CompletionModel;
use rig::providers::ollama;
use rustc_hash::FxHashMap;
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::instrument;

struct NodeA;

#[async_trait]
impl Node for NodeA {
    #[instrument(skip(self, snapshot, _ctx))]
    async fn run(
        &self,
        snapshot: StateSnapshot,
        _ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        //this will be the first node to run, first message should be the user prompt
        // can validate ctx.step if necessary
        let user_prompt = snapshot.messages.last().ok_or(NodeError::MissingInput {
            what: "user_prompt",
        })?;

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
            .map_err(|e| NodeError::Provider {
                provider: "ollama",
                message: e.to_string(),
            })?;
        println!("model response is: {:?}", response);

        let messages: Result<Vec<Message>, serde_json::Error> = response
            .choice
            .into_iter()
            .map(|assistant_content| {
                Ok(Message {
                    content: serde_json::to_string(&assistant_content)?,
                    role: "assistant".into(),
                })
            })
            .collect();
        Ok(NodePartial {
            messages: Some(messages.map_err(NodeError::from)?),
            extra: None,
            errors: None,
        })
    }
}

struct NodeB;

#[async_trait]
impl Node for NodeB {
    #[instrument(skip(self, snapshot, ctx))]
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        let cat_iterations = serde_json::from_value::<i32>(
            snapshot
                .extra
                .get("cat iterations")
                .unwrap_or(&json!(0))
                .clone(),
        )
        .map_err(NodeError::from)?;

        let joke_response = snapshot.messages.last().unwrap();
        let client = ollama::Client::new();
        let completion_model = client.completion_model("gemma3");

        ctx.emit(
            "Node B",
            format!(
                "first joke run is: {}, number of cat iterations is {}",
                joke_response.content, cat_iterations
            ),
        )
        .unwrap();

        let completion_request = completion_model
            .completion_request(rig::completion::Message::user(format!(
                "here's my baby themed joke: {}. \n add a line about a cat and return the full new joke with the cat addition. \n if a line about a cat already exists, overwrite it with a funnier one liner",
                joke_response.content.clone()
            )))
            .preamble("you are comedy writer AI assistant, do exactly as the prompt requires you to and deliver your response as a single succint line of text".to_owned())
            .temperature(0.8)
            .build();

        let response = completion_model
            .completion(completion_request)
            .await
            .map_err(|e| NodeError::Provider {
                provider: "ollama",
                message: e.to_string(),
            })?;

        println!("model response is: {:?}", response);

        let mut extra: FxHashMap<String, Value> = FxHashMap::default();
        extra.insert("cat iterations".into(), json!(cat_iterations + 1));
        let messages: Result<Vec<Message>, serde_json::Error> = response
            .choice
            .into_iter()
            .map(|assistant_content| {
                Ok(Message {
                    content: serde_json::to_string(&assistant_content)?,
                    role: "assistant".into(),
                })
            })
            .collect();
        Ok(NodePartial {
            messages: Some(messages.map_err(NodeError::from)?),
            extra: Some(extra),
            errors: None,
        })
    }
}

/// Demonstration run showcasing:
/// 1. Building and executing a small multi-step graph using Scheduler
/// 2. Inspecting StepRunResult (ran/skipped/outputs)
/// 3. Manual concurrency control and version gating
/// 4. Barrier application using StepRunResult outputs
#[instrument]
pub async fn run_demo3() -> Result<()> {
    println!("\n==============================");
    println!("== Demo3: Let's Rig this bad boy ==");
    println!("==============================\n");

    // 1. Initial state with a user message + seeded extra data
    let mut init = VersionedState::new_with_user_message("Write a joke about babies");
    init.extra
        .get_mut()
        .insert("cat iterations".into(), json!(0));
    // 2. Build a richer graph with some fan-out and re-visits:
    //    Start -> A, Start -> B, A -> B, B -> End
    //    This ensures Step 1 runs both A and B concurrently; Step 2 runs B again due to A's output.
    let app = GraphBuilder::new()
        .add_node(NodeKind::Other("A".into()), NodeA)
        .add_node(NodeKind::Other("B".into()), NodeB)
        .add_edge(NodeKind::Start, NodeKind::Other("A".into()))
        .add_edge(NodeKind::Other("A".into()), NodeKind::Other("B".into()))
        .add_conditional_edge(
            NodeKind::Other("B".into()),
            NodeKind::End,
            NodeKind::Other("B".into()),
            Arc::new(|snapshot: StateSnapshot| {
                serde_json::from_value::<i32>(
                    snapshot
                        .extra
                        .get("cat iterations")
                        .unwrap_or(&json!(0))
                        .clone(),
                )
                .unwrap()
                    > 1
            }),
        )
        //.add_edge(NodeKind::Other("B".into()), NodeKind::End)
        .set_entry(NodeKind::Start)
        .with_runtime_config(RuntimeConfig {
            session_id: Some("alads_8".into()),
            checkpointer: Some(CheckpointerType::SQLite),
            sqlite_db_name: None,
        })
        .compile()
        .map_err(|e| miette::miette!("{e:?}"))?;

    let final_state = app.invoke(init).await?;
    // Optionally log something from the final state to avoid unused warnings
    println!("final messages: {}", final_state.messages.snapshot().len());

    println!("== Demo3 complete ==");
    // Print any error events accumulated
    let errs = final_state.errors.snapshot();
    if !errs.is_empty() {
        println!("\nErrors captured:\n{}", pretty_print(&errs));
    }
    // Recap totals

    Ok(())
}

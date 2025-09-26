use super::graph::GraphBuilder;
use super::node::{Node, NodeContext, NodeError, NodePartial};
use super::state::{StateSnapshot, VersionedState};
use super::types::NodeKind;
use crate::channels::Channel;
use crate::channels::errors::pretty_print;
use crate::message::*;
use crate::runtimes::{CheckpointerType, RuntimeConfig};
use async_trait::async_trait;
use futures::StreamExt;
use miette::Result;
use rig::client::CompletionClient;
use rig::completion::{CompletionModel, GetTokenUsage};
use rig::prelude::*;
use rig::providers::ollama;
use tracing::instrument;

struct NodeA;

#[async_trait]
impl Node for NodeA {
    #[instrument(skip(self, snapshot, ctx))]
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        //this will be the first node to run, first message should be the user prompt
        // can validate ctx.step if necessary
        let user_prompt = snapshot.messages.last().ok_or(NodeError::MissingInput {
            what: "user_prompt",
        })?;

        ctx.emit(
            "Node A pre model call",
            format!("initial prompt is: {}", user_prompt.content),
        )
        .unwrap();
        let client = ollama::Client::new();
        let completion_model = client.completion_model("gemma3");

        let completion_request = completion_model
            .completion_request(user_prompt.content.clone())
            .preamble("You are a senior rust developer AI Assistant".to_owned())
            .temperature(0.9)
            .build();

        let mut stream = completion_model
            .stream(completion_request)
            .await
            .map_err(|e| NodeError::Provider {
                provider: "ollama",
                message: e.to_string(),
            })?;

        let mut chunk_count = 0;
        let mut model_response = String::new();
        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(content) => match content {
                    rig::streaming::StreamedAssistantContent::Text(text) => {
                        ctx.emit("Node A LLM stream", text.to_string()).unwrap();
                        model_response += text.text();
                        chunk_count += 1;
                    }
                    rig::streaming::StreamedAssistantContent::Final(response) => {
                        ctx.emit("Node A LLM stream", "Node A stream complete".to_owned())
                            .unwrap();
                        if let Some(usage) = response.token_usage() {
                            ctx.emit("Node A LLM stream", format!("Token usage: {:?}", usage))
                                .unwrap();
                        }
                        break;
                    }
                    _ => {}
                },
                Err(e) => {
                    if e.to_string().contains("aborted") {
                        println!("\n[Stream cancelled]");
                        break;
                    }
                    eprintln!("Error: {}", e);
                    break;
                }
            }
        }

        Ok(NodePartial {
            messages: Some(vec![Message {
                content: model_response,
                role: "assistant".to_owned(),
            }]),
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
        let model_response = snapshot.messages.last().unwrap();
        let client = ollama::Client::new();
        let completion_model = client.completion_model("gemma3");

        let completion_request = completion_model
            .completion_request(rig::completion::Message::user(format!(
                "here's my essay about rust lifetimes: {}. \n    add 3 more paragraphs about the borrow checker and best practices for using it",
                model_response.content.clone()
            )))
            .preamble("you are a senior Rust developer AI assistant".to_owned())
            .temperature(0.9)
            .build();

        let mut stream = completion_model
            .stream(completion_request)
            .await
            .map_err(|e| NodeError::Provider {
                provider: "ollama",
                message: e.to_string(),
            })?;

        let mut chunk_count = 0;
        let mut model_response = String::new();
        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(content) => match content {
                    rig::streaming::StreamedAssistantContent::Text(text) => {
                        ctx.emit("Node B LLM stream", text.to_string()).unwrap();
                        model_response += text.text();
                        chunk_count += 1;
                    }
                    rig::streaming::StreamedAssistantContent::Final(response) => {
                        ctx.emit("Node B LLM stream", "Node B stream complete".to_owned())
                            .unwrap();
                        if let Some(usage) = response.token_usage() {
                            ctx.emit("Node B LLM stream", format!("Token usage: {:?}", usage))
                                .unwrap();
                        }
                        break;
                    }
                    _ => {}
                },
                Err(e) => {
                    if e.to_string().contains("aborted") {
                        println!("\n[Stream cancelled]");
                        break;
                    }
                    eprintln!("Error: {}", e);
                    break;
                }
            }
        }

        Ok(NodePartial {
            messages: Some(vec![Message {
                content: model_response,
                role: "assistant".to_owned(),
            }]),
            extra: None,
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
pub async fn run_demo4() -> Result<()> {
    println!("\n==============================");
    println!("== Demo4: Now with streaming ==");
    println!("==============================\n");

    // 1. Initial state with a user message + seeded extra data
    let init = VersionedState::new_with_user_message("Write 2 paragraphs about lifetimes in Rust");

    let app = GraphBuilder::new()
        .add_node(NodeKind::Other("A".into()), NodeA)
        .add_node(NodeKind::Other("B".into()), NodeB)
        .add_edge(NodeKind::Start, NodeKind::Other("A".into()))
        .add_edge(NodeKind::Other("A".into()), NodeKind::Other("B".into()))
        .add_edge(NodeKind::Other("B".into()), NodeKind::End)
        .set_entry(NodeKind::Start)
        .with_runtime_config(RuntimeConfig {
            session_id: Some("salads_05".into()),
            checkpointer: Some(CheckpointerType::SQLite),
            sqlite_db_name: None,
        })
        .compile()
        .map_err(|e| miette::miette!("{e:?}"))?;

    let final_state = app.invoke(init).await?;
    // Optionally log something from the final state to avoid unused warnings
    println!("final messages: {}", final_state.messages.snapshot().len());

    println!("== Demo4 complete ==");
    // Print any error events accumulated
    let errs = final_state.errors.snapshot();
    if !errs.is_empty() {
        println!("\nErrors captured:\n{}", pretty_print(&errs));
    }
    // Recap totals

    Ok(())
}

use super::graph::GraphBuilder;
use super::node::{Node, NodeContext, NodeError, NodePartial};
use super::state::{StateSnapshot, VersionedState};
use super::types::NodeKind;
use crate::channels::Channel;
use crate::channels::errors::{ErrorEvent, ErrorScope, LadderError, pretty_print};
use crate::message::*;
use crate::runtimes::{CheckpointerType, RuntimeConfig};
use async_trait::async_trait;
use futures::StreamExt;
use miette::Result;
use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::message::{Reasoning, Text};
use rig::prelude::*;
use rig::providers::gemini::completion::gemini_api_types::{
    AdditionalParameters, GenerationConfig, ThinkingConfig,
};
use rig::{
    providers::gemini::{self},
    streaming::{StreamedAssistantContent, StreamingPrompt},
};
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
        let gen_cfg = GenerationConfig {
            thinking_config: Some(ThinkingConfig {
                include_thoughts: Some(true),
                thinking_budget: 2048,
            }),
            ..Default::default()
        };
        let cfg = AdditionalParameters::default().with_config(gen_cfg);
        // Create streaming agent with a single context prompt
        let agent = gemini::Client::from_env()
            .agent("gemini-2.5-flash")
            .preamble("You are a senior rust developer AI Assistant.")
            .temperature(0.9)
            .additional_params(serde_json::to_value(cfg).unwrap())
            .build();

        // Stream the response and print chunks as they arrive
        let mut stream = agent.stream_prompt(user_prompt.content.clone()).await;

        let mut chunk_count = 0;
        let mut model_response = String::new();
        while let Some(content) = stream.next().await {
            match content {
                Ok(MultiTurnStreamItem::StreamItem(StreamedAssistantContent::Text(Text {
                    text,
                }))) => {
                    model_response += &text;
                    ctx.emit("Node A LLM stream", text).unwrap();
                    chunk_count += 1;
                }
                Ok(MultiTurnStreamItem::StreamItem(StreamedAssistantContent::Reasoning(
                    Reasoning { reasoning, .. },
                ))) => {
                    let reasoning = reasoning.join("\n");
                    ctx.emit("Node A LLM Reasoning", reasoning).unwrap();
                    chunk_count += 1;
                }
                Ok(MultiTurnStreamItem::FinalResponse(res)) => {
                    ctx.emit("Node A LLM stream", res.response()).unwrap();
                    model_response += res.response();
                    chunk_count += 1;
                }
                Err(err) => {
                    eprintln!("Error: {err}");
                }
                _ => {}
            }
        }

        // Create some example errors based on the response
        let mut errors = Vec::new();

        // Example 1: Check if response is very short (could indicate an issue)
        if model_response.len() < 50 {
            errors.push(ErrorEvent {
                when: chrono::Utc::now(),
                scope: ErrorScope::Node {
                    kind: "A".to_string(),
                    step: ctx.step,
                },
                error: LadderError {
                    message: "Generated response is suspiciously short".to_string(),
                    cause: None,
                    details: serde_json::json!({
                        "response_length": model_response.len(),
                        "chunk_count": chunk_count
                    }),
                },
                tags: vec!["quality_check".to_string(), "short_response".to_string()],
                context: serde_json::json!({
                    "node_type": "LLM",
                    "model": "gemma3"
                }),
            });
        }

        // Example 2: Warning if we had very few chunks (potential streaming issue)
        if chunk_count < 3 {
            errors.push(ErrorEvent {
                when: chrono::Utc::now(),
                scope: ErrorScope::Node {
                    kind: "A".to_string(),
                    step: ctx.step,
                },
                error: LadderError {
                    message: format!("Low chunk count in streaming response: {}", chunk_count),
                    cause: None,
                    details: serde_json::json!({
                        "chunk_count": chunk_count,
                        "expected_min": 3
                    }),
                },
                tags: vec!["streaming_warning".to_string()],
                context: serde_json::json!({
                    "node_type": "LLM",
                    "model": "gemma3"
                }),
            });
        }
        ctx.emit(
            "Node A post model call",
            format!("total chunks for this stage: {}", chunk_count),
        )
        .unwrap();

        Ok(NodePartial {
            messages: Some(vec![Message {
                content: model_response,
                role: "assistant".to_owned(),
            }]),
            extra: None,
            errors: if errors.is_empty() {
                None
            } else {
                Some(errors)
            },
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

        ctx.emit(
            "Node B pre model call",
            format!("last model response is: {}", model_response.content),
        )
        .unwrap();

        let gen_cfg = GenerationConfig {
            thinking_config: Some(ThinkingConfig {
                include_thoughts: Some(true),
                thinking_budget: 2048,
            }),
            ..Default::default()
        };
        let cfg = AdditionalParameters::default().with_config(gen_cfg);
        // Create streaming agent with a single context prompt
        let agent = gemini::Client::from_env()
            .agent("gemini-2.5-flash")
            .preamble("You are a senior rust developer AI Assistant.")
            .temperature(0.9)
            .additional_params(serde_json::to_value(cfg).unwrap())
            .build();

        // Stream the response and print chunks as they arrive
        let mut stream = agent.stream_prompt(rig::completion::Message::user(format!(
            "here's my essay about rust lifetimes: {}. \n    add 3 more paragraphs about the borrow checker and best practices for using it",
            model_response.content.clone()
        ))).await;

        let mut chunk_count = 0;
        let mut model_response = String::new();
        while let Some(content) = stream.next().await {
            match content {
                Ok(MultiTurnStreamItem::StreamItem(StreamedAssistantContent::Text(Text {
                    text,
                }))) => {
                    model_response += &text;
                    ctx.emit("Node B LLM stream", text).unwrap();
                    chunk_count += 1;
                }
                Ok(MultiTurnStreamItem::StreamItem(StreamedAssistantContent::Reasoning(
                    Reasoning { reasoning, .. },
                ))) => {
                    let reasoning = reasoning.join("\n");
                    ctx.emit("Node B LLM Reasoning", reasoning).unwrap();
                    chunk_count += 1;
                }
                Ok(MultiTurnStreamItem::FinalResponse(res)) => {
                    ctx.emit("Node B LLM stream", res.response()).unwrap();
                    model_response += res.response();
                    chunk_count += 1;
                }
                Err(err) => {
                    eprintln!("Error: {err}");
                }
                _ => {}
            }
        }

        // Add multiple types of errors for NodeB
        let mut errors = Vec::new();

        // Check if the previous message looks incomplete (missing punctuation)
        let prev_msg = snapshot.messages.last().unwrap();
        if !prev_msg.content.trim_end().ends_with(&['.', '!', '?'][..]) {
            errors.push(ErrorEvent {
                when: chrono::Utc::now(),
                scope: ErrorScope::Node {
                    kind: "B".to_string(),
                    step: ctx.step,
                },
                error: LadderError {
                    message: "Input message appears incomplete (no ending punctuation)".to_string(),
                    cause: None,
                    details: serde_json::json!({
                        "input_message_length": prev_msg.content.len(),
                        "last_chars": prev_msg.content.chars().rev().take(10).collect::<String>()
                    }),
                },
                tags: vec!["input_quality".to_string()],
                context: serde_json::json!({
                    "node_type": "LLM",
                    "model": "gemma3",
                    "operation": "expansion"
                }),
            });
        }

        // Check for streaming issues during expansion (should have more chunks since it's generating more content)
        if chunk_count < 5 {
            errors.push(ErrorEvent {
                when: chrono::Utc::now(),
                scope: ErrorScope::Node {
                    kind: "B".to_string(),
                    step: ctx.step,
                },
                error: LadderError {
                    message: format!("Low chunk count for expansion task: {}", chunk_count),
                    cause: None,
                    details: serde_json::json!({
                        "chunk_count": chunk_count,
                        "expected_min_for_expansion": 5,
                        "task": "add 3 paragraphs"
                    }),
                },
                tags: vec![
                    "streaming_warning".to_string(),
                    "expansion_task".to_string(),
                ],
                context: serde_json::json!({
                    "node_type": "LLM",
                    "model": "gemma3",
                    "operation": "expansion"
                }),
            });
        }

        // Check if expansion actually produced more content than input
        if (model_response.len() as f64) <= (prev_msg.content.len() as f64 * 1.2) {
            errors.push(ErrorEvent {
                when: chrono::Utc::now(),
                scope: ErrorScope::Node {
                    kind: "B".to_string(),
                    step: ctx.step,
                },
                error: LadderError {
                    message: "Expansion task may not have added sufficient content".to_string(),
                    cause: None,
                    details: serde_json::json!({
                        "input_length": prev_msg.content.len(),
                        "output_length": model_response.len(),
                        "expansion_ratio": model_response.len() as f64 / prev_msg.content.len() as f64,
                        "chunk_count": chunk_count
                    }),
                },
                tags: vec!["content_quality".to_string(), "expansion_task".to_string()],
                context: serde_json::json!({
                    "node_type": "LLM",
                    "model": "gemma3",
                    "operation": "expansion"
                }),
            });
        }
        ctx.emit(
            "Node B post model call",
            format!("total chunks for this stage: {}", chunk_count),
        )
        .unwrap();

        Ok(NodePartial {
            messages: Some(vec![Message {
                content: model_response,
                role: "assistant".to_owned(),
            }]),
            extra: None,
            errors: if errors.is_empty() {
                None
            } else {
                Some(errors)
            },
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
            session_id: Some("streaming_1".into()),
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

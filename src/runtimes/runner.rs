use rustc_hash::FxHashMap;
use std::sync::Arc;

use super::checkpointer::{Checkpoint, Checkpointer, restore_session_state};
use crate::app::App;
use crate::channels::Channel;
use crate::node::NodePartial;
use crate::schedulers::{Scheduler, SchedulerState};
use crate::state::VersionedState;
use crate::types::NodeKind;

/// Result of executing one superstep in a session.
#[derive(Debug, Clone)]
pub struct StepReport {
    pub step: u64,
    pub ran_nodes: Vec<NodeKind>,
    pub skipped_nodes: Vec<NodeKind>,
    pub updated_channels: Vec<&'static str>,
    pub next_frontier: Vec<NodeKind>,
    pub state_versions: StateVersions,
    pub completed: bool,
}

/// Snapshot of channel versions for tracking state evolution
#[derive(Debug, Clone)]
pub struct StateVersions {
    pub messages_version: u32,
    pub extra_version: u32,
}

/// Session state that needs to be persisted across steps
#[derive(Debug, Clone)]
pub struct SessionState {
    pub state: VersionedState,
    pub step: u64,
    pub frontier: Vec<NodeKind>,
    pub scheduler: Scheduler,
    pub scheduler_state: SchedulerState,
}

/// Options for step execution
#[derive(Debug, Clone, Default)]
pub struct StepOptions {
    pub interrupt_before: Vec<NodeKind>,
    pub interrupt_after: Vec<NodeKind>,
    pub interrupt_each_step: bool,
}

/// Paused execution context
#[derive(Debug, Clone)]
pub enum PausedReason {
    BeforeNode(NodeKind),
    AfterNode(NodeKind),
    AfterStep(u64),
}

/// Extended step report when execution is paused
#[derive(Debug, Clone)]
pub struct PausedReport {
    pub session_state: SessionState,
    pub reason: PausedReason,
}

/// Result of attempting to run a step
#[derive(Debug, Clone)]
pub enum StepResult {
    Completed(StepReport),
    Paused(PausedReport),
}

/// Stepwise execution wrapper around App that supports sessions and interrupts
pub struct AppRunner {
    app: Arc<App>,
    sessions: FxHashMap<String, SessionState>,
    checkpointer: Option<Arc<dyn Checkpointer>>, // optional pluggable persistence
    autosave: bool,
}

impl AppRunner {
    /// Create a new AppRunner wrapping the given App
    pub fn new(app: App) -> Self {
        Self::with_options(app, None, true)
    }

    pub fn from_arc(app: Arc<App>) -> Self {
        Self::with_options_arc(app, None, true)
    }

    /// Create with explicit checkpointer + autosave toggle
    pub fn with_options(
        app: App,
        checkpointer: Option<Arc<dyn Checkpointer>>,
        autosave: bool,
    ) -> Self {
        Self {
            app: Arc::new(app),
            sessions: FxHashMap::default(),
            checkpointer,
            autosave,
        }
    }
    pub fn with_options_arc(
        app: Arc<App>,
        checkpointer: Option<Arc<dyn Checkpointer>>,
        autosave: bool,
    ) -> Self {
        Self {
            app,
            sessions: FxHashMap::default(),
            checkpointer,
            autosave,
        }
    }

    /// Initialize a new session with the given initial state
    pub fn create_session(
        &mut self,
        session_id: String,
        initial_state: VersionedState,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // If checkpointer present and session exists, load instead of creating anew
        if let Some(cp) = &self.checkpointer
            && let Some(stored) = cp.load_latest(&session_id).map_err(|e| e.to_string())?
        {
            let restored = restore_session_state(&stored);
            self.sessions.insert(session_id, restored);
            return Ok(());
        }

        let frontier = self
            .app
            .edges()
            .get(&NodeKind::Start)
            .cloned()
            .unwrap_or_default();
        if frontier.is_empty() {
            return Err("No nodes to run from START".into());
        }
        let default_limit = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let scheduler = Scheduler::new(default_limit);
        let session_state = SessionState {
            state: initial_state,
            step: 0,
            frontier,
            scheduler,
            scheduler_state: SchedulerState::default(),
        };
        self.sessions
            .insert(session_id.clone(), session_state.clone());
        if let Some(cp) = &self.checkpointer {
            let _ = cp.save(Checkpoint::from_session(&session_id, &session_state));
        }
        Ok(())
    }

    /// Execute one superstep for the given session
    pub async fn run_step(
        &mut self,
        session_id: &str,
        options: StepOptions,
    ) -> Result<StepResult, Box<dyn std::error::Error + Send + Sync>> {
        // Clone session state to avoid borrowing issues
        let mut session_state = self
            .sessions
            .get(session_id)
            .ok_or_else(|| format!("Session '{}' not found", session_id))?
            .clone();

        // Check if already completed
        if session_state.frontier.is_empty()
            || session_state.frontier.iter().all(|n| *n == NodeKind::End)
        {
            let versions = StateVersions {
                messages_version: session_state.state.messages.version(),
                extra_version: session_state.state.extra.version(),
            };
            return Ok(StepResult::Completed(StepReport {
                step: session_state.step,
                ran_nodes: vec![],
                skipped_nodes: session_state.frontier.clone(),
                updated_channels: vec![],
                next_frontier: vec![],
                state_versions: versions,
                completed: true,
            }));
        }

        // Check for interrupt_before
        for node in &session_state.frontier {
            if options.interrupt_before.contains(node) {
                return Ok(StepResult::Paused(PausedReport {
                    session_state: session_state.clone(),
                    reason: PausedReason::BeforeNode(node.clone()),
                }));
            }
        }

        // Execute one superstep
        let step_report = self.run_one_superstep(&mut session_state).await?;

        // Update the session in map & persist if configured
        self.sessions
            .insert(session_id.to_string(), session_state.clone());
        if self.autosave
            && let Some(cp) = &self.checkpointer
        {
            let _ = cp.save(Checkpoint::from_session(session_id, &session_state));
        }

        // Check for interrupt_after
        for node in &step_report.ran_nodes {
            if options.interrupt_after.contains(node) {
                return Ok(StepResult::Paused(PausedReport {
                    session_state: session_state.clone(),
                    reason: PausedReason::AfterNode(node.clone()),
                }));
            }
        }

        // Check for interrupt_each_step
        if options.interrupt_each_step {
            return Ok(StepResult::Paused(PausedReport {
                session_state: session_state.clone(),
                reason: PausedReason::AfterStep(step_report.step),
            }));
        }

        Ok(StepResult::Completed(step_report))
    }

    /// Helper method that executes exactly one superstep on the given session state
    async fn run_one_superstep(
        &self,
        session_state: &mut SessionState,
    ) -> Result<StepReport, Box<dyn std::error::Error + Send + Sync>> {
        session_state.step += 1;
        let step = session_state.step;

        println!("\n-- Superstep {} --", step);

        let snapshot = session_state.state.snapshot();
        println!(
            "msgs={} v{}; extra_keys={} v{}",
            snapshot.messages.len(),
            snapshot.messages_version,
            snapshot.extra.len(),
            snapshot.extra_version
        );

        // Execute via scheduler
        let step_result = session_state
            .scheduler
            .superstep(
                &mut session_state.scheduler_state,
                self.app.nodes(),
                session_state.frontier.clone(),
                snapshot.clone(),
                step,
            )
            .await;

        // Reorder outputs to match ran_nodes order expected by the barrier
        let mut by_kind: FxHashMap<NodeKind, NodePartial> = FxHashMap::default();
        for (kind, part) in step_result.outputs {
            by_kind.insert(kind, part);
        }
        let run_ids: Vec<NodeKind> = step_result.ran_nodes.clone();
        let node_partials: Vec<NodePartial> = run_ids
            .iter()
            .cloned()
            .filter_map(|k| by_kind.remove(&k))
            .collect();

        // Apply barrier using the app's existing method
        let mut update_state = session_state.state.clone();
        let updated_channels = self
            .app
            .apply_barrier(&mut update_state, &run_ids, node_partials)
            .await?;

        // Update session state with the modified state
        session_state.state = update_state;

        // Compute next frontier: unconditional edges + conditional edges
        let mut next_frontier: Vec<NodeKind> = Vec::new();
        let app_edges = self.app.edges();
        let conditional_edges = self.app.conditional_edges();
        let snapshot = session_state.state.snapshot();
        for id in run_ids.iter() {
            // Unconditional edges
            if let Some(dests) = app_edges.get(id) {
                for d in dests {
                    if !next_frontier.contains(d) {
                        next_frontier.push(d.clone());
                    }
                }
            }
            // Conditional edges
            for ce in conditional_edges.iter().filter(|ce| &ce.from == id) {
                let target = if (ce.predicate)(&snapshot) {
                    &ce.yes
                } else {
                    &ce.no
                };
                if !next_frontier.contains(target) {
                    next_frontier.push(target.clone());
                }
            }
        }

        println!("Updated channels this step: {:?}", updated_channels);
        println!("Next frontier: {:?}", next_frontier);

        let completed =
            next_frontier.is_empty() || next_frontier.iter().all(|n| *n == NodeKind::End);

        // Update session state
        session_state.frontier = next_frontier.clone();

        let state_versions = StateVersions {
            messages_version: session_state.state.messages.version(),
            extra_version: session_state.state.extra.version(),
        };

        Ok(StepReport {
            step,
            ran_nodes: run_ids,
            skipped_nodes: step_result.skipped_nodes,
            updated_channels,
            next_frontier,
            state_versions,
            completed,
        })
    }

    /// Run until completion (End nodes or no frontier) - the canonical execution method
    pub async fn run_until_complete(
        &mut self,
        session_id: &str,
    ) -> Result<VersionedState, Box<dyn std::error::Error + Send + Sync>> {
        println!("== Begin run ==");

        loop {
            // Check if we're done before trying to run
            let session_state = self
                .sessions
                .get(session_id)
                .ok_or_else(|| format!("Session '{}' not found", session_id))?;

            if session_state.frontier.is_empty()
                || session_state.frontier.iter().all(|n| *n == NodeKind::End)
            {
                println!("Reached END at step {}", session_state.step);
                break;
            }

            // Run one step
            let step_result = self.run_step(session_id, StepOptions::default()).await?;

            match step_result {
                StepResult::Completed(report) => {
                    if report.completed {
                        break;
                    }
                }
                StepResult::Paused(_) => {
                    // This shouldn't happen with default options, but handle gracefully
                    return Err("Unexpected pause during run_until_complete".into());
                }
            }
        }

        println!("\n== Final state ==");
        let final_session = self
            .sessions
            .get(session_id)
            .ok_or_else(|| format!("Session '{}' not found", session_id))?;
        let final_state = final_session.state.clone();

        // Print final state summary (matching App::invoke output)
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

        Ok(final_state)
    }

    /// Get a snapshot of the current session state
    pub fn get_session(&self, session_id: &str) -> Option<&SessionState> {
        self.sessions.get(session_id)
    }

    /// List all session IDs
    pub fn list_sessions(&self) -> Vec<&String> {
        self.sessions.keys().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EdgePredicate, GraphBuilder};
    use crate::node::{NodeA, NodeB, NodeContext, NodePartial};
    use crate::runtimes::checkpointer::InMemoryCheckpointer;
    use crate::state::{StateSnapshot, VersionedState};
    use async_trait::async_trait;

    struct TestNode {
        message: String,
    }

    #[async_trait]
    impl crate::node::Node for TestNode {
        async fn run(&self, _snapshot: StateSnapshot, _ctx: NodeContext) -> NodePartial {
            NodePartial {
                messages: Some(vec![crate::message::Message {
                    role: "assistant".into(),
                    content: self.message.clone(),
                }]),
                extra: None,
            }
        }
    }

    fn make_test_app() -> App {
        let mut builder = GraphBuilder::new();
        builder = builder.add_node(NodeKind::Start, NodeA);
        builder = builder.add_node(
            NodeKind::Other("test".into()),
            TestNode {
                message: "test message".into(),
            },
        );
        builder = builder.add_node(NodeKind::End, NodeB);
        builder = builder.add_edge(NodeKind::Start, NodeKind::Other("test".into()));
        builder = builder.add_edge(NodeKind::Other("test".into()), NodeKind::End);
        builder = builder.set_entry(NodeKind::Start);
        builder.compile().unwrap()
    }

    #[tokio::test]
    async fn test_conditional_edge_routing() {
        // Predicate: true if extra contains key "go_yes"
        let pred: EdgePredicate =
            std::sync::Arc::new(|snap: &StateSnapshot| snap.extra.contains_key("go_yes"));
        let gb = GraphBuilder::new()
            .add_node(
                NodeKind::Start,
                TestNode {
                    message: "start".into(),
                },
            )
            .add_node(
                NodeKind::Other("Y".into()),
                TestNode {
                    message: "yes path".into(),
                },
            )
            .add_node(
                NodeKind::Other("N".into()),
                TestNode {
                    message: "no path".into(),
                },
            )
            .add_edge(NodeKind::Start, NodeKind::Start) // Add unconditional edge to START itself for initial frontier
            .add_conditional_edge(
                NodeKind::Start,
                NodeKind::Other("Y".into()),
                NodeKind::Other("N".into()),
                pred.clone(),
            )
            .set_entry(NodeKind::Start);
        let app = gb.compile().unwrap();
        let mut runner = AppRunner::new(app);
        // State with go_yes present
        let mut state = VersionedState::new_with_user_message("hi");
        state
            .extra
            .get_mut()
            .insert("go_yes".to_string(), serde_json::json!(1));
        runner
            .create_session("sess1".to_string(), state.clone())
            .unwrap();
        let report = runner
            .run_step("sess1", StepOptions::default())
            .await
            .unwrap();
        if let StepResult::Completed(rep) = report {
            assert!(rep.next_frontier.contains(&NodeKind::Other("Y".into())));
            assert!(!rep.next_frontier.contains(&NodeKind::Other("N".into())));
        } else {
            panic!("Expected completed step");
        }
        // State without go_yes
        let state2 = VersionedState::new_with_user_message("hi");
        runner
            .create_session("sess2".to_string(), state2.clone())
            .unwrap();
        let report2 = runner
            .run_step("sess2", StepOptions::default())
            .await
            .unwrap();
        if let StepResult::Completed(rep2) = report2 {
            assert!(rep2.next_frontier.contains(&NodeKind::Other("N".into())));
            assert!(!rep2.next_frontier.contains(&NodeKind::Other("Y".into())));
        } else {
            panic!("Expected completed step");
        }
    }

    #[tokio::test]
    async fn test_create_session() {
        let app = make_test_app();
        let mut runner = AppRunner::new(app);
        let initial_state = VersionedState::new_with_user_message("hello");

        let result = runner.create_session("test_session".into(), initial_state);
        assert!(result.is_ok());
        assert!(runner.get_session("test_session").is_some());
    }

    #[tokio::test]
    async fn test_run_step_basic() {
        let app = make_test_app();
        let mut runner = AppRunner::new(app);
        let initial_state = VersionedState::new_with_user_message("hello");

        runner
            .create_session("test_session".into(), initial_state)
            .unwrap();

        let result = runner
            .run_step("test_session", StepOptions::default())
            .await;
        assert!(result.is_ok());

        if let Ok(StepResult::Completed(report)) = result {
            assert_eq!(report.step, 1);
            assert_eq!(report.ran_nodes.len(), 1);
            assert!(report.updated_channels.contains(&"messages"));
        } else {
            panic!("Expected completed step, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_run_until_complete() {
        let app = make_test_app();
        let mut runner = AppRunner::new(app);
        let initial_state = VersionedState::new_with_user_message("hello");

        runner
            .create_session("test_session".into(), initial_state)
            .unwrap();

        let result = runner.run_until_complete("test_session").await;
        assert!(result.is_ok());

        let final_state = result.unwrap();
        assert_eq!(final_state.messages.len(), 2); // user + test node message
        assert_eq!(final_state.messages.version(), 2);
    }

    #[tokio::test]
    async fn test_interrupt_before() {
        let app = make_test_app();
        let mut runner = AppRunner::new(app);
        let initial_state = VersionedState::new_with_user_message("hello");

        runner
            .create_session("test_session".into(), initial_state)
            .unwrap();

        // Set interrupt before the test node
        let options = StepOptions {
            interrupt_before: vec![NodeKind::Other("test".into())],
            ..Default::default()
        };

        let result = runner.run_step("test_session", options).await;
        assert!(result.is_ok());

        if let Ok(StepResult::Paused(paused)) = result {
            assert!(matches!(paused.reason, PausedReason::BeforeNode(_)));
        } else {
            panic!("Expected paused step, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_interrupt_after() {
        let app = make_test_app();
        let mut runner = AppRunner::new(app);
        let initial_state = VersionedState::new_with_user_message("hello");

        runner
            .create_session("test_session".into(), initial_state)
            .unwrap();

        // Set interrupt after the "test" node (which runs in the first step)
        let options = StepOptions {
            interrupt_after: vec![NodeKind::Other("test".into())],
            ..Default::default()
        };

        let result = runner.run_step("test_session", options).await;
        assert!(result.is_ok());

        if let Ok(StepResult::Paused(paused)) = result {
            assert!(matches!(paused.reason, PausedReason::AfterNode(_)));
        } else {
            panic!("Expected paused step, got: {:?}", result);
        }
    }

    #[tokio::test]
    async fn test_resume_from_checkpoint() {
        let app = make_test_app();
        let cp = Arc::new(InMemoryCheckpointer::new());
        let mut runner = AppRunner::with_options(app, Some(cp.clone()), true);
        let initial_state = VersionedState::new_with_user_message("hello");
        runner
            .create_session("sessA".into(), initial_state)
            .unwrap();
        // run one step (should autosave)
        let _ = runner
            .run_step("sessA", StepOptions::default())
            .await
            .unwrap();
        // Create a NEW runner using same checkpointer and verify it restores
        let app2 = make_test_app();
        let mut runner2 = AppRunner::with_options(app2, Some(cp.clone()), true);
        // calling create_session with same id should load from checkpoint rather than reset
        runner2
            .create_session(
                "sessA".into(),
                VersionedState::new_with_user_message("ignored"),
            )
            .unwrap();
        let restored = runner2.get_session("sessA").unwrap();
        assert_eq!(restored.step, 1); // resumed after first step
        // Complete run
        let final_state = runner2.run_until_complete("sessA").await.unwrap();
        assert!(final_state.messages.len() >= 2); // user + test node (possibly more if node executes again)
    }
}

//! Checkpointer infrastructure (Week 1 goal)
//!
//! This initial implementation introduces a `Checkpointer` trait and an
//! in‑memory implementation (`InMemoryCheckpointer`). It is intentionally
//! minimal: it stores only the latest checkpoint per session (no history)
//! and performs no serialization (pure in‑process persistence). Later
//! extensions (Week 2+) can add:
//!   * Persistent backends (e.g. Postgres)
//!   * Incremental history / lineage
//!   * Compaction & retention policies
//!   * Structured metadata & tracing correlation IDs
//!
//! The design mirrors the plan in `doc/langgraph_port_plan.md` while keeping
//! integration incremental. A subsequent change will wire this into
//! `AppRunner` so session state flows through the trait instead of the
//! current internal HashMap.

use std::sync::RwLock;

use chrono::{DateTime, Utc};
use rustc_hash::FxHashMap;

use crate::{
    runtimes::runner::SessionState, schedulers::SchedulerState, state::VersionedState,
    types::NodeKind,
};

/// A durable snapshot of session execution state at a barrier boundary.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub session_id: String,
    pub step: u64,
    pub state: VersionedState,
    pub frontier: Vec<NodeKind>,
    pub versions_seen: FxHashMap<String, FxHashMap<String, u64>>, // scheduler gating
    pub concurrency_limit: usize,
    pub created_at: DateTime<Utc>,
}

impl Checkpoint {
    pub fn from_session(session_id: &str, session: &SessionState) -> Self {
        Self {
            session_id: session_id.to_string(),
            step: session.step,
            state: session.state.clone(),
            frontier: session.frontier.clone(),
            versions_seen: session.scheduler_state.versions_seen.clone(),
            concurrency_limit: session.scheduler.concurrency_limit,
            created_at: Utc::now(),
        }
    }
}

/// Errors from checkpointer operations.
#[derive(Debug)]
pub enum CheckpointerError {
    NotFound(String),
    Backend(String),
    Other(String),
}

/// Selects the backing implementation of the `Checkpointer` trait.
///
/// Variants:
/// * `InMemory` – Volatile process‑local storage. Fast, non‑durable; suitable for
///   tests and ephemeral runs.
/// * `SQLite` – Durable, file (or memory) backed storage using `SQLiteCheckpointer`
///   (see `runtimes::checkpointer_sqlite`). Persists step history and the latest
///   snapshot for session resumption.
///
/// Note:
/// The runtime previously had an unreachable wildcard match when exhaustively
/// enumerating these variants. If additional variants are added in the future,
/// they should be explicitly matched (or a deliberate catch‑all retained).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointerType {
    /// In‑memory (non‑durable) checkpointing.
    InMemory,
    /// SQLite‑backed durable checkpointing (see `SQLiteCheckpointer`).
    SQLite,
}

impl std::fmt::Display for CheckpointerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckpointerError::NotFound(s) => write!(f, "session not found: {s}"),
            CheckpointerError::Backend(s) => write!(f, "backend unavailable: {s}"),
            CheckpointerError::Other(s) => write!(f, "other: {s}"),
        }
    }
}

impl std::error::Error for CheckpointerError {}

pub type Result<T> = std::result::Result<T, CheckpointerError>;

/// Trait for saving & loading checkpoints for resumable execution.
///
/// Contract:
/// * `save` replaces the latest checkpoint for the session (idempotent on identical input).
/// * `load_latest` returns `Ok(None)` if no checkpoint exists.
/// * All methods are sync for now; async backends can add async wrappers later.
use async_trait::async_trait;

#[async_trait]
pub trait Checkpointer: Send + Sync {
    /// Persist (replace) the latest checkpoint for a session.
    async fn save(&self, checkpoint: Checkpoint) -> Result<()>;

    /// Load the latest checkpoint for a session (Ok(None) if not found).
    async fn load_latest(&self, session_id: &str) -> Result<Option<Checkpoint>>;

    /// List all session IDs known to the backend.
    async fn list_sessions(&self) -> Result<Vec<String>>;
}

/// Simple in‑memory checkpointer. Stores only the *latest* checkpoint per session.
#[derive(Default)]
pub struct InMemoryCheckpointer {
    inner: RwLock<FxHashMap<String, Checkpoint>>,
}

impl InMemoryCheckpointer {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(FxHashMap::default()),
        }
    }
}

#[async_trait]
impl Checkpointer for InMemoryCheckpointer {
    async fn save(&self, checkpoint: Checkpoint) -> Result<()> {
        let mut map = self
            .inner
            .write()
            .map_err(|e| CheckpointerError::Backend(format!("lock poisoned: {e}")))?;
        map.insert(checkpoint.session_id.clone(), checkpoint);
        Ok(())
    }

    async fn load_latest(&self, session_id: &str) -> Result<Option<Checkpoint>> {
        let map = self
            .inner
            .read()
            .map_err(|e| CheckpointerError::Backend(format!("lock poisoned: {e}")))?;
        Ok(map.get(session_id).cloned())
    }

    async fn list_sessions(&self) -> Result<Vec<String>> {
        let map = self
            .inner
            .read()
            .map_err(|e| CheckpointerError::Backend(format!("lock poisoned: {e}")))?;
        Ok(map.keys().cloned().collect())
    }
}

/// Utility to materialize a `SessionState` from a `Checkpoint`.
/// (Used later when wiring into `AppRunner`).
pub fn restore_session_state(cp: &Checkpoint) -> SessionState {
    use crate::schedulers::Scheduler;
    SessionState {
        state: cp.state.clone(),
        step: cp.step,
        frontier: cp.frontier.clone(),
        scheduler: Scheduler::new(cp.concurrency_limit),
        scheduler_state: SchedulerState {
            versions_seen: cp.versions_seen.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        channels::Channel, schedulers::SchedulerState, state::VersionedState, types::NodeKind,
    };

    #[tokio::test]
    async fn test_save_and_load_roundtrip() {
        let cp_store = InMemoryCheckpointer::new();
        let mut session = SessionState {
            state: VersionedState::new_with_user_message("hi"),
            step: 3,
            frontier: vec![NodeKind::Start],
            scheduler: crate::schedulers::Scheduler::new(4),
            scheduler_state: SchedulerState::default(),
        };
        session.scheduler_state.versions_seen.insert(
            "Start".into(),
            FxHashMap::from_iter([("messages".into(), 1_u64), ("extra".into(), 1_u64)]),
        );

        let cp = Checkpoint::from_session("sess1", &session);
        cp_store.save(cp.clone()).await.unwrap();

        let loaded = cp_store.load_latest("sess1").await.unwrap().unwrap();
        assert_eq!(loaded.step, 3);
        assert_eq!(loaded.frontier, vec![NodeKind::Start]);
        assert_eq!(
            loaded.versions_seen.get("Start").unwrap().get("messages"),
            Some(&1)
        );
        assert_eq!(
            loaded.state.messages.snapshot().len(),
            session.state.messages.snapshot().len()
        );
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let cp_store = InMemoryCheckpointer::new();
        let session = SessionState {
            state: VersionedState::new_with_user_message("x"),
            step: 0,
            frontier: vec![NodeKind::Start],
            scheduler: crate::schedulers::Scheduler::new(1),
            scheduler_state: SchedulerState::default(),
        };
        cp_store
            .save(Checkpoint::from_session("alpha", &session))
            .await
            .unwrap();
        cp_store
            .save(Checkpoint::from_session("beta", &session))
            .await
            .unwrap();
        let mut ids = cp_store.list_sessions().await.unwrap();
        ids.sort();
        assert_eq!(ids, vec!["alpha", "beta"]);
    }
}

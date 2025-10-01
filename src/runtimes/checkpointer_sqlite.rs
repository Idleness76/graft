/*!
SQLite Checkpointer

This module provides the `SQLiteCheckpointer` async implementation of the
`Checkpointer` trait defined in `runtimes/checkpointer.rs`.

Behavior:
- Uses serde-based persistence models (see `runtimes::persistence`) for
  encoding `VersionedState`, frontier node kinds, and `versions_seen`.
- When the `sqlite-migrations` feature is enabled (default), embedded
  migrations (`sqlx::migrate!("./migrations")`) are executed on connect;
  disabling the feature assumes external migration orchestration.

Design goals:
- Keep this module focused on database I/O; pure serialization lives in
  the persistence module.
- Minimize manual JSON handling (legacy helpers were removed).
*/

/* Data Model Mapping (Checkpoint -> DB):
  sessions.id                  <- checkpoint.session_id
  sessions.concurrency_limit   <- checkpoint.concurrency_limit
  steps.session_id             <- checkpoint.session_id
  steps.step                   <- checkpoint.step
  steps.state_json             <- serialized VersionedState
  steps.frontier_json          <- JSON array of encoded NodeKind
  steps.versions_seen_json     <- JSON object (node -> channel -> version)
  steps.ran_nodes_json         <- placeholder empty array (not in Checkpoint yet)
  steps.skipped_nodes_json     <- placeholder empty array
  steps.updated_channels_json  <- placeholder empty array OR null
*/

/* NodeKind Encoding (string form):
   Start        => "Start"
   End          => "End"
   Other(name)  => "Other:<name>"

   Stored state_json layout (example):
   {
     "messages": {
       "version": 3,
       "items": [
         {"role": "user", "content": "..."},
         {"role": "assistant", "content": "..."}
       ]
     },
     "extra": {
       "version": 2,
       "map": {
         "k1": <json value>,
         "k2": <json value>
       }
     }
   }

   Limitations / TODO:
   - Step history currently writes empty arrays for ran/skipped/updated (Checkpoint
     struct does not carry these yet). When the core checkpoint representation adds
     them, extend serialization accordingly.
   - No pagination / filtering helpers (future addition).
   - No optimistic concurrency guard (last_step monotonicity assumed by caller).
   - No JSON schema validation; assumes well‑formed internal state.
*/

use std::sync::Arc;

use chrono::{DateTime, Utc};
use miette::Diagnostic;
use serde_json::Value;
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use thiserror::Error;
use tracing::instrument;

use crate::{
    runtimes::checkpointer::{Checkpoint, Checkpointer, CheckpointerError, Result},
    runtimes::persistence::{PersistedState, PersistedVersionsSeen},
    state::VersionedState,
    types::NodeKind,
};

#[derive(Debug, Error, Diagnostic)]
pub enum SqliteCheckpointerError {
    #[error("SQLx error: {0}")]
    #[diagnostic(
        code(graft::sqlite::sqlx),
        help("Ensure the SQLite database URL is valid and accessible.")
    )]
    Sqlx(#[from] sqlx::Error),

    #[error("JSON serialization error: {0}")]
    #[diagnostic(
        code(graft::sqlite::serde),
        help("Check serialized shapes for state/frontier/versions_seen.")
    )]
    Serde(#[from] serde_json::Error),

    #[error("Missing persisted field: {0}")]
    #[diagnostic(
        code(graft::sqlite::missing),
        help("Backfill or re-run migrations to populate the missing field.")
    )]
    Missing(&'static str),

    #[error("Backend error: {0}")]
    #[diagnostic(code(graft::sqlite::backend))]
    Backend(String),

    #[error("Other error: {0}")]
    #[diagnostic(code(graft::sqlite::other))]
    Other(String),
}

impl From<SqliteCheckpointerError> for CheckpointerError {
    fn from(e: SqliteCheckpointerError) -> Self {
        match e {
            SqliteCheckpointerError::Sqlx(err) => CheckpointerError::Backend(err.to_string()),
            SqliteCheckpointerError::Serde(err) => CheckpointerError::Other(err.to_string()),
            SqliteCheckpointerError::Missing(what) => {
                CheckpointerError::Other(format!("missing persisted field: {what}"))
            }
            SqliteCheckpointerError::Backend(msg) => CheckpointerError::Backend(msg),
            SqliteCheckpointerError::Other(msg) => CheckpointerError::Other(msg),
        }
    }
}

/// SQLite-backed implementation of `Checkpointer`.
pub struct SQLiteCheckpointer {
    pool: Arc<SqlitePool>,
}

impl std::fmt::Debug for SQLiteCheckpointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SQLiteCheckpointer").finish()
    }
}

impl SQLiteCheckpointer {
    /// Connect (or create) a SQLite database at `database_url`.
    /// Example URL: "sqlite://graft.db"
    #[instrument(skip(database_url))]
    pub async fn connect(database_url: &str) -> std::result::Result<Self, CheckpointerError> {
        let pool = SqlitePool::connect(database_url)
            .await
            .map_err(|e| CheckpointerError::Backend(format!("connect error: {e}")))?;
        // Run embedded migrations only if the feature is enabled (idempotent).
        #[cfg(feature = "sqlite-migrations")]
        {
            if let Err(e) = sqlx::migrate!("./migrations").run(&pool).await {
                return Err(CheckpointerError::Backend(format!(
                    "migration failure: {e}"
                )));
            }
        }
        #[cfg(not(feature = "sqlite-migrations"))]
        {
            // Feature disabled: assume external migration orchestration already applied schema.
        }
        Ok(Self {
            pool: Arc::new(pool),
        })
    }
}

#[async_trait::async_trait]
impl Checkpointer for SQLiteCheckpointer {
    #[instrument(skip(self, checkpoint), err)]
    async fn save(&self, checkpoint: Checkpoint) -> Result<()> {
        // Serialize using persistence module (serde-based)
        let persisted_state = PersistedState::from(&checkpoint.state);
        let state_json = serde_json::to_string(&persisted_state)
            .map_err(|e| CheckpointerError::Other(format!("state serialize: {e}")))?;
        let frontier_enc: Vec<String> = checkpoint.frontier.iter().map(|k| k.encode()).collect();
        let frontier_json = serde_json::to_string(&frontier_enc)
            .map_err(|e| CheckpointerError::Other(format!("frontier serialize: {e}")))?;
        let persisted_vs = PersistedVersionsSeen(checkpoint.versions_seen.clone());
        let versions_seen_json = serde_json::to_string(&persisted_vs)
            .map_err(|e| CheckpointerError::Other(format!("versions_seen serialize: {e}")))?;

        // Placeholder arrays (StepReport not folded into Checkpoint yet)
        let empty_arr = "[]";

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| CheckpointerError::Backend(format!("tx begin: {e}")))?;

        // Ensure session row
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO sessions (id, concurrency_limit)
            VALUES (?1, ?2)
        "#,
        )
        .bind(&checkpoint.session_id)
        .bind(checkpoint.concurrency_limit as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| CheckpointerError::Backend(format!("insert session: {e}")))?;

        // Insert or replace step row (allows idempotent re-save of same step)
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO steps (
                session_id,
                step,
                state_json,
                frontier_json,
                versions_seen_json,
                ran_nodes_json,
                skipped_nodes_json,
                updated_channels_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        )
        .bind(&checkpoint.session_id)
        .bind(checkpoint.step as i64)
        .bind(&state_json)
        .bind(&frontier_json)
        .bind(&versions_seen_json)
        .bind(empty_arr) // ran_nodes_json
        .bind(empty_arr) // skipped_nodes_json
        .bind(empty_arr) // updated_channels_json
        .execute(&mut *tx)
        .await
        .map_err(|e| CheckpointerError::Backend(format!("insert step: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| CheckpointerError::Backend(format!("tx commit: {e}")))?;

        Ok(())
    }

    #[instrument(skip(self, session_id), err)]
    async fn load_latest(&self, session_id: &str) -> Result<Option<Checkpoint>> {
        let row_opt: Option<SqliteRow> = sqlx::query(
            r#"
            SELECT
                s.id,
                s.last_step,
                s.last_state_json,
                s.last_frontier_json,
                s.last_versions_seen_json,
                s.concurrency_limit,
                s.updated_at
            FROM sessions s
            WHERE s.id = ?1
            "#,
        )
        .bind(session_id)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| CheckpointerError::Backend(format!("select latest: {e}")))?;

        let row = match row_opt {
            Some(r) => r,
            None => return Ok(None),
        };

        let last_step: i64 = row.get("last_step");

        let state_json: Option<String> = row
            .try_get("last_state_json")
            .map_err(|e| CheckpointerError::Backend(format!("last_state_json read: {e}")))?;
        let frontier_json: Option<String> = row
            .try_get("last_frontier_json")
            .map_err(|e| CheckpointerError::Backend(format!("last_frontier_json read: {e}")))?;
        let versions_seen_json: Option<String> =
            row.try_get("last_versions_seen_json").map_err(|e| {
                CheckpointerError::Backend(format!("last_versions_seen_json read: {e}"))
            })?;
        let concurrency_limit: i64 = row.get("concurrency_limit");
        let updated_at_str: String = row.get("updated_at");

        if last_step == 0 && state_json.is_none() {
            // Session row exists but no checkpoint has been persisted yet.
            return Ok(None);
        }

        let state_payload = state_json.ok_or_else(|| {
            CheckpointerError::Other("missing state_json for persisted checkpoint".into())
        })?;
        let frontier_payload = frontier_json.ok_or_else(|| {
            CheckpointerError::Other("missing frontier_json for persisted checkpoint".into())
        })?;
        let versions_seen_payload = versions_seen_json.ok_or_else(|| {
            CheckpointerError::Other("missing versions_seen_json for persisted checkpoint".into())
        })?;

        let state_val: Value = serde_json::from_str(&state_payload)
            .map_err(|e| CheckpointerError::Other(format!("state parse: {e}")))?;
        let frontier_val: Value = serde_json::from_str(&frontier_payload)
            .map_err(|e| CheckpointerError::Other(format!("frontier parse: {e}")))?;
        let versions_seen_val: Value = serde_json::from_str(&versions_seen_payload)
            .map_err(|e| CheckpointerError::Other(format!("versions_seen parse: {e}")))?;

        // Deserialize using persistence models
        let persisted_state: PersistedState = serde_json::from_value(state_val)
            .map_err(|e| CheckpointerError::Other(format!("state parse (serde): {e}")))?;
        let state = VersionedState::try_from(persisted_state)
            .map_err(|e| CheckpointerError::Other(format!("state convert: {e}")))?;
        let frontier: Vec<NodeKind> = frontier_val
            .as_array()
            .ok_or_else(|| CheckpointerError::Other("frontier not array".into()))?
            .iter()
            .filter_map(|v| v.as_str())
            .map(NodeKind::decode)
            .collect();
        let persisted_vs: PersistedVersionsSeen = serde_json::from_value(versions_seen_val)
            .map_err(|e| CheckpointerError::Other(format!("versions_seen parse: {e}")))?;
        let versions_seen = persisted_vs.0;

        let created_at = DateTime::parse_from_rfc3339(&updated_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(Some(Checkpoint {
            session_id: session_id.to_string(),
            step: last_step as u64,
            state,
            frontier,
            versions_seen,
            concurrency_limit: concurrency_limit as usize,
            created_at,
        }))
    }

    #[instrument(skip(self), err)]
    async fn list_sessions(&self) -> Result<Vec<String>> {
        let rows = sqlx::query(
            r#"
            SELECT id FROM sessions
            ORDER BY updated_at DESC
            "#,
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| CheckpointerError::Backend(format!("list sessions: {e}")))?;

        Ok(rows.into_iter().map(|r| r.get::<String, _>("id")).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::Channel;
    use crate::runtimes::checkpointer::restore_session_state;
    use crate::runtimes::checkpointer::Checkpoint as CP;

    use crate::state::VersionedState;
    use rustc_hash::FxHashMap;
    use std::fs::File;
    use tempfile::TempDir;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_sqlite_checkpointer_roundtrip() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect sqlite memory");
        let mut state = VersionedState::new_with_user_message("hello");
        state
            .extra
            .get_mut()
            .insert("k".into(), serde_json::json!(42));

        let mut versions_seen: FxHashMap<String, FxHashMap<String, u64>> = FxHashMap::default();
        versions_seen.insert(
            "Start".into(),
            FxHashMap::from_iter([("messages".into(), 1_u64), ("extra".into(), 1_u64)]),
        );

        let cp_struct = CP {
            session_id: "sessX".into(),
            step: 1,
            state: state.clone(),
            frontier: vec![NodeKind::End],
            versions_seen: versions_seen.clone(),
            concurrency_limit: 4,
            created_at: Utc::now(),
        };

        // Save (async trait method)
        cp.save(cp_struct.clone()).await.expect("save");

        // Load
        let loaded = cp
            .load_latest("sessX")
            .await
            .expect("load_latest")
            .expect("Some checkpoint");
        assert_eq!(loaded.step, 1);
        assert_eq!(loaded.frontier, vec![NodeKind::End]);
        assert_eq!(
            loaded
                .versions_seen
                .get("Start")
                .and_then(|m| m.get("messages"))
                .copied(),
            Some(1)
        );
        assert_eq!(loaded.state.messages.snapshot()[0].role, "user");
        assert_eq!(
            loaded.state.extra.snapshot().get("k"),
            Some(&serde_json::json!(42))
        );

        // Restore session state utility compatibility
        let session_state = restore_session_state(&loaded);
        assert_eq!(session_state.step, 1);
        assert_eq!(session_state.frontier.len(), 1);
        assert_eq!(session_state.scheduler.concurrency_limit, 4);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_list_sessions() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect");
        for i in 0..3 {
            let s_id = format!("s{i}");
            let state = VersionedState::new_with_user_message("x");
            let cp_struct = Checkpoint {
                session_id: s_id.clone(),
                step: 1,
                state: state.clone(),
                frontier: vec![NodeKind::End],
                versions_seen: FxHashMap::default(),
                concurrency_limit: 1,
                created_at: Utc::now(),
            };
            cp.save(cp_struct).await.unwrap();
        }
        let mut sessions = cp.list_sessions().await.unwrap();
        sessions.sort();
        assert_eq!(sessions, vec!["s0", "s1", "s2"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_load_nonexistent() {
        let cp = SQLiteCheckpointer::connect("sqlite::memory:")
            .await
            .expect("connect");
        let res = cp.load_latest("nope").await.unwrap();
        assert!(res.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_resume_step_zero_checkpoint() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("checkpoint.db");
        let db_url = format!("sqlite://{}", db_path.to_string_lossy());

        File::create(&db_path).expect("create db file");

        let cp_initial = SQLiteCheckpointer::connect(&db_url)
            .await
            .expect("connect initial");

        let state = VersionedState::new_with_user_message("hi");
        let checkpoint = Checkpoint {
            session_id: "sess0".into(),
            step: 0,
            state: state.clone(),
            frontier: vec![NodeKind::Start],
            versions_seen: FxHashMap::default(),
            concurrency_limit: 2,
            created_at: Utc::now(),
        };

        cp_initial.save(checkpoint).await.expect("save step0");
        drop(cp_initial);

        let cp_reloaded = SQLiteCheckpointer::connect(&db_url)
            .await
            .expect("reconnect");
        let loaded = cp_reloaded
            .load_latest("sess0")
            .await
            .expect("load_latest result")
            .expect("checkpoint present");

        assert_eq!(loaded.step, 0);
        assert_eq!(loaded.frontier, vec![NodeKind::Start]);
        assert_eq!(loaded.state.messages.snapshot(), state.messages.snapshot(),);
        assert!(loaded.versions_seen.is_empty());
    }
}

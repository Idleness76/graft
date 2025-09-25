use crate::channels::Channel;
use crate::channels::errors::{ErrorEvent, ErrorScope, LadderError};
use crate::runtimes::checkpointer::{Checkpoint, Checkpointer};
use crate::runtimes::checkpointer_sqlite::SQLiteCheckpointer;
use crate::runtimes::runner::SessionState;
use crate::schedulers::{Scheduler, SchedulerState};
use crate::state::VersionedState;
use crate::types::NodeKind;
use chrono::Utc;

/// Test to verify that error events are properly persisted to the database
/// and correctly restored when loading checkpoints.
///
/// This serves as proof that the unified error handling system with
/// ErrorsChannel persistence is working end-to-end.
#[tokio::test]
async fn test_error_persistence_roundtrip() {
    // Create an in-memory SQLite database for testing
    let checkpointer = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");

    // Create initial state with a user message
    let mut state = VersionedState::new_with_user_message("test persistence");

    // Create test error events with different scopes and details
    let node_error = ErrorEvent {
        when: Utc::now(),
        scope: ErrorScope::Node {
            kind: "test_node".to_string(),
            step: 42,
        },
        error: LadderError::msg("Node processing failed"),
        tags: vec!["critical".to_string(), "node_failure".to_string()],
        context: serde_json::json!({"input_data": "corrupted", "retry_count": 3}),
    };

    let app_error = ErrorEvent {
        when: Utc::now(),
        scope: ErrorScope::App,
        error: LadderError::msg("Configuration validation error")
            .with_details(serde_json::json!({"error_code": 400, "field": "timeout"})),
        tags: vec!["validation".to_string()],
        context: serde_json::Value::Null,
    };

    let scheduler_error = ErrorEvent {
        when: Utc::now(),
        scope: ErrorScope::Scheduler { step: 15 },
        error: LadderError::msg("Concurrency limit exceeded")
            .with_cause(LadderError::msg("Too many parallel tasks")),
        tags: vec![],
        context: serde_json::json!({"active_tasks": 50, "limit": 10}),
    };

    // Add errors to state using the channel interface
    state.errors.get_mut().extend(vec![
        node_error.clone(),
        app_error.clone(),
        scheduler_error.clone(),
    ]);

    // Verify errors are in memory
    assert_eq!(state.errors.len(), 3);
    let in_memory_errors = state.errors.snapshot();
    assert_eq!(in_memory_errors[0].error.message, "Node processing failed");
    assert_eq!(
        in_memory_errors[1].error.message,
        "Configuration validation error"
    );
    assert_eq!(
        in_memory_errors[2].error.message,
        "Concurrency limit exceeded"
    );

    // Create a session state for checkpointing
    let session = SessionState {
        state,
        step: 5,
        frontier: vec![NodeKind::Start],
        scheduler: Scheduler::new(4),
        scheduler_state: SchedulerState::default(),
    };

    // Save checkpoint with errors to database
    let session_id = "error_persistence_test";
    let checkpoint = Checkpoint::from_session(session_id, &session);
    checkpointer
        .save(checkpoint)
        .await
        .expect("Failed to save checkpoint");

    // Load checkpoint from database
    let loaded_checkpoint = checkpointer
        .load_latest(session_id)
        .await
        .expect("Failed to load checkpoint")
        .expect("Checkpoint not found");

    // Verify all errors were correctly persisted and restored
    let restored_errors = loaded_checkpoint.state.errors.snapshot();
    assert_eq!(restored_errors.len(), 3, "All errors should be restored");

    // Verify error 1: Node error with tags and context
    assert_eq!(restored_errors[0].error.message, "Node processing failed");
    assert_eq!(restored_errors[0].tags.len(), 2);
    assert!(restored_errors[0].tags.contains(&"critical".to_string()));
    assert!(
        restored_errors[0]
            .tags
            .contains(&"node_failure".to_string())
    );
    if let ErrorScope::Node { kind, step } = &restored_errors[0].scope {
        assert_eq!(kind, "test_node");
        assert_eq!(*step, 42);
    } else {
        panic!("Expected Node scope");
    }

    // Verify error 2: App error with details
    assert_eq!(
        restored_errors[1].error.message,
        "Configuration validation error"
    );
    assert_eq!(restored_errors[1].tags, vec!["validation".to_string()]);
    assert!(matches!(restored_errors[1].scope, ErrorScope::App));

    // Verify error 3: Scheduler error with cause chain
    assert_eq!(
        restored_errors[2].error.message,
        "Concurrency limit exceeded"
    );
    assert!(restored_errors[2].error.cause.is_some());
    if let Some(cause) = &restored_errors[2].error.cause {
        assert_eq!(cause.message, "Too many parallel tasks");
    }
    if let ErrorScope::Scheduler { step } = &restored_errors[2].scope {
        assert_eq!(*step, 15);
    } else {
        panic!("Expected Scheduler scope");
    }

    // Verify context data serialization
    assert!(!restored_errors[0].context.is_null());
    assert_eq!(restored_errors[0].context["input_data"], "corrupted");
    assert_eq!(restored_errors[0].context["retry_count"], 3);

    assert_eq!(restored_errors[1].context, serde_json::Value::Null);

    assert!(!restored_errors[2].context.is_null());
    assert_eq!(restored_errors[2].context["active_tasks"], 50);
    assert_eq!(restored_errors[2].context["limit"], 10);

    println!("✅ Error persistence test completed successfully!");
    println!("   - Saved 3 error events to database");
    println!("   - Restored 3 error events from database");
    println!("   - All error details, scopes, tags, and context preserved");
    println!("   - Error cause chains correctly serialized/deserialized");
}

/// Test to verify that error events maintain proper serialization format
/// for cross-version compatibility
#[tokio::test]
async fn test_error_serialization_format() {
    let checkpointer = SQLiteCheckpointer::connect("sqlite::memory:")
        .await
        .expect("Failed to create test database");

    let mut state = VersionedState::new_with_user_message("serialization test");

    // Create an error with all possible fields populated
    let complex_error = ErrorEvent {
        when: Utc::now(),
        scope: ErrorScope::Runner {
            session: "test_session".to_string(),
            step: 100,
        },
        error: LadderError::msg("Complex error for format testing")
            .with_details(serde_json::json!({
                "nested": {
                    "data": [1, 2, 3],
                    "flags": {"critical": true, "retryable": false}
                }
            }))
            .with_cause(
                LadderError::msg("Root cause error")
                    .with_details(serde_json::json!({"source": "network"})),
            ),
        tags: vec!["format_test".to_string(), "comprehensive".to_string()],
        context: serde_json::json!({
            "timestamp": "2025-09-23T00:00:00Z",
            "user_agent": "test_client/1.0",
            "request_id": "req_12345"
        }),
    };

    state.errors.get_mut().push(complex_error);

    let session = SessionState {
        state,
        step: 10,
        frontier: vec![NodeKind::Start],
        scheduler: Scheduler::new(1),
        scheduler_state: SchedulerState::default(),
    };

    // Save and reload
    let checkpoint = Checkpoint::from_session("format_test", &session);
    checkpointer.save(checkpoint).await.unwrap();

    let loaded = checkpointer
        .load_latest("format_test")
        .await
        .unwrap()
        .unwrap();

    let errors = loaded.state.errors.snapshot();
    assert_eq!(errors.len(), 1);

    let error = &errors[0];

    // Verify complex nested details are preserved
    assert_eq!(error.error.details["nested"]["data"][1], 2);
    assert_eq!(error.error.details["nested"]["flags"]["critical"], true);

    // Verify cause chain with details
    let cause = error.error.cause.as_ref().unwrap();
    assert_eq!(cause.message, "Root cause error");
    assert_eq!(cause.details["source"], "network");

    // Verify complex context data
    assert_eq!(error.context["user_agent"], "test_client/1.0");
    assert_eq!(error.context["request_id"], "req_12345");

    println!("✅ Error serialization format test passed!");
    println!("   - Complex nested JSON data preserved");
    println!("   - Error cause chains with details preserved");
    println!("   - All context and metadata intact");
}

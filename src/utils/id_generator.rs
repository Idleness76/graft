//! ID Generator for run, step, and node IDs.
//! Provides utilities for generating unique IDs using UUIDs.
//! Supports thread ID, step ID, node ID, and helpers for resuming from checkpoints.

use std::fmt;
use uuid::Uuid;

/// Configuration for ID generation.
#[derive(Debug, Clone)]
pub struct IdConfig {
    /// Random seed for deterministic ID generation (optional).
    /// Note: UUID v4 is random; for full determinism, consider v5 or custom seeding.
    pub seed: Option<u64>,
}

impl fmt::Display for IdConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "seed: {}", self.seed.unwrap_or(0))
    }
}

/// ID Generator, wrapping uuid for unique ID generation.
#[derive(Debug)]
pub struct IdGenerator {
    config: IdConfig,
}

impl IdGenerator {
    /// Create a new ID generator with default config.
    pub fn new() -> Self {
        Self {
            config: IdConfig { seed: None },
        }
    }

    pub fn generate_id(&self) -> String {
        // Use seed if present, else random UUID
        if let Some(seed) = self.config.seed {
            format!("ID-seeded-{}", seed) // Placeholder; replace with actual seeded logic
        } else {
            self.generate_guid()
        }
    }

    /// Create with custom config.
    pub fn with_config(config: IdConfig) -> Self {
        Self { config }
    }

    /// Generate a UUID v4 as a string.
    pub fn generate_guid(&self) -> String {
        Uuid::new_v4().to_string()
    }

    /// Generate an ID with a specific prefix.
    pub fn generate_id_with_prefix(&self, prefix: &str) -> String {
        format!("{}-{}", prefix, self.generate_guid())
    }

    /// Generate a run ID (e.g., for a full execution).
    pub fn generate_run_id(&self) -> String {
        self.generate_id_with_prefix("run")
    }

    /// Generate a step ID.
    pub fn generate_step_id(&self) -> String {
        self.generate_id_with_prefix("step")
    }

    /// Generate a node ID.
    pub fn generate_node_id(&self) -> String {
        self.generate_id_with_prefix("node")
    }

    /// Helper to generate a random ID.
    pub fn generate_random_id(&self) -> String {
        self.generate_guid()
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

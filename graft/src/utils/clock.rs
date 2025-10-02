//! Injectable Clock for checkpoints.
//! Provides a mockable time source, wrapping chrono for system time.

use chrono::Utc;

/// Trait for time sources.
pub trait Clock {
    fn now(&self) -> u64;
}

/// System clock, wrapping chrono.
#[derive(Debug)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        Utc::now().timestamp() as u64
    }
}

/// Mock clock for testing.
#[derive(Debug)]
pub struct MockClock {
    pub current_time: u64,
}

impl MockClock {
    pub fn new(start_time: u64) -> Self {
        Self {
            current_time: start_time,
        }
    }

    pub fn advance(&mut self, seconds: u64) {
        self.current_time += seconds;
    }
}

impl Clock for MockClock {
    fn now(&self) -> u64 {
        self.current_time
    }
}

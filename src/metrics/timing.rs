//! Operation timing context for detailed performance tracking.

use std::time::{Duration, Instant};

/// Tracks timing breakdown for a storage operation.
///
/// Allows detailed instrumentation of operation phases without
/// allocating per-operation. Uses stack-based timing.
#[derive(Clone, Debug, Default)]
pub struct OperationTiming {
    /// Total operation duration
    pub total_micros: u64,
    /// Time in WAL append phase
    pub wal_micros: u64,
    /// Time in index mutation phase
    pub index_micros: u64,
    /// Time in cache operations
    pub cache_micros: u64,
    /// Time in data store operations
    pub store_micros: u64,
}

impl OperationTiming {
    /// Create a timing tracker for the current operation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record timing for a phase.
    pub fn record_phase(&mut self, phase: TimingPhase, duration: Duration) {
        let micros = duration.as_micros() as u64;
        match phase {
            TimingPhase::WalAppend => self.wal_micros = micros,
            TimingPhase::IndexMutation => self.index_micros = micros,
            TimingPhase::CacheOp => self.cache_micros = micros,
            TimingPhase::StoreOp => self.store_micros = micros,
        }
    }

    /// Set total operation duration.
    pub fn set_total(&mut self, duration: Duration) {
        self.total_micros = duration.as_micros() as u64;
    }
}

/// Phases of a storage operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimingPhase {
    WalAppend,
    IndexMutation,
    CacheOp,
    StoreOp,
}

/// RAII timer for measuring operation phase duration.
pub struct PhaseTimer {
    start: Instant,
    phase: TimingPhase,
}

impl PhaseTimer {
    /// Start timing a phase.
    pub fn start(phase: TimingPhase) -> Self {
        Self {
            start: Instant::now(),
            phase,
        }
    }

    /// Get the phase being timed.
    pub fn phase(&self) -> TimingPhase {
        self.phase
    }

    /// Get elapsed duration without stopping the timer.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Consume the timer and return the duration.
    pub fn finish(self) -> Duration {
        self.start.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn timing_context_records_phases() {
        let mut timing = OperationTiming::new();
        timing.record_phase(TimingPhase::WalAppend, Duration::from_millis(1));
        timing.record_phase(TimingPhase::IndexMutation, Duration::from_millis(2));

        assert_eq!(timing.wal_micros, 1000);
        assert_eq!(timing.index_micros, 2000);
    }

    #[test]
    fn phase_timer_measures_elapsed() {
        let timer = PhaseTimer::start(TimingPhase::StoreOp);
        thread::sleep(Duration::from_millis(10));
        let elapsed = timer.finish();

        assert!(elapsed.as_millis() >= 10);
        assert!(elapsed.as_millis() < 50); // Some buffer for test flakiness
    }

    #[test]
    fn phase_timer_can_peek_without_consuming() {
        let timer = PhaseTimer::start(TimingPhase::CacheOp);
        thread::sleep(Duration::from_millis(5));
        let _peek = timer.elapsed();
        let finished = timer.finish();

        assert!(finished.as_millis() >= 5);
    }
}

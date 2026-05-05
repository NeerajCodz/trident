use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct IoRateLimiter {
    bytes_per_second: usize,
    window_start: Instant,
    consumed_in_window: usize,
}

impl IoRateLimiter {
    pub fn new(bytes_per_second: usize) -> Self {
        Self {
            bytes_per_second,
            window_start: Instant::now(),
            consumed_in_window: 0,
        }
    }

    pub fn consume(&mut self, bytes: usize) {
        if self.bytes_per_second == 0 || bytes == 0 {
            return;
        }
        let elapsed = self.window_start.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.window_start = Instant::now();
            self.consumed_in_window = 0;
        }
        self.consumed_in_window = self.consumed_in_window.saturating_add(bytes);
        if self.consumed_in_window <= self.bytes_per_second {
            return;
        }
        let overflow = self.consumed_in_window - self.bytes_per_second;
        let sleep_secs = overflow as f64 / self.bytes_per_second as f64;
        let sleep_duration = Duration::from_secs_f64(sleep_secs.min(0.250));
        if !sleep_duration.is_zero() {
            thread::sleep(sleep_duration);
        }
    }
}

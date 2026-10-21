use std::time::{Duration, Instant};

/// Measure active wall time independently of scheduler ticks or event volume.
/// Fractional seconds survive each sample, so frequent messages cannot lose time.
pub(super) struct Clock {
    sampled: Instant,
    remainder: Duration,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            sampled: Instant::now(),
            remainder: Duration::ZERO,
        }
    }

    pub fn sample(&mut self, active: bool) -> u64 {
        let now = Instant::now();
        let interval = now.duration_since(self.sampled);
        self.sampled = now;
        if active {
            self.remainder += interval;
        }
        let seconds = self.remainder.as_secs();
        self.remainder -= Duration::from_secs(seconds);
        seconds
    }
}

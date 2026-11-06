//! Wall-time comparison for original, unmodified native update deltas.
use std::time::{Duration, Instant};

pub(super) struct Window {
    start: Instant,
    ticks: u64,
    native_ms: u64,
    invalid: u64,
    min: i32,
    max: i32,
}

pub(super) struct Sample {
    pub wall_ms: f64,
    pub native_ms: u64,
    pub ticks: u64,
    pub invalid: u64,
    pub min: i32,
    pub max: i32,
}

impl Window {
    pub fn new(now: Instant) -> Self {
        Self { start: now, ticks: 0, native_ms: 0, invalid: 0, min: i32::MAX, max: i32::MIN }
    }

    pub fn push(&mut self, delta: i32, now: Instant) -> Option<Sample> {
        self.ticks += 1;
        self.min = self.min.min(delta);
        self.max = self.max.max(delta);
        if delta < 0 { self.invalid += 1; } else { self.native_ms += delta as u64; }
        let elapsed = now.saturating_duration_since(self.start);
        if elapsed < Duration::from_secs(2) { return None; }
        let sample = Sample { wall_ms: elapsed.as_secs_f64() * 1000.0,
            native_ms: self.native_ms, ticks: self.ticks, invalid: self.invalid,
            min: self.min, max: self.max };
        *self = Self::new(now);
        Some(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exposes_half_speed_without_changing_deltas_or_fps() {
        let start = Instant::now();
        let mut clock = Window::new(start);
        for tick in 1..120 {
            assert!(clock.push(8, start + Duration::from_nanos(tick * 2_000_000_000 / 120)).is_none());
        }
        let sample = clock.push(8, start + Duration::from_secs(2)).unwrap();
        assert_eq!((sample.ticks, sample.native_ms), (120, 960));
        assert_eq!(sample.wall_ms, 2000.0);
        assert_eq!((sample.min, sample.max, sample.invalid), (8, 8, 0));
    }
    #[test]
    fn negative_native_time_is_reported_and_windows_do_not_accumulate_old_values() {
        let start = Instant::now();
        let mut clock = Window::new(start);
        clock.push(-1, start + Duration::from_secs(1));
        let first = clock.push(20, start + Duration::from_secs(2)).unwrap();
        assert_eq!((first.native_ms, first.invalid, first.min), (20, 1, -1));
        let next = clock.push(2000, start + Duration::from_secs(4)).unwrap();
        assert_eq!((next.ticks, next.native_ms, next.invalid), (1, 2000, 0));
    }
}

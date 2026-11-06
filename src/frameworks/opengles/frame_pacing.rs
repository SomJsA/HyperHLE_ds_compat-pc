//! Absolute present deadlines. GPU work and swap/vsync consume the same frame
//! budget as guest work; they must not be followed by an outdated sleep delay.
use std::time::{Duration, Instant};

pub(super) fn deadline(next_due: &mut Option<Instant>, fps: Option<f64>, now: Instant) -> Option<Instant> {
    let Some(fps) = fps.filter(|fps| fps.is_finite() && *fps > 0.0) else {
        *next_due = None;
        return None;
    };
    let interval = 1.0 / fps;
    let period = Duration::from_secs_f64(interval);
    let Some(due) = *next_due else {
        *next_due = now.checked_add(period);
        return None;
    };
    *next_due = if now.saturating_duration_since(due) > period {
        // Keep at most one interval of slop, including after loading stalls.
        let periods = (now.saturating_duration_since(due).as_secs_f64() / interval).ceil();
        due.checked_add(Duration::from_secs_f64(interval * periods))
    } else {
        due.checked_add(period)
    };
    (now < due).then_some(due)
}

pub(super) fn remaining(deadline: Option<Instant>, now: Instant) -> Option<Duration> {
    deadline.and_then(|due| due.checked_duration_since(now)).filter(|delay| !delay.is_zero())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_and_vsync_time_are_deducted_from_the_sleep_budget() {
        let start = Instant::now();
        let mut next = None;
        assert_eq!(deadline(&mut next, Some(60.0), start), None);
        let due = deadline(&mut next, Some(60.0), start + Duration::from_millis(4));
        let after_present = remaining(due, start + Duration::from_millis(13)).unwrap();
        assert!(after_present > Duration::from_millis(3));
        assert!(after_present < Duration::from_millis(4));
        assert_eq!(remaining(due, start + Duration::from_millis(20)), None);
    }

    #[test]
    fn late_frames_do_not_sleep_and_disabling_the_cap_resets_the_schedule() {
        let start = Instant::now();
        let mut next = None;
        deadline(&mut next, Some(60.0), start);
        assert_eq!(deadline(&mut next, Some(60.0), start + Duration::from_secs(2)), None);
        assert!(next.unwrap() <= start + Duration::from_secs(2) + Duration::from_millis(17));
        assert_eq!(deadline(&mut next, None, start + Duration::from_secs(3)), None);
        assert_eq!(next, None);
        assert_eq!(deadline(&mut next, Some(60.0), start + Duration::from_secs(4)), None);
        assert_eq!(remaining(Some(start), start), None);
    }

    #[test]
    fn seventy_is_a_real_deadline_not_a_game_clock_multiplier() {
        let start = Instant::now();
        for fps in [30.0, 60.0, 70.0] {
            let mut next = None;
            deadline(&mut next, Some(fps), start);
            let due = deadline(&mut next, Some(fps), start + Duration::from_millis(3));
            let delay = remaining(due, start + Duration::from_millis(12)).unwrap();
            assert!((delay.as_secs_f64() - (1.0 / fps - 0.012)).abs() < 0.000001);
        }
    }
}

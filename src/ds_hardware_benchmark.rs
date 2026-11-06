//! Optional user-requested real-present sample. No capture, game mutation or IO.
#[derive(Debug, Clone)]
pub struct Result {
    pub frames: usize,
    pub seconds: f64,
    pub fps: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub over50: usize,
}
pub struct Sample {
    warmup: usize,
    valid_us: u64,
    frames: Vec<u64>,
    target_us: u64,
}
impl Sample {
    pub fn new() -> Self { Self { warmup: 60, valid_us: 0, frames: Vec::with_capacity(4096), target_us: 30_000_000 } }
    pub fn interrupt(&mut self) { self.warmup=60; }
    pub fn push(&mut self, dt_us: u64, live: bool) -> Option<Result> {
        if !live || dt_us == 0 { self.warmup = 60; return None; }
        if self.warmup > 0 { self.warmup -= 1; return None; }
        if self.frames.len() >= 100_000 { return None; }
        self.frames.push(dt_us); self.valid_us += dt_us;
        if self.valid_us < self.target_us { return None; }
        let mut sorted = self.frames.clone(); sorted.sort_unstable();
        let rank = |percent: usize| sorted[((sorted.len() * percent + 99) / 100).saturating_sub(1)] as f64 / 1000.0;
        Some(Result { frames: sorted.len(), seconds: self.valid_us as f64 / 1_000_000.0,
            fps: sorted.len() as f64 * 1_000_000.0 / self.valid_us as f64,
            p95_ms: rank(95), p99_ms: rank(99), over50: sorted.iter().filter(|v| **v > 50_000).count() })
    }
}
pub fn live_state(state: u32, frame: u64, sampled_frame: u64) -> bool {
    // Include native fire/melee and the entire pre-death gameplay interval.
    // A short positive whitelist omitted valid state3 firing frames.
    matches!(state, 0..=18 | 25) && sampled_frame <= frame && frame - sampled_frame <= 2
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn excludes_cutscene_death_unknown_and_stale_state() {
        for state in [26, 29, u32::MAX] { assert!(!live_state(state, 200, 200)); }
        assert!(!live_state(0, 200, 190));
        assert!(live_state(9, 200, 199));
        for state in [3,4,11,18,25] { assert!(live_state(state,200,199)); }
        for state in [19,20,21,22,23,24,27,28,30] { assert!(!live_state(state,200,199)); }
    }
    #[test] fn menu_and_loading_cannot_complete_a_sample() {
        let mut s=Sample::new();
        for _ in 0..10000 { assert!(s.push(16_667, false).is_none()); }
        assert_eq!(s.valid_us,0);assert!(s.frames.is_empty());
    }
    #[test] fn warmup_and_pause_are_excluded() {
        let mut s=Sample::new();for _ in 0..60 {s.push(16_667,true);}
        assert!(s.frames.is_empty());s.push(16_667,true);assert_eq!(s.frames.len(),1);
        s.push(2_000_000,false);assert_eq!(s.frames.len(),1);
        for _ in 0..60 {s.push(16_667,true);}assert_eq!(s.frames.len(),1);
    }
    #[test] fn long_live_hitches_are_included() {
        let mut s=Sample::new();for _ in 0..60{s.push(16_667,true);}
        s.push(750_000,true);assert_eq!(s.valid_us,750_000);assert_eq!(s.frames,vec![750_000]);
    }
    #[test] fn explicit_pause_excludes_resume_gap_but_next_fire_hitch_is_retained() {
        let mut s=Sample::new();for _ in 0..60{s.push(16_667,true);}
        s.push(16_667,true);s.interrupt();s.push(10_000_000,true);
        for _ in 0..59{s.push(16_667,true);}
        assert_eq!(s.frames,vec![16_667]);
        s.push(750_000,live_state(3,200,200));assert_eq!(s.frames,vec![16_667,750_000]);
    }
    #[test] fn thirty_active_seconds_uses_actual_intervals() {
        let mut s=Sample::new();for _ in 0..60{s.push(20_000,true);}
        let mut result=None;for _ in 0..1500{result=s.push(20_000,true);}
        let r=result.unwrap();assert_eq!(r.frames,1500);assert_eq!(r.fps,50.0);assert_eq!(r.p99_ms,20.0);assert_eq!(r.over50,0);
    }
}

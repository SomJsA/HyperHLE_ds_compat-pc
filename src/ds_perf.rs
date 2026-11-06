//! Opt-in CPU call timings. No extra GL query, finish, readback or trace dump.
use std::sync::{atomic::{AtomicU64, Ordering}, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Copy, Clone)]
pub(crate) enum Stage {
    Readback,
    Overlay,
    Composition,
    FastPresent,
    ShaderCompile,
    ShaderLink,
    TextureSync,
}
impl Stage {
    fn index(self) -> usize { self as usize }
    fn name(self) -> &'static str {
        match self {
            Self::Readback=>"readback",
            Self::Overlay=>"cpu_overlay",
            Self::Composition=>"ca_composition",
            Self::FastPresent=>"gpu_present",
            Self::ShaderCompile=>"shader_compile_call",
            Self::ShaderLink=>"shader_link_call",
            Self::TextureSync=>"texture_sync",
        }
    }
}
pub(crate) const STAGE_COUNT:usize=Stage::TextureSync as usize+1;
fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| ["TOUCHHLE_DS_PERF_TRACE", "TOUCHHLE_DS_SUPER_MONITOR"]
        .iter().any(|key| std::env::var(key).as_deref() == Ok("1")))
}

// Independent of the sync timer: Off returns before that timer exists.
// Rows are Off/Flush/Finish; columns are payload/non-payload submissions.
// These count guest calls reaching the sync hook, not GPU completion, bytes,
// or HD/mip/helper paths which bypass this hook. No new GL query or wait.
struct TextureSubmissions([[AtomicU64; 2]; 3]);
impl Default for TextureSubmissions {
    fn default() -> Self { Self(std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(0)))) }
}
impl TextureSubmissions {
    fn record(&self, mode: crate::ds_perf_control::TextureSyncMode, payload: bool) {
        use crate::ds_perf_control::TextureSyncMode;
        let row = match mode { TextureSyncMode::Off => 0, TextureSyncMode::Flush => 1, TextureSyncMode::Finish => 2 };
        self.0[row][usize::from(!payload)].fetch_add(1, Ordering::Relaxed);
    }
    fn snapshot(&self) -> [[u64; 2]; 3] {
        std::array::from_fn(|row| std::array::from_fn(|column| self.0[row][column].load(Ordering::Relaxed)))
    }
}
static TEXTURE_SUBMISSIONS: OnceLock<TextureSubmissions> = OnceLock::new();
// CPU/GPU submission counts; independent of requested performance switches.
struct PairCounts([AtomicU64; 2]);
impl Default for PairCounts {
    fn default() -> Self { Self(std::array::from_fn(|_| AtomicU64::new(0))) }
}
impl PairCounts {
    fn record(&self, gpu: bool) { self.0[usize::from(gpu)].fetch_add(1, Ordering::Relaxed); }
    fn snapshot(&self) -> [u64; 2] { std::array::from_fn(|i| self.0[i].load(Ordering::Relaxed)) }
}
static TEXT_CARDS: OnceLock<PairCounts> = OnceLock::new();
static PRESENT_TRANSFERS: OnceLock<PairCounts> = OnceLock::new();
pub(crate) fn text_card_submission(gpu: bool) {
    if enabled() { TEXT_CARDS.get_or_init(PairCounts::default).record(gpu); }
}
pub(crate) fn present_transfer_submission(gpu: bool) {
    if enabled() { PRESENT_TRANSFERS.get_or_init(PairCounts::default).record(gpu); }
}
pub(crate) fn text_card_submissions() -> [u64; 2] {
    TEXT_CARDS.get().map(PairCounts::snapshot).unwrap_or([0; 2])
}
pub(crate) fn present_transfer_submissions() -> [u64; 2] {
    PRESENT_TRANSFERS.get().map(PairCounts::snapshot).unwrap_or([0; 2])
}
pub(crate) fn texture_submission(mode: crate::ds_perf_control::TextureSyncMode, payload: bool) {
    if enabled() { TEXTURE_SUBMISSIONS.get_or_init(TextureSubmissions::default).record(mode, payload); }
}
pub(crate) fn texture_submissions() -> [[u64; 2]; 3] {
    TEXTURE_SUBMISSIONS.get().map(TextureSubmissions::snapshot).unwrap_or([[0; 2]; 3])
}
static LATEST:OnceLock<Mutex<[(f64,f64,u64);STAGE_COUNT]>>=OnceLock::new();
pub(crate) fn snapshot() -> [(f64,f64,u64);STAGE_COUNT] {
    LATEST.get_or_init(||Mutex::new([(0.0,0.0,0);STAGE_COUNT])).lock().map(|s|*s).unwrap_or([(0.0,0.0,0);STAGE_COUNT])
}
struct Window { samples:u64, nanos:u128, max_nanos:u128, reported:Instant }
pub(crate) struct Timer { stage:Stage, started:Option<Instant> }
impl Timer {
    pub fn new(stage:Stage) -> Self {
        Self { stage, started:enabled().then(Instant::now) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ds_perf_control::TextureSyncMode::{Off, Flush, Finish};

    #[test]
    fn off_uploads_remain_observable_without_a_sync_timer() {
        let counters = TextureSubmissions::default();
        for mode in [Off, Flush, Finish, Off, Finish, Off] { counters.record(mode, true); }
        assert_eq!(counters.snapshot(), [[3, 0], [1, 0], [2, 0]]);
    }
    #[test]
    fn presentation_counters_distinguish_actual_cpu_and_gpu_work() {
        let counts = PairCounts::default();
        counts.record(true); counts.record(false); counts.record(true);
        assert_eq!(counts.snapshot(), [1, 2]);
    }
    #[test]
    fn allocations_and_empty_submissions_do_not_claim_payload_activity() {
        let counters = TextureSubmissions::default();
        counters.record(Off, false); counters.record(Finish, false);
        assert_eq!(counters.snapshot(), [[0, 1], [0, 0], [0, 1]]);
    }
    #[test]
    fn concurrent_submission_totals_do_not_lose_updates() {
        let counters = TextureSubmissions::default();
        std::thread::scope(|scope| {
            for _ in 0..4 { let counters = &counters; scope.spawn(move || {
                for _ in 0..1000 { counters.record(Off, true); }
            }); }
        });
        assert_eq!(counters.snapshot(), [[4000, 0], [0, 0], [0, 0]]);
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        let Some(started)=self.started else{return};
        static STATS:OnceLock<[Mutex<Window>;STAGE_COUNT]>=OnceLock::new();
        let stats=STATS.get_or_init(||std::array::from_fn(|_|Mutex::new(Window{samples:0,nanos:0,max_nanos:0,reported:Instant::now()})));
        let Ok(mut window)=stats[self.stage.index()].lock() else{return};
        let nanos=started.elapsed().as_nanos();
        window.samples+=1;window.nanos+=nanos;window.max_nanos=window.max_nanos.max(nanos);
        if let Ok(mut latest)=LATEST.get_or_init(||Mutex::new([(0.0,0.0,0);STAGE_COUNT])).lock() {
            let total=latest[self.stage.index()].2+1;
            latest[self.stage.index()]=(window.nanos as f64/window.samples as f64/1e6,window.max_nanos as f64/1e6,total);
        }
        if window.samples==1 || window.reported.elapsed()>=Duration::from_secs(2) {
            echo!("[DSperf] stage={} samples={} mean_ms={:.3} max_ms={:.3}",self.stage.name(),window.samples,window.nanos as f64/window.samples as f64/1e6,window.max_nanos as f64/1e6);
            window.reported=Instant::now();
            // Keep a representative recent window after the startup sample.
            if window.samples>1 {window.samples=0;window.nanos=0;window.max_nanos=0;}
        }
    }
}

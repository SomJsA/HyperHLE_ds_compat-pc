//! Opt-in main-thread elapsed spans. Nested scopes are not exclusive CPU time.
//! No GL calls, guest reads, pixel capture, input or worker ownership changes.
use std::cell::RefCell;
use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    DrawElements = 0,
    DrawArrays = 1,
    IndexRange = 2,
    Skin = 3,
    SkinSubmit = 4,
}
const KINDS: usize = 5;
const MAX_LAYOUTS: usize = 16;
static FRAME: AtomicU64 = AtomicU64::new(0);
static THREAD: AtomicU64 = AtomicU64::new(1);
fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var("TOUCHHLE_DS_GL_PROFILE").as_deref() == Ok("1");
        if on {
            let _ = writeln!(std::io::stderr().lock(), "[DSgl-profile-meta] schema=1 units=ns scopes=DrawElements,DrawArrays,IndexRange,Skin,SkinSubmit; nested elapsed wall spans, not exclusive CPU or GPU time; skin math includes output allocation; layout fields=vsize,vtype,vstride,wsize,wtype,wstride,isize,itype,istride,buffers_mask,units; owner_thread_tag is local, not OS TID; frame is last observed native-present serial");
        }
        on
    })
}
pub(crate) fn present(frame: u64) {
    if enabled() {
        FRAME.store(frame, Relaxed);
    }
}

#[derive(Clone, Copy, Default)]
struct Counts {
    calls: u64,
    total_ns: u64,
    max_ns: u64,
    query_ns: u64,
    math_ns: u64,
    post_ns: u64,
    vertices: u64,
    output_values: u64,
    completed_math: u64,
}
#[derive(Clone, Copy)]
struct Sample {
    kind: Kind,
    total_ns: u64,
    phases: Option<[u64; 3]>,
    vertices: u64,
    output_values: u64,
    layout: Option<[u32; 11]>,
}
struct State {
    started: Instant,
    last: Instant,
    owner: u64,
    lo: Option<u64>,
    counts: [Counts; KINDS],
    layouts: Vec<([u32; 11], u64, u64)>,
    other_layout_calls: u64,
}
impl Default for State {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            last: now,
            owner: THREAD.fetch_add(1, Relaxed),
            lo: None,
            counts: [Counts::default(); KINDS],
            layouts: Vec::with_capacity(MAX_LAYOUTS),
            other_layout_calls: 0,
        }
    }
}
impl State {
    fn record(&mut self, sample: Sample, frame: u64) {
        self.lo.get_or_insert(frame);
        let c = &mut self.counts[sample.kind as usize];
        c.calls += 1;
        c.total_ns = c.total_ns.saturating_add(sample.total_ns);
        c.max_ns = c.max_ns.max(sample.total_ns);
        c.vertices = c.vertices.saturating_add(sample.vertices);
        c.output_values = c.output_values.saturating_add(sample.output_values);
        if let Some([query, math, post]) = sample.phases {
            c.query_ns = c.query_ns.saturating_add(query);
            c.math_ns = c.math_ns.saturating_add(math);
            c.post_ns = c.post_ns.saturating_add(post);
            c.completed_math += 1;
        } else if sample.kind == Kind::Skin {
            c.query_ns = c.query_ns.saturating_add(sample.total_ns);
        }
        if let Some(layout) = sample.layout {
            if let Some((_, calls, vertices)) = self.layouts.iter_mut().find(|v| v.0 == layout) {
                *calls += 1;
                *vertices = vertices.saturating_add(sample.vertices);
            } else if self.layouts.len() < MAX_LAYOUTS {
                self.layouts.push((layout, 1, sample.vertices));
            } else {
                self.other_layout_calls += 1;
            }
        }
    }
    fn report(&self, frame: u64) -> String {
        let mut s = String::with_capacity(4096);
        let wall_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = write!(s, "{{\"schema\":1,\"owner_thread_tag\":{},\"wall_unix_ms\":{},\"profile_mono_us\":{},\"elapsed_ns\":{},\"frame_lo\":{},\"frame_hi\":{},\"kinds\":[", self.owner, wall_ms, self.started.elapsed().as_micros(), self.last.elapsed().as_nanos(), self.lo.unwrap_or(frame), frame);
        for (i, c) in self.counts.iter().enumerate() {
            if i != 0 {
                s.push(',');
            }
            let _ = write!(s, "{{\"kind\":{},\"calls\":{},\"total_ns\":{},\"max_ns\":{},\"query_ns\":{},\"math_ns\":{},\"post_ns\":{},\"vertices\":{},\"output_values\":{},\"completed_math\":{}}}", i, c.calls, c.total_ns, c.max_ns, c.query_ns, c.math_ns, c.post_ns, c.vertices, c.output_values, c.completed_math);
        }
        s.push_str("],\"layouts\":[");
        for (i, (layout, calls, vertices)) in self.layouts.iter().enumerate() {
            if i != 0 {
                s.push(',');
            }
            let _ = write!(
                s,
                "{{\"signature\":{:?},\"calls\":{},\"vertices\":{}}}",
                layout, calls, vertices
            );
        }
        let _ = write!(s, "],\"other_layout_calls\":{}}}", self.other_layout_calls);
        s
    }
    fn clear(&mut self) {
        self.last = Instant::now();
        self.lo = None;
        self.counts = [Counts::default(); KINDS];
        self.layouts.clear();
        self.other_layout_calls = 0;
    }
}
pub(crate) struct Timer {
    kind: Kind,
    start: Instant,
    frame: u64,
    math_start: Option<Instant>,
    math_end: Option<Instant>,
    vertices: u64,
    output_values: u64,
    layout: Option<[u32; 11]>,
}
pub(crate) fn begin(kind: Kind) -> Option<Timer> {
    if !enabled() {
        return None;
    }
    Some(Timer {
        kind,
        start: Instant::now(),
        frame: FRAME.load(Relaxed),
        math_start: None,
        math_end: None,
        vertices: 0,
        output_values: 0,
        layout: None,
    })
}
impl Timer {
    pub(crate) fn skin_layout(&mut self, layout: [u32; 11], vertices: usize, output_values: usize) {
        self.layout = Some(layout);
        self.vertices = vertices as u64;
        self.output_values = output_values as u64;
    }
    pub(crate) fn math_begin(&mut self) {
        self.math_start = Some(Instant::now());
    }
    pub(crate) fn math_end(&mut self) {
        self.math_end = Some(Instant::now());
    }
}
fn ns(d: Duration) -> u64 {
    d.as_nanos().min(u64::MAX as u128) as u64
}
impl Drop for Timer {
    fn drop(&mut self) {
        let end = Instant::now();
        let phases = self.math_start.zip(self.math_end).map(|(a, b)| {
            [
                ns(a.duration_since(self.start)),
                ns(b.duration_since(a)),
                ns(end.duration_since(b)),
            ]
        });
        let sample = Sample {
            kind: self.kind,
            total_ns: ns(end.duration_since(self.start)),
            phases,
            vertices: self.vertices,
            output_values: self.output_values,
            layout: self.layout,
        };
        thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
        STATE.with(|state| {
            if let Ok(mut state) = state.try_borrow_mut() {
                state.record(sample, self.frame);
                if state.last.elapsed() >= Duration::from_secs(1) {
                    let line = state.report(FRAME.load(Relaxed));
                    let _ = writeln!(std::io::stderr().lock(), "[DSgl-profile] {}", line);
                    state.clear();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiler_counts_nested_kinds_and_preserves_complete_and_aborted_skin_phases() {
        let mut s = State::default();
        let sample = Sample {
            kind: Kind::Skin,
            total_ns: 100,
            phases: Some([20, 70, 10]),
            vertices: 3,
            output_values: 12,
            layout: Some([3; 11]),
        };
        s.record(sample, 10);
        s.record(
            Sample {
                phases: None,
                total_ns: 7,
                ..sample
            },
            11,
        );
        s.record(
            Sample {
                kind: Kind::DrawElements,
                total_ns: 200,
                phases: None,
                layout: None,
                ..sample
            },
            11,
        );
        let c = s.counts[Kind::Skin as usize];
        assert_eq!(
            (
                c.calls,
                c.total_ns,
                c.query_ns,
                c.math_ns,
                c.post_ns,
                c.completed_math
            ),
            (2, 107, 27, 70, 10, 1)
        );
        assert_eq!(c.query_ns + c.math_ns + c.post_ns, c.total_ns);
        assert_eq!(s.counts[0].total_ns, 200);
        assert_eq!(s.layouts, vec![([3; 11], 2, 6)]);
        assert_eq!(s.lo, Some(10));
        let report = s.report(11);
        assert!(report.contains("\"frame_lo\":10,\"frame_hi\":11"));
        println!("[TEST_PROFILE_JSON] {}", report);
        s.clear();
        assert!(s.lo.is_none() && s.layouts.is_empty());
        assert_eq!(s.counts[3].calls, 0);
    }
    #[test]
    fn profiler_layouts_are_bounded_without_losing_total_calls_or_vertices() {
        let mut s = State::default();
        for i in 0..100u32 {
            s.record(
                Sample {
                    kind: Kind::Skin,
                    total_ns: 10,
                    phases: None,
                    vertices: 2,
                    output_values: 6,
                    layout: Some([i; 11]),
                },
                i as u64,
            );
        }
        assert_eq!(s.layouts.len(), MAX_LAYOUTS);
        assert_eq!(s.other_layout_calls, 84);
        assert_eq!(s.counts[3].calls, 100);
        assert_eq!(s.counts[3].vertices, 200);
        assert!(s.report(100).len() < 8192);
    }
}

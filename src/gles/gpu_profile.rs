//! Context-owned, opt-in asynchronous GPU command timestamps.
//! Samples every 30th native present serial. No pixels, fences, glFinish or
//! blocking result fetch. Nested presentation/draw spans are not additive.
use super::super::gl21compat_raw as gl;
use std::collections::VecDeque;
use std::ffi::CStr;
use std::sync::atomic::{AtomicU64,Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const CPU_STAGES: [&str; 10] = ["draw", "msaa_resolve", "present_copy", "present_quad",
    "before_copy", "prepare_present", "restore_before_swap", "swap_and_current", "restore_after_swap", "clear"];
static NEXT_OWNER:AtomicU64=AtomicU64::new(1);
fn gpu_stage(stage:usize)->bool {matches!(stage,0..=3|9)}
const CAPACITY: usize = 256;
struct Span { queries: [u32; 2], stage: usize, frame: u64 }
#[derive(Clone, Copy, Default)]
struct Stats { calls: u64, ns: u64, maximum: u64 }
#[derive(Clone, Copy, Default)]
struct CpuStats { calls:u64, wall_ns:u64, cycles:u64, cycle_calls:u64 }

#[cfg(target_os="windows")]
fn thread_cycles() -> Option<u64> {
    #[link(name="kernel32")]
    extern "system" {
        fn GetCurrentThread() -> *mut std::ffi::c_void;
        fn QueryThreadCycleTime(thread:*mut std::ffi::c_void, cycles:*mut u64) -> i32;
    }
    let mut cycles=0;
    unsafe {(QueryThreadCycleTime(GetCurrentThread(),&mut cycles)!=0).then_some(cycles)}
}
#[cfg(not(target_os="windows"))]
fn thread_cycles() -> Option<u64> {None}

pub(super) struct State {
    owner:u64,
    enabled: bool,
    checked: bool,
    supported: bool,
    active: [Option<Span>; 10],
    pending: VecDeque<Span>,
    free: Vec<[u32; 2]>,
    created: usize,
    dropped: u64,
    totals: [Stats; 10],
    cpu_active: [Option<(Instant,Option<u64>)>;10],
    cpu_totals: [CpuStats;10],
    since: Option<Instant>,
}

#[cfg(test)]
#[path = "gpu_profile_tests.rs"]
mod tests;

impl Default for State {
    fn default() -> Self {
        Self { owner:NEXT_OWNER.fetch_add(1,Ordering::Relaxed),enabled: cfg!(target_os="windows") && std::env::var("TOUCHHLE_DS_GPU_PROFILE").as_deref()==Ok("1"),
            checked:false, supported:false, active:std::array::from_fn(|_|None),
            pending:VecDeque::new(), free:Vec::new(), created:0, dropped:0,
            totals:[Stats::default();10], cpu_active:std::array::from_fn(|_|None),
            cpu_totals:[CpuStats::default();10], since:None }
    }
}

impl State {
    unsafe fn supported(&mut self) -> bool {
        if !self.checked {
            self.checked=true;
            let extension=gl::GetString(gl::EXTENSIONS);
            self.supported=!extension.is_null()
                && CStr::from_ptr(extension.cast()).to_string_lossy().split_whitespace().any(|v|v=="GL_ARB_timer_query")
                && gl::QueryCounter::is_loaded() && gl::GetQueryObjectui64v::is_loaded();
            let mut bits=0;
            if self.supported {gl::GetQueryiv(gl::TIMESTAMP,gl::QUERY_COUNTER_BITS,&mut bits);self.supported=bits>=32;}
            self.since=Some(Instant::now());
            eprintln!("[DSgpu-profile] owner={} supported={} timestamp_bits={} sample_every=30 capacity={} semantics=GPU-command-timestamps-not-exclusive-utilization",self.owner,self.supported,bits,CAPACITY);
        }
        self.supported
    }

    pub(super) unsafe fn begin(&mut self, stage: usize) {
        if !self.enabled || stage>=CPU_STAGES.len() {return;}
        let frame=crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed);
        // Do not count final-present draws twice in the draw total.
        if frame%30!=0 || (matches!(stage,0|9) && self.active[3].is_some()) || self.cpu_active[stage].is_some() {return;}
        self.cpu_active[stage]=Some((Instant::now(),thread_cycles()));
        if !gpu_stage(stage) || self.active[stage].is_some() || !self.supported() {return;}
        let queries=if let Some(pair)=self.free.pop() {pair} else if self.created<CAPACITY {
            let mut pair=[0;2]; gl::GenQueries(2,pair.as_mut_ptr());
            self.created+=1; pair
        } else {self.dropped+=1;return;};
        gl::QueryCounter(queries[0],gl::TIMESTAMP);
        self.active[stage]=Some(Span{queries,stage,frame});
    }

    pub(super) unsafe fn end(&mut self, stage: usize) {
        if !self.enabled || stage>=CPU_STAGES.len() {return;}
        if let Some((start,cycles))=self.cpu_active[stage].take() {
            let stats=&mut self.cpu_totals[stage];stats.calls+=1;
            stats.wall_ns=stats.wall_ns.saturating_add(start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
            if let Some(delta)=cycles.zip(thread_cycles()).and_then(|(a,b)|b.checked_sub(a)) {
                stats.cycles=stats.cycles.saturating_add(delta);stats.cycle_calls+=1;
            }
        }
        if !gpu_stage(stage) {return;}
        if let Some(span)=self.active[stage].take() {
            gl::QueryCounter(span.queries[1],gl::TIMESTAMP);
            self.pending.push_back(span);
        }
    }

    pub(super) unsafe fn poll(&mut self) {
        if !self.enabled || !self.supported() {return;}
        let frame=crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed);
        while let Some(span)=self.pending.front() {
            if frame.saturating_sub(span.frame)<3 {break;}
            let mut available=0;
            gl::GetQueryObjectiv(span.queries[1],gl::QUERY_RESULT_AVAILABLE,&mut available);
            if available==0 {break;}
            let span=self.pending.pop_front().unwrap();
            let (mut start,mut end)=(0u64,0u64);
            gl::GetQueryObjectui64v(span.queries[0],gl::QUERY_RESULT,&mut start);
            gl::GetQueryObjectui64v(span.queries[1],gl::QUERY_RESULT,&mut end);
            if let Some(ns)=end.checked_sub(start) {
                let stats=&mut self.totals[span.stage];
                stats.calls+=1;stats.ns=stats.ns.saturating_add(ns);stats.maximum=stats.maximum.max(ns);
            } else {self.dropped+=1;}
            self.free.push(span.queries);
        }
        if self.since.is_some_and(|t|t.elapsed().as_secs_f64()>=1.0) {
            let stages:Vec<_>=self.cpu_totals.iter().enumerate().map(|(i,c)|{
                let s=self.totals.get(i).copied().unwrap_or_default();serde_json::json!({
                "stage":CPU_STAGES[i],"calls":s.calls,"gpu_ns":s.ns,"max_gpu_ns":s.maximum,
                "cpu_calls":c.calls,"cpu_wall_ns":c.wall_ns,"cpu_cycles":c.cycles,"cpu_cycle_calls":c.cycle_calls
            })}).collect();
            eprintln!("[DSgpu-profile] {}",serde_json::json!({"unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
                "owner":self.owner,"frame":frame,"stages":stages,"pending":self.pending.len(),"created_pairs":self.created,"dropped":self.dropped}));
            self.totals=[Stats::default();10];self.cpu_totals=[CpuStats::default();10];self.since=Some(Instant::now());
        }
    }

    pub(super) unsafe fn release(&mut self) {
        if !self.enabled || !self.supported {return;}
        let unresolved=self.pending.len()+self.active.iter().filter(|s|s.is_some()).count();
        for pair in self.free.drain(..) {gl::DeleteQueries(2,pair.as_ptr());}
        for span in self.pending.drain(..) {gl::DeleteQueries(2,span.queries.as_ptr());}
        for span in self.active.iter_mut().filter_map(Option::take) {gl::DeleteQueries(2,span.queries.as_ptr());}
        self.created=0;
        eprintln!("[DSgpu-profile] owner={} released_without_wait unresolved_pairs={unresolved}",self.owner);
    }
}

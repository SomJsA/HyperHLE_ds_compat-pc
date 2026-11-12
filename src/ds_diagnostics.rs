//! Bounded asynchronous play diagnostics. Explicit F7 may request one PNG; no ordinary input trace/guest
//! mutation, compression or synchronous filesystem IO in the present path.
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{mpsc::{self, SyncSender, TrySendError}, Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[path="ds_problem_marks.rs"] mod problem_marks;
#[path="ds_diagnostic_delivery.rs"] mod delivery;
use delivery::{Delivery, Permit};
pub(crate) fn mark_problem(){problem_marks::request();}
pub(crate) fn problem_notice()->Option<String>{problem_marks::text()}
pub(crate) unsafe fn capture_problem_frame(gles:&mut dyn crate::gles::GLES,viewport:(u32,u32,u32,u32)){problem_marks::capture(gles,viewport)}
const QUEUE: usize = 8192;
const FRAME_RING: usize = 65536;
const EVENT_RING: usize = 2048;
const RETAIN_US: u64 = 45_000_000;
const BEFORE_US: u64 = 30_000_000;
const AFTER_US: u64 = 3_000_000;
const PHASES: usize = 11;
const BINS: usize = 1002; // 0.5ms bins through 500ms, plus overflow.
const PHASE_NAMES: [&str; PHASES] = ["unknown", "menu", "loading", "loading_ready", "gameplay_hud_clear", "cutscene", "native_hud_menu", "dead", "station_or_rig_transition", "paused", "world_unclassified"];
static ACTIVE: OnceLock<Option<Client>> = OnceLock::new();
static MANUAL: OnceLock<Option<Client>> = OnceLock::new();
static RUNTIME_VERSION: OnceLock<String> = OnceLock::new();
static FATAL: AtomicBool = AtomicBool::new(false);
static BAD_END: AtomicBool = AtomicBool::new(false);
static LEVEL: AtomicU32 = AtomicU32::new(0);
static LOST: AtomicU64 = AtomicU64::new(0);
static SKIPPED: AtomicU64 = AtomicU64::new(0);
static COLLECT_NS: AtomicU64 = AtomicU64::new(0);
static CORE_THREAD:AtomicU64=AtomicU64::new(0);
static SECONDARY_COLLECT_NS:AtomicU64=AtomicU64::new(0);
static SECONDARY_COLLECT_MAX_NS:AtomicU64=AtomicU64::new(0);
static COLLECT_MAX_NS: AtomicU64 = AtomicU64::new(0);
static STATE: AtomicU32 = AtomicU32::new(u32::MAX);
static HUD: AtomicU32 = AtomicU32::new(u32::MAX);
static PLAYER_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static MENU_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static LOAD_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static LOAD_READY: AtomicBool = AtomicBool::new(false);
static COUNTERS: OnceLock<Vec<Counter>> = OnceLock::new();
struct Client { tx: SyncSender<Message>, start: Instant, delivery: Arc<Delivery> }
#[derive(Clone, Copy, Debug)]
struct Frame { t: u64, n: u64, dt: u64, phase: usize, state: u32, hud: u32 }
enum Message { Frame(Frame), Event(u64, &'static str, [u64; 4],u64), Feature(u64, &'static str, String,u64), Problem(problem_marks::Problem), Picture(problem_marks::Picture) }
fn thread_tag()->u64 {
    static NEXT:AtomicU64=AtomicU64::new(1);
    thread_local! {static TAG:std::cell::Cell<u64>=const{std::cell::Cell::new(0)};}
    TAG.with(|tag|{let value=tag.get();if value!=0{value}else{let value=NEXT.fetch_add(1,Relaxed);tag.set(value);value}})
}
struct Counter { n: AtomicU64, sum: AtomicU64, max: AtomicU64, over50: AtomicU64, bins: Vec<AtomicU64> }
impl Counter {
    fn new() -> Self { Self { n: AtomicU64::new(0), sum: AtomicU64::new(0), max: AtomicU64::new(0), over50: AtomicU64::new(0), bins: (0..BINS).map(|_|AtomicU64::new(0)).collect() } }
    fn push(&self, dt:u64) {
        self.n.fetch_add(1,Relaxed);self.sum.fetch_add(dt,Relaxed);self.max.fetch_max(dt,Relaxed);
        if dt>50_000 {self.over50.fetch_add(1,Relaxed);}
        self.bins[((dt/500) as usize).min(BINS-1)].fetch_add(1,Relaxed);
    }
    fn take(&self) -> Value {
        let n=self.n.swap(0,Relaxed);let sum=self.sum.swap(0,Relaxed);let max=self.max.swap(0,Relaxed);
        let hist:Vec<u64>=self.bins.iter().map(|v|v.swap(0,Relaxed)).collect();let count:u64=hist.iter().sum();
        let percentile=|p:u64| {let target=(count*p+99)/100;let mut seen=0;for (i,v) in hist.iter().enumerate(){seen+=v;if seen>=target && target>0{return if i==BINS-1{max as f64/1000.0}else{(i+1) as f64*0.5};}}0.0};
        json!({"frames":n,"valid_interval_seconds":sum as f64/1e6,"fps":if sum>0{Some(n as f64*1e6/sum as f64)}else{None},
            "mean_ms":if n>0{Some(sum as f64/n as f64/1000.0)}else{None},"p95_ms_upper_estimate":percentile(95),"p99_ms_upper_estimate":percentile(99),
            "max_ms":max as f64/1000.0,"over50ms":self.over50.swap(0,Relaxed),"histogram_frames":count})
    }
}
fn requested()->bool { static ON:OnceLock<bool>=OnceLock::new();*ON.get_or_init(||std::env::var("TOUCHHLE_DS_DIAGNOSTICS").as_deref()==Ok("1")) }
pub(crate) fn enabled()->bool { requested() && ACTIVE.get().and_then(Option::as_ref).is_some_and(|c|!c.delivery.closed() && !c.delivery.ended.load(std::sync::atomic::Ordering::Acquire)) }
fn now(c:&Client)->u64 { c.start.elapsed().as_micros().min(u64::MAX as u128) as u64 }
fn send(c:&Client,m:Message)->bool {
    let Some(permit)=c.delivery.enter() else{return false;};
    send_admitted(c,m,permit)
}
fn send_admitted(c:&Client,m:Message,_permit:Permit<'_>)->bool {
    c.delivery.queued.fetch_add(1,Relaxed);
    if let Err(error)=c.tx.try_send(m) {
        c.delivery.queued.fetch_sub(1,Relaxed);LOST.fetch_add(1,Relaxed);
        if matches!(error,TrySendError::Disconnected(_)){LEVEL.store(3,Relaxed);}
        false
    }else{
        true
    }
}
pub(crate) fn detail_enabled()->bool {enabled() && LEVEL.load(Relaxed)<2}
pub(crate) fn collector_cost(started:Instant){record_cost(started);}
fn record_cost(started:Instant){
    let cost=started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    if CORE_THREAD.load(Relaxed)!=0 && thread_tag()!=CORE_THREAD.load(Relaxed){SECONDARY_COLLECT_NS.fetch_add(cost,Relaxed);SECONDARY_COLLECT_MAX_NS.fetch_max(cost,Relaxed);}
    else{COLLECT_NS.fetch_add(cost,Relaxed);COLLECT_MAX_NS.fetch_max(cost,Relaxed);}
}
pub(crate) fn event(code:&'static str,values:[u64;4]) {if let Some(Some(c))=ACTIVE.get(){let begin=Instant::now();send(c,Message::Event(now(c),code,values,thread_tag()));record_cost(begin);}}
pub(crate) fn feature(code:&'static str,text:&str) {if let Some(Some(c))=ACTIVE.get(){let begin=Instant::now();send(c,Message::Feature(now(c),code,text.chars().take(512).collect(),thread_tag()));record_cost(begin);}}
pub(crate) fn player(frame:u64,hud:u32,state:u32) {
    if !enabled(){return;}let begin=Instant::now();HUD.store(hud,Relaxed);STATE.store(state,Relaxed);PLAYER_FRAME.store(frame,Relaxed);record_cost(begin);
}
pub(crate) fn menu(frame:u64,page:u32) {if enabled(){MENU_FRAME.store(frame,Relaxed);static LAST:AtomicU32=AtomicU32::new(u32::MAX);if LAST.swap(page,Relaxed)!=page{event("native_menu_page",[page as u64,0,0,0]);}}}
pub(crate) fn loading(frame:u64,ready:bool) {if enabled(){LOAD_FRAME.store(frame,Relaxed);LOAD_READY.store(ready,Relaxed);}}
fn fresh(frame:u64,stamp:u64)->bool {stamp<=frame && frame-stamp<=2}
fn classify(frame:u64)->usize {
    if fresh(frame,LOAD_FRAME.load(Relaxed)){return if LOAD_READY.load(Relaxed){3}else{2};}
    if fresh(frame,MENU_FRAME.load(Relaxed)){return 1;}
    if !fresh(frame,PLAYER_FRAME.load(Relaxed)){return 0;}
    phase(HUD.load(Relaxed),STATE.load(Relaxed))
}
fn phase(hud:u32,state:u32)->usize {
    if state==26{return 7;}if state==29{return 5;}if (19..=24).contains(&state){return 8;}
    match hud {2|3=>9,4|5=>6,0 if state<=18=>4,0=>10,_=>0}
}
pub(crate) fn present(n:u64) {
    crate::ds_gl_profile::present(n);
    let Some(Some(c))=ACTIVE.get() else{return;};
    if c.delivery.closed(){return;}
    let started=Instant::now();let t=now(c);
    thread_local! {static LAST:std::cell::Cell<u64>=const{std::cell::Cell::new(0)};}
    let last=LAST.with(|last|last.replace(t));if last==0{event("native_present_thread",[thread_tag(),n,0,0]);return;}
    let p=classify(n);let dt=t.saturating_sub(last);COUNTERS.get().unwrap()[p].push(dt);
    let current=fresh(n,PLAYER_FRAME.load(Relaxed));
    let state=if current{STATE.load(Relaxed)}else{u32::MAX};let hud=if current{HUD.load(Relaxed)}else{u32::MAX};
    thread_local! {static LAST_STATE:std::cell::Cell<(usize,u32,u32)>=const{std::cell::Cell::new((usize::MAX,u32::MAX,u32::MAX))};}
    if LAST_STATE.with(|last|last.replace((p,state,hud)))!=(p,state,hud){send(c,Message::Event(t,"native_state",[p as u64,state as u64,hud as u64,n],thread_tag()));}
    let level=LEVEL.load(Relaxed);
    if level==0 || (level==1 && n%4==0) {
        send(c,Message::Frame(Frame{t,n,dt,phase:p,state,hud}));
    }else{SKIPPED.fetch_add(1,Relaxed);}
    record_cost(started);
}
pub(crate) fn gl_error(code:u32){
    let Some(Some(c))=ACTIVE.get() else{return;};static LAST:AtomicU64=AtomicU64::new(0);let t=now(c);
    if t.saturating_sub(LAST.load(Relaxed))>=500_000{LAST.store(t,Relaxed);event("gl_error_observed",[code as u64,0,0,0]);}
}
pub(crate) fn fault() {if enabled(){BAD_END.store(true,Relaxed);FATAL.store(true,Relaxed);event("native_fatal",[0;4]);}}
pub(crate) fn finish(normal:bool) {
    if !normal{BAD_END.store(true,Relaxed);fault();}
    for c in [ACTIVE.get().and_then(Option::as_ref), MANUAL.get().and_then(Option::as_ref)].into_iter().flatten() {
        c.delivery.close();
        // Teardown only, never a game-frame wait. A failed/slow disk cannot hang exit.
        wait_for_writer(&c.delivery);
    }
}
fn wait_for_writer(delivery:&Delivery)->bool {
    let limit=Instant::now()+Duration::from_millis(200);
    while !delivery.ended.load(std::sync::atomic::Ordering::Acquire) && Instant::now()<limit {std::thread::sleep(Duration::from_millis(2));}
    delivery.ended.load(std::sync::atomic::Ordering::Acquire)
}
pub(crate) fn init(version:&str) {
    RUNTIME_VERSION.get_or_init(||version.chars().take(160).collect());
    if !requested(){return;}
    ACTIVE.get_or_init(||{
        CORE_THREAD.store(thread_tag(),Relaxed);
        let root=std::env::var_os("TOUCHHLE_DS_DIAGNOSTICS_DIR").map(PathBuf::from)?;
        let start=Instant::now();let wall=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
        let (tx,rx)=mpsc::sync_channel(QUEUE);COUNTERS.get_or_init(||(0..PHASES).map(|_|Counter::new()).collect());
        let delivery=Arc::new(Delivery::default());let writer_delivery=delivery.clone();
        let version=version.chars().take(160).collect::<String>();
        if std::thread::Builder::new().name("ds-diagnostics".into()).spawn(move||{
            if let Err(error)=worker(root,rx,start,wall,version,&writer_delivery) {
                LEVEL.store(3,Relaxed);problem_marks::backend_failed();
                eprintln!("[DSdiagnostics] backend disabled: {:?}; original records retained",error.kind());
            }writer_delivery.ended.store(true,std::sync::atomic::Ordering::Release);
        }).is_err(){return None;}
        Some(Client{tx,start,delivery})
    });
}

// Starting an F7 writer must not activate ordinary collectors using ACTIVE.
fn problem_client() -> Option<&'static Client> {
    if requested() {
        return ACTIVE.get().and_then(Option::as_ref).filter(|_| enabled());
    }
    MANUAL.get_or_init(|| {
        let base=PathBuf::from(std::env::var_os("TOUCHHLE_DS_MANUAL_DIAGNOSTICS_ROOT")?);
        let wall=SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_millis();
        let root=base.join(format!("session-manual-{wall}-{}",std::process::id()));
        let start=Instant::now();let (tx,rx)=mpsc::sync_channel(16);
        let delivery=Arc::new(Delivery::default());let writer_delivery=delivery.clone();
        let version=RUNTIME_VERSION.get().cloned().unwrap_or_else(||"unknown".into());
        std::thread::Builder::new().name("ds-problem-captures".into()).spawn(move|| {
            if let Err(error)=problem_marks::manual_worker(root,rx,start,wall,version,&writer_delivery) {
                LEVEL.store(3,Relaxed);problem_marks::backend_failed();
                eprintln!("[DSissue] manual writer failed: {:?}",error.kind());
            }
            writer_delivery.ended.store(true,std::sync::atomic::Ordering::Release);
        }).ok()?;
        Some(Client{tx,start,delivery})
    }).as_ref().filter(|c|!c.delivery.closed() && !c.delivery.ended.load(std::sync::atomic::Ordering::Acquire))
}

fn collector_budget(scene:bool,super_trace:bool)->(u64,u64) {
    // Data-first external build: measured menu capture85-135ms/s under super
    // trace exhausted the former10/40ms budget and repeatedly hid details.
    // Keep basic conservative; disclose higher opt-in budgets, not zero cost.
    if super_trace {(200_000_000,500_000_000)}else if scene{(50_000_000,200_000_000)}else{(1_000_000,4_000_000)}
}
#[derive(Default)]
struct Governor { level:u32, healthy:u32 }
impl Governor {
    fn next(&mut self,main_ns:u64,backlog:usize,write_us:u64,io_failed:bool)->(u32,&'static str) {
        let (reduce,pause)=collector_budget(crate::ds_scene_diagnostics::requested(),crate::ds_stutter_probe::requested());
        let wanted=if io_failed{3}else if main_ns>pause || backlog>QUEUE*3/4 || write_us>100_000{2}
            else if main_ns>reduce || backlog>QUEUE/2 || write_us>20_000{1}else{0};
        let reason=if io_failed{"write_failed"}else if backlog>QUEUE/2{"queue_backlog"}else if main_ns>reduce{"collector_budget"}else if write_us>20_000{"slow_writer"}else{"healthy_hysteresis"};
        if wanted>self.level{self.level=wanted;self.healthy=0;}
        else if wanted<self.level && self.level<3 {self.healthy+=1;if self.healthy>=5{self.level-=1;self.healthy=0;}}
        else{self.healthy=0;}
        (self.level,reason)
    }
}
struct History { frames:VecDeque<Frame>,events:VecDeque<Value>,seconds:VecDeque<Value>,lost_frames:u64,lost_events:u64 }
impl History {
    fn new()->Self {Self{frames:VecDeque::with_capacity(FRAME_RING),events:VecDeque::with_capacity(EVENT_RING),seconds:VecDeque::with_capacity(120),lost_frames:0,lost_events:0}}
    fn frame(&mut self,f:Frame) {
        while self.frames.front().is_some_and(|old|old.t+RETAIN_US<f.t){self.frames.pop_front();}
        if self.frames.len()==FRAME_RING{self.frames.pop_front();self.lost_frames+=1;}self.frames.push_back(f);
    }
    fn event(&mut self,value:Value){if self.events.len()==EVENT_RING{self.events.pop_front();self.lost_events+=1;}self.events.push_back(value);}
    fn second(&mut self,value:Value){if self.seconds.len()==120{self.seconds.pop_front();}self.seconds.push_back(value);}
    fn dump(&self,trigger:u64,end:u64,reason:&str,extra:&Value)->Value {
        let from=trigger.saturating_sub(BEFORE_US);
        json!({"schema":1,"reason":reason,"trigger_us":trigger,"requested_from_us":from,"requested_until_us":trigger+AFTER_US,"dumped_at_us":end,
            "timebase":"session-start.json monotonic_us; wall clock mapping in session header", "frame_columns":["mono_us","native_present","interval_us","phase","native_state","native_hud"],
            "frames":self.frames.iter().filter(|f|f.t>=from && f.t<=end).map(|f|[f.t,f.n,f.dt,f.phase as u64,f.state as u64,f.hud as u64]).collect::<Vec<_>>(),
            "events":self.events.iter().filter(|v|v["mono_us"].as_u64().is_some_and(|t|t>=from && t<=end)).collect::<Vec<_>>(),
            "one_second_essentials":self.seconds.iter().filter(|v|v["mono_us"].as_u64().is_some_and(|t|t>=from && t<=end)).collect::<Vec<_>>(),
            "coverage":{"oldest_detail_us":self.frames.front().map(|f|f.t),"ring_evictions":self.lost_frames,"event_evictions":self.lost_events,
                "queue_dropped_total":LOST.load(Relaxed),"detail_skipped_total":SKIPPED.load(Relaxed),"current_detail_level":LEVEL.load(Relaxed),
                "note":"Missing frames/ranges are not evidence of no stutter. Startup shorter than 30s has only available history; pauses, process death and self-protection can leave gaps."},"explicit_user_note":extra})
    }
}
fn write_new(path:&Path,value:&Value)->io::Result<()> {
    let mut f=BufWriter::new(OpenOptions::new().write(true).create_new(true).open(path)?);serde_json::to_writer(&mut f,value)?;f.write_all(b"\n")?;f.flush()
}
fn replace_json(path:&Path,value:&Value)->io::Result<()> {
    let temp=path.with_extension("tmp");let mut f=File::create(&temp)?;serde_json::to_writer(&mut f,value)?;f.write_all(b"\n")?;drop(f);fs::rename(temp,path)
}
struct Sink {root:PathBuf,part:u64,timeline:BufWriter<File>,detail:BufWriter<File>,last_write_us:u64,total_write_us:u64,writes:u64,slow_ms:u64}
impl Sink {
    fn new(root:PathBuf)->io::Result<Self> {
        let timeline=BufWriter::with_capacity(131072,OpenOptions::new().create_new(true).write(true).open(root.join("timeline-000000.jsonl"))?);
        let mut detail=BufWriter::with_capacity(131072,OpenOptions::new().create_new(true).write(true).open(root.join("frames-000000.csv"))?);
        writeln!(detail,"mono_us,native_present,interval_us,phase,native_state,native_hud")?;
        Ok(Self{root,part:0,timeline,detail,last_write_us:0,total_write_us:0,writes:0,slow_ms:0})
    }
    fn json(&mut self,v:&Value)->io::Result<()> {serde_json::to_writer(&mut self.timeline,v)?;self.timeline.write_all(b"\n")}
    fn flush(&mut self,t:u64)->io::Result<()> {
        let begin=Instant::now();if self.slow_ms>0{std::thread::sleep(Duration::from_millis(self.slow_ms));}self.timeline.flush()?;self.detail.flush()?;
        let part=t/60_000_000;
        if part>self.part {
            self.part=part;
            self.timeline=BufWriter::with_capacity(131072,OpenOptions::new().create_new(true).write(true).open(self.root.join(format!("timeline-{part:06}.jsonl")))?);
            self.detail=BufWriter::with_capacity(131072,OpenOptions::new().create_new(true).write(true).open(self.root.join(format!("frames-{part:06}.csv")))?);
            writeln!(self.detail,"mono_us,native_present,interval_us,phase,native_state,native_hud")?;
        }
        self.last_write_us=begin.elapsed().as_micros() as u64;self.total_write_us+=self.last_write_us;self.writes+=1;Ok(())
    }
}
#[cfg(windows)]
fn memory()->Value {
    #[repr(C)] #[derive(Default)] struct FileTime {lo:u32,hi:u32}
    impl FileTime {fn ticks(&self)->u64{((self.hi as u64)<<32)|self.lo as u64}}
    #[repr(C)] struct Memory {cb:u32,faults:u32,peak:usize,working:usize,peak_paged:usize,paged:usize,peak_nonpaged:usize,nonpaged:usize,pagefile:usize,peak_pagefile:usize,private:usize}
    #[link(name="kernel32")] extern "system" {fn GetCurrentProcess()->*mut std::ffi::c_void;fn GetProcessTimes(p:*mut std::ffi::c_void,c:*mut FileTime,e:*mut FileTime,k:*mut FileTime,u:*mut FileTime)->i32;fn K32GetProcessMemoryInfo(p:*mut std::ffi::c_void,m:*mut Memory,n:u32)->i32;}
    let mut m:Memory=unsafe{std::mem::zeroed()};m.cb=std::mem::size_of::<Memory>() as u32;
    let size=m.cb;let(mut c,mut e,mut k,mut u)=(FileTime::default(),FileTime::default(),FileTime::default(),FileTime::default());
    let cpu=if unsafe{GetProcessTimes(GetCurrentProcess(),&mut c,&mut e,&mut k,&mut u)}!=0{Some(k.ticks().saturating_add(u.ticks()))}else{None};
    if unsafe{K32GetProcessMemoryInfo(GetCurrentProcess(),&mut m,size)}!=0{json!({"source":"K32GetProcessMemoryInfo","working_bytes":m.working,"private_bytes":m.private,"process_cpu_total_100ns":cpu,"cpu_scope":"all native process threads, cumulative, not normalized utilization"})}else{json!({"available":false})}
}
#[cfg(not(windows))] fn memory()->Value {json!({"available":false,"reason":"platform collector not implemented in this PC milestone"})}
#[cfg(windows)]
fn disk_free(path:&Path)->Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name="kernel32")] extern "system" {fn GetDiskFreeSpaceExW(path:*const u16,available:*mut u64,total:*mut u64,free:*mut u64)->i32;}
    let path:Vec<u16>=path.as_os_str().encode_wide().chain(Some(0)).collect();let mut bytes=0;
    (unsafe{GetDiskFreeSpaceExW(path.as_ptr(),&mut bytes,std::ptr::null_mut(),std::ptr::null_mut())}!=0).then_some(bytes)
}
#[cfg(not(windows))] fn disk_free(_path:&Path)->Option<u64>{None}
fn low_space(bytes:Option<u64>)->bool{bytes.is_some_and(|v|v<256*1024*1024)}
fn worker(root:PathBuf,rx:mpsc::Receiver<Message>,start:Instant,wall:u128,version:String,delivery:&Delivery)->io::Result<()> {
    let injected=std::env::var("TOUCHHLE_DS_BACKGROUND_TEST").as_deref()==Ok("1") && std::env::var("TOUCHHLE_DS_DIAGNOSTICS_FAULT_TEST").as_deref()==Ok("1");
    fs::create_dir_all(&root)?;fs::create_dir_all(root.join("requests/handled"))?;
    write_new(&root.join("session-start.json"),&json!({"schema":1,"pid":std::process::id(),"wall_unix_ms":wall,"monotonic_origin_us":0,"runtime_version":version,
        "statistics":"native present; phase-tagged; histogram p95/p99 are 0.5ms upper-bin estimates, overflow uses maximum; concurrent counter boundary can differ slightly",
        "phase_names":PHASE_NAMES,"queue_capacity":QUEUE,"frame_ring_capacity":FRAME_RING,"event_ring_capacity":EVENT_RING,
        "history_seconds":30,"detail_retention_seconds":45,"rotation_seconds":60,"total_capacity_limit":null,"automatic_deletion":false,
        "privacy":"no screenshots/recording/keystrokes/accounts/device unique IDs/full personal paths; no automatic upload",
        "origin_threads":"process-local collector tags, not OS/device/guest thread IDs; native_present_thread identifies a present producer; tags distinguish native worker producers without a call-stack claim",
        "budget_provisional":{"main_collect_ns_per_second":collector_budget(crate::ds_scene_diagnostics::requested(),crate::ds_stutter_probe::requested()).0,"main_pause_ns_per_second":collector_budget(crate::ds_scene_diagnostics::requested(),crate::ds_stutter_probe::requested()).1,"writer_reduce_us":20000,"writer_pause_us":100000,"recovery_healthy_seconds":5},
        "scene_diagnostics":crate::ds_scene_diagnostics::contract(),"stutter_probe_requested":crate::ds_stutter_probe::requested(),
        "fault_injection_test":injected,"modal_kind_codes":["popup","store","bench","pause","hud_menu","transition"],
        "present_submission_bits":{"low8":"FSR requested","bit8":"FSR eligible","bit9":"FSR final submitted","bit10":"shader submitted","bit11":"GPU text selected"},
        "hd_observer":"replacement submission and dimensions, not driver acceptance or GPU completion; app-observed GL errors separately recorded without extra GL queries",
        "missing_observers":["logical quest completion is not inferred","OS crash signals are not guaranteed to be intercepted","Android thermal collector deferred"]}))?;
    let mut sink=Sink::new(root.clone())?;let mut history=History::new();let mut governor=Governor::default();
    let (mut second,mut flush_at,mut health_at)=(0,0,0);
    let mut pending:Option<(u64,u64,String,Value)>=None;let mut cooldown=0;let mut failed:Option<String>=None;let mut gap_start=None;
    let mut lag_max=0;let mut pending_io_us=0;let mut incident_serial=0u64;
    let mut free_bytes=disk_free(&root);let mut disk_checked=0;
    let mut problems=problem_marks::Jobs::default();
    let (mut frames_written,mut events_written,mut frames_persisted,mut events_persisted)=(0u64,0u64,0u64,0u64);
    loop {
        // Poll in bounded background batches. Registering a parked receiver
        // woke a Windows thread on every present; this keeps try_send cheap.
        let message=rx.try_recv().ok();
        if message.is_none(){std::thread::sleep(Duration::from_millis(5));}
        let work_started=Instant::now();
        let t=start.elapsed().as_micros() as u64;
        if let Some(message)=message {
            delivery.queued.fetch_sub(1,Relaxed);
            let value=match message {
                Message::Frame(f)=>{
                    lag_max=lag_max.max(t.saturating_sub(f.t));
                    history.frame(f);
                    if failed.is_none() {match writeln!(sink.detail,"{},{},{},{},{},{}",f.t,f.n,f.dt,f.phase,f.state,f.hud){Ok(())=>frames_written+=1,Err(e)=>failed=Some(format!("{:?}",e.kind()))}}
                    if f.phase==4 && f.dt>=100_000 && f.t>=cooldown {
                        if let Some((first,until,_,_))=&mut pending{*until=(f.t+AFTER_US).min(*first+10_000_000);}
                        else{pending=Some((f.t,f.t+AFTER_US,"severe_live_frame".into(),Value::Null));}
                    }
                    None
                }
                Message::Event(at,code,values,thread)=>{
                    if (code=="gl_error_observed" || code.ends_with("failed") || (code=="native_text_mount" && values[0]==0)) && at>=cooldown && pending.is_none(){pending=Some((at,at+AFTER_US,"reliable_error_observed".into(),json!({"code":code})));}
                    Some(json!({"type":"native_event","mono_us":at,"code":code,"values":values,"origin_thread_tag":thread}))
                },
                Message::Feature(at,code,text,thread)=>Some(json!({"type":"actual_feature","mono_us":at,"code":code,"value":text,"origin_thread_tag":thread})),
                Message::Picture(picture)=>{problems.picture(picture,&root);None},
                Message::Problem(problem)=>{
                    if failed.is_none(){if let Err(e)=problems.accept(problem,t,&root,&mut history,&mut sink){failed=Some(format!("{:?}",e.kind()));problem_marks::backend_failed();}}
                    else{problem_marks::backend_failed();}None
                },
            };
            if let Some(v)=value {history.event(v.clone());if failed.is_none(){match sink.json(&v){Ok(())=>events_written+=1,Err(e)=>failed=Some(format!("{:?}",e.kind()))}}}
        }
        if t>=second+1_000_000 {
            let elapsed=t-second;second=t;
            if injected {
                let path=root.join("test-writer-delay-ms");
                sink.slow_ms=fs::metadata(&path).ok().filter(|m|m.is_file()&&m.len()<=16)
                    .and_then(|_|fs::read_to_string(path).ok()).and_then(|s|s.trim().parse::<u64>().ok()).unwrap_or(0).min(500);
            }
            if t>=disk_checked+10_000_000 || injected {
                disk_checked=t;free_bytes=disk_free(&root);
                if injected {let path=root.join("test-free-space-bytes");if fs::metadata(&path).is_ok_and(|m|m.is_file()&&m.len()<=24){free_bytes=fs::read_to_string(path).ok().and_then(|s|s.trim().parse().ok());}}
            }
            if low_space(free_bytes){failed=Some("low_disk_space".into());}
            let metrics=COUNTERS.get().unwrap().iter().zip(PHASE_NAMES).map(|(counter,name)|(name.to_owned(),counter.take())).collect::<serde_json::Map<_,_>>();
            let collect=COLLECT_NS.swap(0,Relaxed);let old=governor.level;
            let (level,reason)=governor.next(collect.saturating_mul(1_000_000)/elapsed.max(1),delivery.queued.load(Relaxed),pending_io_us,failed.is_some());pending_io_us=0;LEVEL.store(level,Relaxed);
            if old!=level {
                if level>0 && gap_start.is_none(){gap_start=Some(t);}
                let v=json!({"type":"self_protection","mono_us":t,"old_level":old,"level":level,"reason":reason,"missing_range_start_us":gap_start,"missing_range_end_us":if level==0{Some(t)}else{None},
                    "detail_policy":match level{0=>"all frames",1=>"one in four frames",2=>"detail paused; lightweight counters retained",_=>"writer failed; lightweight counters only"},"dropped_total":LOST.load(Relaxed),"skipped_total":SKIPPED.load(Relaxed)});
                if level==0{gap_start=None;}history.event(v.clone());if failed.is_none(){if let Err(e)=sink.json(&v){failed=Some(format!("{:?}",e.kind()));}}
            }
            let scene=crate::ds_scene_diagnostics::snapshot();
            let essential=json!({"scene_activity":scene,"type":"one_second_essentials","mono_us":t,"window_us":elapsed,"phase_statistics":metrics,"detail_level":level,"collector_ns":collect,"collector_max_ns":COLLECT_MAX_NS.swap(0,Relaxed),"collector_core_thread_tag":CORE_THREAD.load(Relaxed),"secondary_thread_collector_ns":SECONDARY_COLLECT_NS.swap(0,Relaxed),"secondary_thread_collector_max_ns":SECONDARY_COLLECT_MAX_NS.swap(0,Relaxed),"queue_backlog":delivery.queued.load(Relaxed),"queue_lag_max_us":lag_max,"dropped_total":LOST.load(Relaxed),"detail_skipped_total":SKIPPED.load(Relaxed)});
            history.second(essential.clone());lag_max=0;
            if failed.is_none() {if let Err(e)=sink.json(&essential){failed=Some(format!("{:?}",e.kind()));}}
            // At most 16 explicit, bounded note requests per poll. Handled files
            // move out of the request directory; no historical file is deleted.
            if let Ok(items)=fs::read_dir(root.join("requests")) {
                for entry in items.filter_map(Result::ok).filter(|v|v.file_name().to_str().is_some_and(|n|n.ends_with(".request.json"))).take(16) {
                    let path=entry.path();let Some(meta)=fs::symlink_metadata(&path).ok() else{continue;};if !meta.is_file()||meta.file_type().is_symlink()||meta.len()>4096{continue;}
                    if let Ok(bytes)=fs::read(&path) {if let Ok(v)=serde_json::from_slice::<Value>(&bytes) {
                        let note=v["note"].as_str().unwrap_or("").chars().take(500).collect::<String>();
                        let problem=problem_marks::Problem::new(t,"launcher_note",note);
                        if failed.is_none(){if let Err(e)=problems.accept(problem,t,&root,&mut history,&mut sink){failed=Some(format!("{:?}",e.kind()));problem_marks::backend_failed();}}
                        let _=fs::rename(&path,root.join("requests/handled").join(entry.file_name()));
                    }}
                }
            }
        }
        if failed.is_none(){if let Err(e)=problems.tick(t,false,&root,&history){failed=Some(format!("{:?}",e.kind()));problem_marks::backend_failed();}}
        if FATAL.swap(false,Relaxed){pending=Some((t,t,"native_fatal_or_panic".into(),Value::Null));}
        if pending.as_ref().is_some_and(|(_,until,_,_)|t>=*until) {
            let (trigger,_,reason,note)=pending.take().unwrap();cooldown=t+10_000_000;incident_serial+=1;
            if failed.is_none() {
                let begin=Instant::now();let value=history.dump(trigger,t,&reason,&note);
                let result=write_new(&root.join(format!("incident-{trigger:016}-{incident_serial:04}.json")),&value);
                let cost=begin.elapsed().as_micros() as u64;pending_io_us=pending_io_us.max(cost);
                if let Err(e)=result{failed=Some(format!("{:?}",e.kind()));}
                let _=sink.json(&json!({"type":"incident_dump","mono_us":t,"trigger_us":trigger,"writer_us":cost,"reason":reason}));
            }
        }
        if t>=flush_at+2_000_000 && failed.is_none() {
            flush_at=t;match sink.flush(t){Ok(())=>{frames_persisted=frames_written;events_persisted=events_written;},Err(e)=>failed=Some(format!("{:?}",e.kind()))}pending_io_us=pending_io_us.max(sink.last_write_us);
        }
        if failed.is_some() && LEVEL.swap(3,Relaxed)!=3 {
            governor.level=3;gap_start=Some(t);
            history.event(json!({"type":"writer_stopped","mono_us":t,"reason":failed,"level":3}));
        }
        if t>=health_at+2_000_000 {
            health_at=t;
            let health=json!({"persisted_frame_records":frames_persisted,"persisted_event_records":events_persisted,"problem_marks_pending":problems.len(),"schema":1,"mono_us":t,"detail_level":LEVEL.load(Relaxed),"writer_error":failed,"queue_backlog":delivery.queued.load(Relaxed),"dropped_total":LOST.load(Relaxed),"detail_skipped_total":SKIPPED.load(Relaxed),"open_missing_range_us":gap_start,
                "last_flush_us":sink.last_write_us,"total_flush_us":sink.total_write_us,"flushes":sink.writes,"injected_writer_delay_ms":sink.slow_ms,"disk_free_bytes":free_bytes,"disk_reserve_bytes":256u64*1024*1024,"memory":memory(),"scene_trace_requested":crate::ds_scene_diagnostics::requested(),"stutter_probe_active":crate::ds_stutter_probe::enabled(),"scope":"writer activity and long frames aligned by monotonic time; coincidence is not proof of causality"});
            // A failed backend attempts this small status once, never per frame.
            if let Err(error)=replace_json(&root.join("health.json"),&health){if failed.is_none(){failed=Some(format!("{:?}",error.kind()));}}
            if failed.is_some(){health_at=u64::MAX-60_000_000;}
        }
        if delivery.closed() && (delivery.drained() || delivery.timed_out()) {
            let drained=delivery.drained();
            let tail=delivery.queued.load(std::sync::atomic::Ordering::Acquire);
            let inflight=delivery.inflight();
            if failed.is_none(){if let Err(e)=problems.tick(t,true,&root,&history){failed=Some(format!("{:?}",e.kind()));problem_marks::backend_failed();}}
            if let Some((trigger,_,reason,note))=pending.take(){let _=write_new(&root.join(format!("incident-exit-{trigger:016}.json")),&history.dump(trigger,t,&reason,&note));}
            if let Err(e)=sink.flush(t){if failed.is_none(){failed=Some(format!("{:?}",e.kind()));}}
            write_new(&root.join("session-end.json"),&json!({"schema":1,"mono_us":t,"normal_exit_observed":!BAD_END.load(Relaxed),"writer_error":failed,"queued_tail_not_persisted":tail,"dropped_total":LOST.load(Relaxed),
                "shutdown_queue_drained":drained,"shutdown_drain_timed_out":!drained,"shutdown_senders_inflight":inflight,"shutdown_drain_limit_ms":delivery::DRAIN_LIMIT.as_millis(),
                "shutdown_rejected_after_close":delivery.rejected(),"shutdown_close_mono_us":delivery.closed_at().map(|at|at.saturating_duration_since(start).as_micros()),
                "shutdown_scope":"Admission closed before drain; accepted producers finish or150ms timeout is disclosed. Main exit waits at most200ms; filesystem stalls can outlast that wait and leave session-end absent."}))?;
            return Ok(());
        }
        // Includes encoding, any implicit buffered-write flush, incident dumps,
        // directory polling, memory queries and explicit writes. recv wait is
        // excluded. This measures background activity, not inferred game cost.
        pending_io_us=pending_io_us.max(work_started.elapsed().as_micros() as u64);
    }
}

#[cfg(test)]
#[path="ds_diagnostics_shutdown_tests.rs"] mod shutdown_tests;
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn data_first_budgets_preserve_basic_and_disclose_opt_in_overhead() {
        assert_eq!(collector_budget(false,false),(1_000_000,4_000_000));
        assert_eq!(collector_budget(true,false),(50_000_000,200_000_000));
        assert_eq!(collector_budget(true,true),(200_000_000,500_000_000));
        assert_eq!(collector_budget(false,true),collector_budget(true,true));
    }
    #[test] fn phase_gates_do_not_turn_loading_cutscene_death_or_stations_into_gameplay() {
        assert_eq!(phase(0,0),4);assert_eq!(phase(0,5),4);assert_eq!(phase(0,29),5);assert_eq!(phase(0,26),7);
        for state in 19..=24{assert_eq!(phase(0,state),8);}assert_eq!(phase(2,0),9);assert_eq!(phase(4,0),6);assert_eq!(phase(u32::MAX,0),0);
        assert!(!fresh(50,u64::MAX));assert!(!fresh(50,40));assert!(fresh(50,49));
    }
    #[test] fn low_space_protection_is_a_free_space_floor_not_a_total_log_quota() {
        assert!(low_space(Some(1)));assert!(!low_space(Some(1024*1024*1024)));assert!(!low_space(None));
    }
    #[test] fn counter_retains_severe_valid_intervals() {
        let c=Counter::new();c.push(16_667);c.push(2_000_000);let v=c.take();assert_eq!(v["frames"],2);assert_eq!(v["max_ms"],2000.0);assert_eq!(v["over50ms"],1);assert_eq!(c.take()["frames"],0);
    }
    #[test] fn history_retains_thirty_seconds_before_and_three_after_trigger() {
        let mut h=History::new();for n in 0..4000 {h.frame(Frame{t:n*10000,n,dt:10000,phase:4,state:0,hud:0});}
        let dump=h.dump(35_000_000,38_000_000,"test",&Value::Null);let frames=dump["frames"].as_array().unwrap();
        assert_eq!(frames.first().unwrap()[0],5_000_000);assert_eq!(frames.last().unwrap()[0],38_000_000);
    }
    #[test] fn history_memory_is_bounded_and_missing_detail_is_disclosed() {
        let mut h=History::new();for n in 0..FRAME_RING+100{h.frame(Frame{t:n as u64,n:n as u64,dt:1,phase:4,state:0,hud:0});}
        for n in 0..EVENT_RING+20{h.event(json!({"mono_us":n}));}for n in 0..140{h.second(json!({"mono_us":n}));}
        assert_eq!(h.frames.len(),FRAME_RING);assert_eq!(h.events.len(),EVENT_RING);assert_eq!(h.seconds.len(),120);
        assert_eq!(h.lost_frames,100);assert_eq!(h.lost_events,20);
    }
    #[test] fn protection_pauses_on_backlog_and_requires_healthy_hysteresis() {
        let mut g=Governor::default();assert_eq!(g.next(0,QUEUE,0,false).0,2);
        for _ in 0..4{assert_eq!(g.next(0,0,0,false).0,2);}assert_eq!(g.next(0,0,0,false).0,1);
        assert_eq!(g.next(0,0,0,true).0,3);for _ in 0..20{assert_eq!(g.next(0,0,0,false).0,3);}
    }
    #[test] fn protection_accounts_for_collector_and_dump_writer_cost() {
        assert_eq!(Governor::default().next(1_500_000,0,0,false).0,1);
        assert_eq!(Governor::default().next(0,0,150_000,false).0,2);
    }
    #[test] fn full_queue_never_blocks_and_counts_dropped_messages() {
        let (tx,_rx)=mpsc::sync_channel(1);let c=Client{tx,start:Instant::now(),delivery:Arc::new(Delivery::default())};let before=LOST.load(Relaxed);
        send(&c,Message::Event(0,"first",[0;4],0));let t=Instant::now();send(&c,Message::Event(0,"full",[0;4],0));
        assert!(t.elapsed()<Duration::from_millis(50));assert!(LOST.load(Relaxed)>before);c.delivery.queued.fetch_sub(1,Relaxed);
    }
    fn fixture()->PathBuf {
        let path=std::env::temp_dir().join(format!("ds-diagnostics-test-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&path).unwrap();assert_eq!(path.parent().unwrap().canonicalize().unwrap(),std::env::temp_dir().canonicalize().unwrap());path
    }
    #[test] fn real_slow_flush_is_measured_and_governor_pauses_detail() {
        let path=fixture();let mut sink=Sink::new(path.clone()).unwrap();sink.slow_ms=120;sink.flush(0).unwrap();
        assert!(sink.last_write_us>=100_000);assert_eq!(Governor::default().next(0,0,sink.last_write_us,false).0,2);drop(sink);
        fs::remove_file(path.join("timeline-000000.jsonl")).unwrap();fs::remove_file(path.join("frames-000000.csv")).unwrap();fs::remove_dir(path).unwrap();
    }
    #[test] fn real_readonly_handle_write_error_is_propagated_without_overwriting_source() {
        let path=fixture();let source=path.join("original.bin");fs::write(&source,b"preserve").unwrap();
        let mut sink=Sink::new(path.clone()).unwrap();sink.detail=BufWriter::new(File::open(&source).unwrap());writeln!(sink.detail,"fails on flush").unwrap();
        assert!(sink.flush(0).is_err());assert_eq!(fs::read(&source).unwrap(),b"preserve");drop(sink);
        for name in ["original.bin","timeline-000000.jsonl","frames-000000.csv"]{fs::remove_file(path.join(name)).unwrap();}fs::remove_dir(path).unwrap();
    }
    #[test] fn rotation_preserves_old_parts_and_never_overwrites_existing_output() {
        let path=fixture();let mut sink=Sink::new(path.clone()).unwrap();sink.flush(61_000_000).unwrap();assert!(Sink::new(path.clone()).is_err());drop(sink);
        for name in ["timeline-000000.jsonl","frames-000000.csv","timeline-000001.jsonl","frames-000001.csv"]{assert!(path.join(name).is_file());fs::remove_file(path.join(name)).unwrap();}fs::remove_dir(path).unwrap();
    }
}

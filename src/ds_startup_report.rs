//! Small current-run report shared by desktop and Android frontends.
//! Feature events write only during initialization. Optional benchmark writes
//! once on completion; the present path never polls files or reads pixels.
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;
use serde_json::{json, Value};

static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
static REPORT: OnceLock<Mutex<Value>> = OnceLock::new();
static BENCH: OnceLock<bool> = OnceLock::new();
static DONE: AtomicBool = AtomicBool::new(false);
static LIVE_STATE: AtomicU32 = AtomicU32::new(u32::MAX);
static LIVE_FRAME: AtomicU64 = AtomicU64::new(0);
static PAUSED: AtomicBool = AtomicBool::new(false);
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
// Called before native options may have initialized BENCH. Never query the
// environment/OnceLock here: Android onResume can precede getArguments.
#[cfg(target_os="android")]
#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_MainActivity_nativeBenchmarkPause(
    _env: *mut std::ffi::c_void, _class: *mut std::ffi::c_void, paused: u8,
) {
    PAUSED.store(paused!=0,Ordering::Relaxed);
    INTERRUPTED.store(true,Ordering::Relaxed);
}

pub fn enabled() -> bool { path().is_some() }
fn path() -> &'static Option<PathBuf> {
    PATH.get_or_init(|| std::env::var_os("TOUCHHLE_DS_STARTUP_REPORT").filter(|s| !s.is_empty()).map(PathBuf::from))
}
pub fn benchmark_active() -> bool {
    *BENCH.get_or_init(|| std::env::var("TOUCHHLE_DS_BENCHMARK").as_deref() == Ok("1")) && !DONE.load(Ordering::Relaxed)
}
fn update(change: impl FnOnce(&mut Value)) {
    let Some(path) = path() else { return; };
    let state = REPORT.get_or_init(|| Mutex::new(json!({"schema":1,"pid":std::process::id(),
        "text_patch":null,"renderer":null,"width":null,"height":null,"af_requested":null,"af_actual":null,
        "af_verified":false,"msaa_requested":null,"msaa_actual":null,
        "benchmark":{"phase":if benchmark_active(){"waiting_play"}else{"not_requested"}}})));
    let Ok(mut data) = state.lock() else { return; };
    change(&mut data);
    let Ok(bytes) = serde_json::to_vec(&*data) else { return; };
    let temp=path.with_extension("tmp");
    let result=(|| -> std::io::Result<()> {
        if let Some(parent)=path.parent(){std::fs::create_dir_all(parent)?;}
        std::fs::write(&temp,bytes)?;std::fs::rename(&temp,path)
    })();
    if let Err(error)=result { static LOGGED:AtomicBool=AtomicBool::new(false); if !LOGGED.swap(true,Ordering::Relaxed) { log!("[DSstartup] report unavailable: {}",error); } }
}
pub fn text_patch(mounted: bool) {
    crate::ds_diagnostics::event("native_text_mount",[mounted as u64,0,0,0]);
    update(|r| r["text_patch"]=json!(mounted));
}
pub fn renderer(description: &str, width: u32, height: u32) {
    crate::ds_diagnostics::feature("renderer",description);
    crate::ds_diagnostics::event("renderbuffer_allocated",[width as u64,height as u64,0,0]);
    update(|r| {r["renderer"]=json!(description);r["width"]=json!(width);r["height"]=json!(height);});
}
pub fn anisotropy(requested: f32, actual: f32, verified: bool) {
    crate::ds_diagnostics::event("anisotropy_readback",[requested as u64,actual as u64,verified as u64,0]);
    update(|r| {r["af_requested"]=json!(requested);r["af_actual"]=json!(actual);r["af_verified"]=json!(verified);});
}
pub fn msaa(requested: u32, actual: u32) {
    crate::ds_diagnostics::event("msaa_allocated",[requested as u64,actual as u64,0,0]);
    update(|r| {r["msaa_requested"]=json!(requested);r["msaa_actual"]=json!(actual);});
}
pub fn player_state(frame: u64, state: u32) {
    if !benchmark_active() { return; }
    LIVE_STATE.store(state,Ordering::Relaxed);LIVE_FRAME.store(frame,Ordering::Relaxed);
}
pub fn present(frame: u64) {
    if !benchmark_active() { return; }
    struct Active { last:Option<Instant>, sample:crate::ds_hardware_benchmark::Sample }
    static ACTIVE:OnceLock<Mutex<Active>>=OnceLock::new();
    let active=ACTIVE.get_or_init(||Mutex::new(Active{last:None,sample:crate::ds_hardware_benchmark::Sample::new()}));
    let Ok(mut active)=active.lock() else {return;};
    let now=Instant::now();let prior=active.last.replace(now);
    if PAUSED.load(Ordering::Relaxed) || INTERRUPTED.swap(false,Ordering::Relaxed) {active.sample.interrupt();return;}
    let Some(prior)=prior else{return;};
    let live=crate::ds_hardware_benchmark::live_state(LIVE_STATE.load(Ordering::Relaxed),frame,LIVE_FRAME.load(Ordering::Relaxed));
    let Some(result)=active.sample.push(now.duration_since(prior).as_micros().min(u64::MAX as u128) as u64,live) else{return;};
    DONE.store(true,Ordering::Relaxed);drop(active);
    update(|r| r["benchmark"]=json!({"phase":"complete","frames":result.frames,"seconds":result.seconds,
        "fps":result.fps,"p95_ms":result.p95_ms,"p99_ms":result.p99_ms,"over50ms":result.over50,
        "scope":"optional active-scene sample; no guarantee of all-level60"}));
    log!("[DSbenchmark] completed frames={} seconds={:.3} fps={:.2} p99_ms={:.3}",result.frames,result.seconds,result.fps,result.p99_ms);
}

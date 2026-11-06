//! Explicit opt-in wall-time spans for stutter localization. No input, pixels,
//! driver queries or IO. Nested spans overlap and must not be summed.
use std::sync::OnceLock;
use std::time::Instant;
pub(crate) fn requested()->bool {
    static ON:OnceLock<bool>=OnceLock::new();
    *ON.get_or_init(||std::env::var("TOUCHHLE_DS_STUTTER_PROBE").as_deref()==Ok("1"))
}
pub(crate) fn enabled()->bool {requested() && crate::ds_diagnostics::detail_enabled()}
pub(crate) fn sample_due()->bool {
    if !enabled(){return false;}
    thread_local! {static LAST:std::cell::Cell<Option<Instant>>=const{std::cell::Cell::new(None)};}
    let now=Instant::now();
    LAST.with(|last| {
        if last.get().is_some_and(|previous|now.duration_since(previous)<std::time::Duration::from_millis(200)){false}
        else{last.set(Some(now));true}
    })
}
pub(crate) struct Span {start:Option<Instant>,kind:u64,tag:u64}
impl Span {
    pub(crate) fn new(kind:u64,tag:u64)->Self {
        let start=if enabled(){Some(Instant::now())}else{None};
        if let Some(cost)=start{crate::ds_diagnostics::collector_cost(cost);}
        Self{start,kind,tag}
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        let Some(start)=self.start else{return;};let cost=Instant::now();let us=cost.duration_since(start).as_micros().min(u64::MAX as u128) as u64;
        crate::ds_diagnostics::collector_cost(cost);
        if us>=2000 && crate::ds_diagnostics::detail_enabled() {crate::ds_diagnostics::event("runtime_wall_span",[self.kind,us,crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),self.tag]);}
    }
}

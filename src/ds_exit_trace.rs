//! Opt-in lifecycle wait observations. No callback, save or scheduler behavior changes.
use crate::Environment;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct State { trace: Option<Trace> }
struct Trace {
    started: Instant,
    last_report: Instant,
    stage: &'static str,
    points: u32,
    reports: u8,
}

pub(crate) fn configured() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    cfg!(target_os="windows") && *ENABLED.get_or_init(||
        std::env::var("TOUCHHLE_DS_LIFECYCLE_TRACE").as_deref()==Ok("1"))
}

pub(crate) fn point(env: &mut Environment, stage: &'static str) {
    if stage=="begin" && configured() && env.bundle.bundle_identifier()=="com.ea.deadspace.bv" {
        let now=Instant::now();
        env.framework_state.exit_trace_state().trace=Some(Trace {
            started:now,last_report:now,stage,points:0,reports:0,
        });
    }
    let Some(trace)=env.framework_state.exit_trace_state().trace.as_mut() else {return};
    trace.stage=stage;trace.points+=1;
    echo!("[DSexit] stage={} elapsed_ms={} thread={} frame={}",stage,
        trace.started.elapsed().as_millis(),env.current_thread,
        crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed));
    if stage!="diagnostics.finished" {
        crate::ds_diagnostics::event("lifecycle_exit",[env.current_thread as u64,
            trace.points as u64,trace.started.elapsed().as_millis() as u64,0]);
    }
}

/// Called only by an explicitly enabled run at a cooperative host boundary.
pub(crate) fn poll(env: &mut Environment) {
    let now=Instant::now();
    let report={
        let Some(trace)=env.framework_state.exit_trace_state().trace.as_mut() else {return};
        if trace.reports>=4 || now.duration_since(trace.started)<Duration::from_secs(1)
            || now.duration_since(trace.last_report)<Duration::from_secs(2) {return}
        trace.reports+=1;trace.last_report=now;
        (trace.stage,now.duration_since(trace.started).as_millis(),trace.reports)
    };
    echo!("[DSexit] waiting stage={} elapsed_ms={} snapshot={}",report.0,report.1,report.2);
    env.trace_exit_threads();
}

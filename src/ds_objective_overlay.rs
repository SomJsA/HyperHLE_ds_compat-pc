//! Objective Popup display takeover. Original initializer/update/navigation run.
#[path = "ds_objective_policy.rs"]
mod policy;
use crate::abi::{CallFromHost, GuestFunction};
use crate::ds_chapter_overlay::{patch, wide, word};
use crate::Environment;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
static READY: AtomicBool = AtomicBool::new(false);
static SNAPSHOT: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
pub(crate) struct State {
    checked: bool,
    armed: bool,
    suppressed: bool,
    owner: (u32, u32),
    index: Option<u32>,
    pending: bool,
    played: bool,
    serial: u32,
    last: u64,
}

fn suppress(env: &mut Environment, desired: bool) -> bool {
    let current = env.framework_state.opengles.ds_objective.suppressed;
    if current == desired { return true; }
    if word(env, policy::DRAW_SITE) != Some(if current { policy::NOP } else { policy::DRAW_ORIGINAL }) {
        echo!("[DSobjective-native] instruction mismatch; custom display withheld"); return false;
    }
    patch(env, policy::DRAW_SITE, if desired { policy::NOP } else { policy::DRAW_ORIGINAL });
    env.framework_state.opengles.ds_objective.suppressed = desired;
    echo!("[DSobjective-native] {}", if desired { "hidden" } else { "restored" });
    true
}

pub(crate) fn observe(env: &mut Environment) {
    if std::env::var_os("TOUCHHLE_DS_OBJECTIVE_OVERLAY").is_none() { return; }
    if !env.framework_state.opengles.ds_objective.checked {
        if !env.bundle.bundle_identifier().to_ascii_lowercase().contains("deadspace") { return; }
        env.framework_state.opengles.ds_objective.checked = true;
        if word(env, policy::CALL_SITE) != Some(policy::CALL_ORIGINAL)
            || word(env, policy::DRAW_SITE) != Some(policy::DRAW_ORIGINAL) {
            echo!("[DSobjective-hook] unsupported binary; retaining native display"); return;
        }
        patch(env, policy::CALL_SITE, policy::BREAKPOINT);
        env.framework_state.opengles.ds_objective.armed = true;
        echo!("[DSobjective-hook] ready; original initializer and update retained");
    }
    if !env.framework_state.opengles.ds_objective.armed { return; }
    let hud: u32 = GuestFunction::from_addr_and_thumb_flag(0x0003_0474, false).call_from_host(env, ());
    let world = if hud >= 0x1000 { word(env, hud + 4).unwrap_or(0) } else { 0 };
    let owner = if hud >= 0x1000 && world >= 0x1000 { (hud, world) } else { (0,0) };
    let phase = word(env, hud.saturating_add(0xf8c)).unwrap_or(0);
    let eligible = env.mem.get_bytes_fallible(crate::mem::ConstVoidPtr::from_bits(hud.saturating_add(0xfa4)),1)
        .and_then(|b| b.first()).copied().unwrap_or(0);
    let index = {
        let state = &mut env.framework_state.opengles.ds_objective;
        if state.owner != owner {
            if !state.pending { state.index = None; }
            state.owner = owner; state.played = false;
        }
        // Keep the last objective across phase 0. Hud::ResetObjectivePopup
        // (used by the native G locator) restarts the same popup without
        // emitting a new TriggerObjectivePopup event, so clearing here makes
        // every locator replay fall back to the native text.
        if owner == (0,0) || (phase == 0 && state.played) { state.played = false; }
        state.pending = false;
        if matches!(phase, 1..=3) { state.played = true; }
        state.index
    };
    let show = owner != (0,0) && index.is_some() && READY.load(Ordering::Acquire)
        && policy::visible(phase, eligible, word(env,hud.saturating_add(0x234)).unwrap_or(u32::MAX),
            word(env,hud.saturating_add(0x240)).unwrap_or(u32::MAX), word(env,hud.saturating_add(0xdb4)).unwrap_or(1),
            word(env,hud.saturating_add(0x374)).unwrap_or(u32::MAX));
    let patched = suppress(env,show);
    let value = if show && patched {
        (u64::from(env.framework_state.opengles.ds_objective.serial)<<32) | (u64::from(phase)<<16) | u64::from(index.unwrap())
    } else { 0 };
    SNAPSHOT.store(value, Ordering::Release);
    let state = &mut env.framework_state.opengles.ds_objective;
    if state.last != value {
        echo!("[DSobjective-view] serial={} index={:?} phase={} visible={}",state.serial,index,phase,value!=0);
        state.last = value;
    }
}

pub(crate) fn handle_breakpoint(env: &mut Environment) -> bool {
    if !env.framework_state.opengles.ds_objective.armed || env.cpu.regs()[crate::cpu::Cpu::PC] != policy::CALL_SITE { return false; }
    let id = wide(env,env.cpu.regs()[1]);
    let index = id.as_deref().and_then(crate::ds_subtitle_overlay::text_entry);
    let state=&mut env.framework_state.opengles.ds_objective;
    state.index=index; state.pending=true; state.played=false; state.serial=state.serial.wrapping_add(1);
    echo!("[DSobjective-event] serial={} id={:?} mapped={}",state.serial,id,index.is_some());
    env.cpu.regs_mut()[crate::cpu::Cpu::LR]=policy::CALL_SITE+4;
    env.cpu.regs_mut()[crate::cpu::Cpu::PC]=0x002f_80d0;
    true
}

#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_ObjectiveOverlay_nativeSnapshot(
    _env:*mut std::ffi::c_void,_class:*mut std::ffi::c_void,
) -> i64 { SNAPSHOT.load(Ordering::Acquire) as i64 }
#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_ObjectiveOverlay_nativeSetReady(
    _env:*mut std::ffi::c_void,_class:*mut std::ffi::c_void,ready:u8,
) {
    READY.store(ready!=0,Ordering::Release);
    if ready==0 { SNAPSHOT.store(0,Ordering::Release); }
}

pub(crate) fn host_ready(ready:bool) { READY.store(ready, Ordering::Release); }
pub(crate) fn host_snapshot()->u64 { SNAPSHOT.load(Ordering::Acquire) }

//! Android chapter event takeover. Guest timing stays in ChapterPopup; the
//! Android Canvas view draws independently of GLES/FSR and consumes no touches.
//! Instruction addresses apply only to the checked Dead Space 1.3.34 binary.

#[path = "ds_chapter_policy.rs"]
mod policy;

use crate::abi::{CallFromHost, GuestFunction};
use crate::mem::{ConstVoidPtr, MutPtr};
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
    chapter: Option<u8>,
    pending: bool,
    played: bool,
    serial: u32,
    last: Option<(u8, u32, bool)>,
}

pub(crate) fn word(env: &Environment, address: u32) -> Option<u32> {
    let b = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(address), 4)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

pub(crate) fn wide(env: &Environment, address: u32) -> Option<String> {
    let start = word(env, address)?;
    let end = word(env, address.checked_add(4)?)?;
    let len = end.checked_sub(start)?;
    if start < 0x1000 || len == 0 || len > 256 || len % 4 != 0 {
        return None;
    }
    let b = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(start), len)?;
    b.chunks_exact(4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
        .take_while(|v| *v != 0)
        .map(char::from_u32)
        .collect::<Option<String>>()
}

pub(crate) fn patch(env: &mut Environment, address: u32, value: u32) {
    env.mem.write(MutPtr::<u32>::from_bits(address), value);
    env.cpu.invalidate_cache_range(address, 4);
}

fn suppress(env: &mut Environment, desired: bool) -> bool {
    let current = env.framework_state.opengles.ds_chapter.suppressed;
    if current == desired {
        return true;
    }
    let expected = if current {
        policy::ARM_NOP
    } else {
        policy::DRAW_ORIGINAL
    };
    if word(env, policy::DRAW_SITE) != Some(expected) {
        echo!("[DSchapter-native] instruction mismatch; custom display withheld");
        return false;
    }
    patch(
        env,
        policy::DRAW_SITE,
        if desired {
            policy::ARM_NOP
        } else {
            policy::DRAW_ORIGINAL
        },
    );
    env.framework_state.opengles.ds_chapter.suppressed = desired;
    echo!(
        "[DSchapter-native] {}",
        if desired { "hidden" } else { "restored" }
    );
    true
}

pub(crate) fn observe(env: &mut Environment) {
    if std::env::var_os("TOUCHHLE_DS_CHAPTER_OVERLAY").is_none() {
        return;
    }
    if !env.framework_state.opengles.ds_chapter.checked {
        // Check both sites before writing either. Other titles/versions retain
        // their original code, and no getter at a guessed address is invoked.
        if !env
            .bundle
            .bundle_identifier()
            .to_ascii_lowercase()
            .contains("deadspace")
        {
            return;
        }
        env.framework_state.opengles.ds_chapter.checked = true;
        if word(env, policy::CALL_SITE) != Some(policy::CALL_ORIGINAL)
            || word(env, policy::DRAW_SITE) != Some(policy::DRAW_ORIGINAL)
        {
            echo!("[DSchapter-hook] unsupported binary; original chapter drawing retained");
            return;
        }
        patch(env, policy::CALL_SITE, policy::CALL_BREAKPOINT);
        env.framework_state.opengles.ds_chapter.armed = true;
        echo!(
            "[DSchapter-hook] ready: all 12 chapters; original BL target={:#x}",
            policy::call_target()
        );
    }
    if !env.framework_state.opengles.ds_chapter.armed {
        return;
    }

    let get_hud = GuestFunction::from_addr_and_thumb_flag(0x0003_0474, false);
    let hud: u32 = get_hud.call_from_host(env, ());
    let world = word(env, hud.saturating_add(4)).unwrap_or(0);
    let phase = word(env, hud.saturating_add(0xdb4)).unwrap_or(0);
    let eligible = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(hud.saturating_add(0xdd4)), 1)
        .and_then(|b| b.first())
        .copied()
        .unwrap_or(0);
    let hud_state = word(env, hud.saturating_add(0x234)).unwrap_or(u32::MAX);
    let input_state = word(env, hud.saturating_add(0x240)).unwrap_or(u32::MAX);
    let owner = if hud >= 0x1000 && world >= 0x1000 {
        (hud, world)
    } else {
        (0, 0)
    };
    let chapter = {
        let state = &mut env.framework_state.opengles.ds_chapter;
        if owner != state.owner {
            // A request just captured belongs to the newly bound world. An
            // older request must never bleed into a different world/popup.
            if !state.pending {
                state.chapter = None;
            }
            state.owner = owner;
            state.played = false;
        }
        if owner == (0, 0) || (phase == 0 && state.played) {
            state.chapter = None;
            state.played = false;
        }
        state.pending = false;
        if matches!(phase, 1..=3) {
            state.played = true;
        }
        state.chapter
    };
    let show = chapter.is_some()
        && owner != (0, 0)
        && policy::visible(phase, eligible, hud_state, input_state)
        && READY.load(Ordering::Acquire);
    // Hide across all active phases, including fade-out. On unsupported IDs,
    // missing view/font, reset, or hidden HUD, restore the exact original BL.
    let patched = suppress(env, show);
    let value = if show && patched {
        policy::snapshot(
            chapter.unwrap(),
            phase,
            env.framework_state.opengles.ds_chapter.serial,
        )
    } else {
        0
    };
    SNAPSHOT.store(value, Ordering::Release);
    let key = (chapter.unwrap_or(0), phase, value != 0);
    let state = &mut env.framework_state.opengles.ds_chapter;
    if state.last != Some(key) {
        echo!("[DSchapter-overlay] serial={} chapter={} phase={} visible={} hud={:#x} world={:#x} hud_state={} input_state={}",
            state.serial, key.0, phase, key.2, hud, world, hud_state, input_state);
        state.last = Some(key);
    }
}

pub(crate) fn handle_breakpoint(env: &mut Environment) -> bool {
    if !env.framework_state.opengles.ds_chapter.armed
        || env.cpu.regs()[crate::cpu::Cpu::PC] != policy::CALL_SITE
    {
        return false;
    }
    let number = wide(env, env.cpu.regs()[1]);
    let title = wide(env, env.cpu.regs()[2]);
    let chapter = number
        .as_deref()
        .zip(title.as_deref())
        .and_then(|(number, title)| policy::chapter_number(number, title));
    let state = &mut env.framework_state.opengles.ds_chapter;
    state.chapter = chapter;
    state.pending = true;
    state.played = false;
    state.serial = state.serial.wrapping_add(1);
    crate::ds_diagnostics::event("native_chapter",[chapter.unwrap_or(0) as u64,state.serial as u64,0,0]);
    echo!(
        "[DSchapter-event] serial={} number={:?} title={:?} chapter={:?}",
        state.serial,
        number,
        title,
        chapter
    );
    // Execute the intercepted BL's control-flow semantics. The original method
    // still receives the same arguments and retains all scheduling/side effects.
    env.cpu.regs_mut()[crate::cpu::Cpu::LR] = policy::CALL_SITE + 4;
    env.cpu.regs_mut()[crate::cpu::Cpu::PC] = policy::call_target();
    true
}

#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_ChapterOverlayView_nativeSnapshot(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> i64 {
    SNAPSHOT.load(Ordering::Acquire) as i64
}

#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_ChapterOverlayView_nativeSetReady(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    ready: u8,
) {
    READY.store(ready != 0, Ordering::Release);
    if ready == 0 {
        SNAPSHOT.store(0, Ordering::Release);
    }
}

pub(crate) fn host_ready(ready:bool) { READY.store(ready, Ordering::Release); }
pub(crate) fn host_snapshot()->u64 { SNAPSHOT.load(Ordering::Acquire) }

/// Read-only benchmark admission. The original code signature must match;
/// menus/loading without an active world or with modal input are not measured.
pub(crate) fn benchmark_scene_ready(env: &mut Environment, player: u32) -> bool {
    let site = word(env, policy::CALL_SITE);
    if site != Some(policy::CALL_ORIGINAL) && site != Some(policy::CALL_BREAKPOINT) { return false; }
    let get_hud = GuestFunction::from_addr_and_thumb_flag(0x0003_0474, false);
    let hud: u32 = get_hud.call_from_host(env, ());
    let world=word(env,hud.saturating_add(4)).unwrap_or(0);
    hud >= 0x1000 && world >= 0x1000 && player >= 0x1000
        && word(env,world.saturating_add(0xe0)) == Some(player)
        && word(env,player) == Some(0x00a7_a4f8)
        && word(env, hud.saturating_add(0x234)) == Some(0)
        && word(env, hud.saturating_add(0x240)) == Some(0)
}

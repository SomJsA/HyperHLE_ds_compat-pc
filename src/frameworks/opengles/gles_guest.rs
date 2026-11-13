/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! Wrapper functions exposing OpenGL ES to the guest.

use crate::abi::{CallFromHost, GuestFunction};
use crate::dyld::{export_c_func, export_c_func_aliased, FunctionExports};
use crate::frameworks::opengles::eagl::EAGLContextHostObject;
use crate::gles::{gles11_raw as gles11, GLES};
use crate::mem::{ConstPtr, ConstVoidPtr, GuestISize, GuestUSize, Mem, MutPtr, MutVoidPtr, Ptr};
use crate::objc::nil;
use crate::Environment;
use std::collections::HashMap;
use std::slice::from_raw_parts;
use std::sync::{Arc, Mutex, OnceLock};

#[path = "ds_input_region.rs"]
mod ds_input_region;
#[path = "ds_interaction_policy.rs"]
mod ds_interaction_policy;
#[path = "ds_qte.rs"]
mod ds_qte;
#[path = "ds_test_assist.rs"]
pub(crate) mod ds_test_assist;
#[path = "ds_player_shortcuts.rs"]
mod ds_player_shortcuts;
use touchHLE_gl_bindings::gles11::{
    ARRAY_BUFFER, ELEMENT_ARRAY_BUFFER, ELEMENT_ARRAY_BUFFER_BINDING, WRITE_ONLY_OES,
};

use crate::gles::gles11_raw::types::{
    GLbitfield, GLboolean, GLclampf, GLclampx, GLenum, GLfixed, GLfloat, GLint,
    GLintptr as HostGLintptr, GLsizei, GLsizeiptr as HostGLsizeiptr, GLubyte, GLuint, GLvoid,
};
type GuestGLsizeiptr = GuestISize;
type GuestGLintptr = GuestISize;

const SUPPORTED_COMPRESSED_TEXTURE_FORMATS: &[GLenum] = &[
    gles11::COMPRESSED_RGBA_PVRTC_2BPPV1_IMG,
    gles11::COMPRESSED_RGBA_PVRTC_4BPPV1_IMG,
    gles11::COMPRESSED_RGB_PVRTC_2BPPV1_IMG,
    gles11::COMPRESSED_RGB_PVRTC_4BPPV1_IMG,
    gles11::PALETTE4_R5_G6_B5_OES,
    gles11::PALETTE4_RGB5_A1_OES,
    gles11::PALETTE4_RGB8_OES,
    gles11::PALETTE4_RGBA4_OES,
    gles11::PALETTE4_RGBA8_OES,
    gles11::PALETTE8_R5_G6_B5_OES,
    gles11::PALETTE8_RGB5_A1_OES,
    gles11::PALETTE8_RGB8_OES,
    gles11::PALETTE8_RGBA4_OES,
    gles11::PALETTE8_RGBA8_OES,
];

fn trace_potatogold_render() -> bool {
    std::env::var_os("TOUCHHLE_TRACE_POTATOGOLD_RENDER").is_some()
}

/// Set when the HUD state tracker (`TOUCHHLE_DS_TRACK_VTABLE`) sees a tracked
/// byte change, so the frame dumper writes that frame even when it is not on
/// the dump schedule. This is what makes the QTE capture hands-free.
pub(crate) static FORCE_FRAME_DUMP: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Address of the object the HUD state tracker watches, filled in by the
/// vtable scan (`TOUCHHLE_DS_TRACK_VTABLE`).
pub(crate) static TRACK_OBJ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Every *further* object with the tracked vtable, when
/// `TOUCHHLE_DS_TRACK_ALL=1` is set. One level can hold several instances of a
/// class (four `GameObjectDoor`s in chapter 12, measured 2026-10-11) and the
/// interesting one is not necessarily the first address the memory sweep hits.
pub(crate) fn track_all() -> &'static std::sync::Mutex<Vec<u32>> {
    static V: std::sync::OnceLock<std::sync::Mutex<Vec<u32>>> = std::sync::OnceLock::new();
    V.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Objects of the class named by `TOUCHHLE_DS_POKE_VTABLE`, found by the same
/// memory sweep as the tracker. Used by the OR-poke below, which has to poke a
/// class *other* than the tracked one (e.g. write an "interaction requested"
/// bit into the player while the tracker watches a door).
fn poke_objs() -> &'static std::sync::Mutex<Vec<u32>> {
    static V: std::sync::OnceLock<std::sync::Mutex<Vec<u32>>> = std::sync::OnceLock::new();
    V.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// One-shot latch for `TOUCHHLE_DS_CHASE_AT_FRAME` (re-run the chase dump at a
/// later frame than the scan, e.g. once the player stands at the door).
static CHASE_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `InputForwarderTapToAction` objects collected by the vtable scan
/// (`TOUCHHLE_DS_EKEY_FWD_VTABLE`), so the E key can find the region rectangle
/// the engine itself would accept a tap in (DECOMPILE.md D-023/D-027/D-029).
fn ekey_fwds() -> &'static std::sync::Mutex<Vec<u32>> {
    static V: std::sync::OnceLock<std::sync::Mutex<Vec<u32>>> = std::sync::OnceLock::new();
    V.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Refresh the scene/player action-forwarder cache.
///
/// The list is not static for the lifetime of the app: changing levels and
/// spawning new interactive objects creates new forwarders while old pointers
/// become invalid. Refreshing only when the list is empty kept the first level's
/// objects forever, so E could open an early door but miss a pickup spawned
/// later. A forced refresh replaces the cache with the current vtable matches.
fn ds_refresh_action_forwarders(env: &mut Environment, min_frame_gap: u64) -> bool {
    let _stutter_span=crate::ds_stutter_probe::Span::new(6,min_frame_gap);
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed);
    let last = LAST.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < min_frame_gap {
        return false;
    }
    LAST.store(now.max(1), Ordering::Relaxed);

    let objects = ds_sweep_vptr(env, 0x00a7d1e8);
    if objects.is_empty() {
        return false;
    }
    if let Ok(mut cached) = ekey_fwds().lock() {
        *cached = objects;
        return true;
    }
    false
}

/// Every action region the engine currently offers, read from its own data:
/// for each collected `InputForwarderTapToAction`, if its `InputRegionList` is
/// enabled (flag at list+8), walk list+0xc -> array -> region and return
/// `(action_code, owner_object, x, y, w, h)`.
/// `action_code` is the value the forwarder puts in the event it sends
/// (0x10 = interact, measured 2026-10-11); the rect is in the 480x320 UI space
/// the tap coordinates use (DECOMPILE.md D-023..D-029).
/// Dump the *presented* frame when the user pressed `M` / `F7`.
///
/// Why this exists: the dump code in `glClear` only runs on frames where the
/// guest clears the colour buffer (`if mask & COLOR_BUFFER_BIT != 0`). Measured
/// 2026-10-17 at `--scale-hack=4` (1920x1280): the level rendered without colour
/// clears, so `[DSmark]` was printed twice and **no file was ever written** -
/// a user pressing F7 to report a bug at high resolution would get nothing.
/// `-[EAGLContext presentRenderbuffer:]` calls this at the present boundary,
/// where the drawable is guaranteed to be complete, so a marked frame is always
/// captured regardless of how the guest renders.
pub(crate) fn ds_trace_present_frame(env: &mut Environment) {
    if std::env::var_os("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_DARK").is_none()
        && std::env::var_os("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_BRIGHT").is_none()
        && std::env::var_os("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_FRAME_DARK").is_none()
    {
        return;
    }
    let tid = env.current_thread;
    let ctx = {
        let state = &mut env.framework_state.opengles;
        (*state.current_ctx_for_thread(tid)).or(*state.current_ctx_for_thread(0))
    };
    let resolve = ctx.and_then(|ctx| env.objc.borrow::<EAGLContextHostObject>(ctx).drawable_fbo);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        let mut previous = 0;
        gles.GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut previous);
        if let Some(fbo) = resolve {
            gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, fbo);
        }
        gles.diagnostic_frame_ready();
        gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, previous as GLuint);
    });
}

pub(crate) fn ds_dump_presented_frame(env: &mut Environment, marked: bool) {
    let dir = match std::env::var("TOUCHHLE_DS_DUMP_FRAMES") {
        Ok(d) => d,
        Err(_) => {
            log_once!(
                "[DSmark] TOUCHHLE_DS_DUMP_FRAMES is not set; the marked frame was not written"
            );
            return;
        }
    };
    let rotate: i32 = std::env::var("TOUCHHLE_DS_DUMP_FRAME_ROTATE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let fcur = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    static MARKSEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static PRESENTSEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = if marked {
        MARKSEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
    } else {
        PRESENTSEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
    };
    let scale: i32 = if marked {
        1
    } else {
        std::env::var("TOUCHHLE_DS_DUMP_FRAME_SCALE")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1)
            .clamp(1, 8)
    };
    let resolve_fbo = {
        let tid = env.current_thread;
        let ctx = {
            let state = &mut env.framework_state.opengles;
            (*state.current_ctx_for_thread(tid)).or(*state.current_ctx_for_thread(0))
        };
        ctx.map(|ctx| env.objc.borrow::<EAGLContextHostObject>(ctx).drawable_fbo)
            .flatten()
    };
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        let mut previous_fbo: GLint = 0;
        gles.GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut previous_fbo);
        if let Some(resolve_fbo) = resolve_fbo {
            // The resolve FBO contains the completed single-sample image.
            // Reading the multisample source FBO produces a black/undefined
            // result on desktop GL.
            gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, resolve_fbo);
        }
        let mut vp = [0i32; 4];
        gles.GetIntegerv(gles11::VIEWPORT, vp.as_mut_ptr());
        let (w, h) = (vp[2], vp[3]);
        if w <= 0 || h <= 0 || w > 8192 || h > 8192 {
            if let Some(resolve_fbo) = resolve_fbo {
                if previous_fbo as GLuint != resolve_fbo {
                    gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, previous_fbo as GLuint);
                }
            }
            return;
        }
        let mut buf = vec![0u8; (w * h * 4) as usize];
        gles.ReadPixels(
            0,
            0,
            w,
            h,
            gles11::RGBA,
            gles11::UNSIGNED_BYTE,
            buf.as_mut_ptr() as *mut _,
        );
        let (ow, oh) = if rotate == 90 || rotate == 270 {
            (h, w)
        } else {
            (w, h)
        };
        let (sw, sh) = (ow / scale, oh / scale);
        let header = format!("P6\n{} {}\n255\n", sw, sh);
        let mut out = Vec::with_capacity((sw * sh * 3) as usize + 32);
        out.extend_from_slice(header.as_bytes());
        for oy in 0..sh {
            for ox in 0..sw {
                let (ox, oy) = (ox * scale, oy * scale);
                let (sx, sy) = match rotate {
                    90 => (oy, h - 1 - ox),
                    180 => (w - 1 - ox, h - 1 - oy),
                    270 => (w - 1 - oy, ox),
                    _ => (ox, oy),
                };
                // source is bottom-up
                let i = (((h - 1 - sy) * w + sx) * 4) as usize;
                out.push(buf[i]);
                out.push(buf[i + 1]);
                out.push(buf[i + 2]);
            }
        }
        ds_burn_label(
            &mut out,
            header.len(),
            sw as usize,
            sh as usize,
            &format!("{}{}", if marked { "M" } else { "P" }, seq),
        );
        let name = if marked {
            format!("mark_#{}_f{:06}.ppm", seq, fcur)
        } else {
            format!("present_{:05}_f{:06}.ppm", seq, fcur)
        };
        let _ = std::fs::create_dir_all(&dir);
        match std::fs::write(std::path::Path::new(&dir).join(&name), out) {
            Ok(()) => echo!(
                "[DSmark] {}{} f={} {}x{} -> {} (present-time dump)",
                if marked { "M" } else { "P" },
                seq,
                fcur,
                sw,
                sh,
                name
            ),
            Err(e) => echo!(
                "[DSmark] {}{} write failed: {}",
                if marked { "M" } else { "P" },
                seq,
                e
            ),
        }
        if let Some(resolve_fbo) = resolve_fbo {
            if previous_fbo as GLuint != resolve_fbo {
                gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, previous_fbo as GLuint);
            }
        }
    });
}

// OnUpdatePostCamera computes leaf enablement using the target interaction
// node's transform (including the kinesis offset), then projects its region.
// Do not re-run CanInteract with a player transform: that is a different input.
fn ds_live_region_bounds(env: &Environment, region: u32) -> Option<(i32, i32, i32, i32)> {
    if region == 0 || env.mem.read(ConstPtr::<u8>::from_bits(region + 8)) == 0 {
        return None;
    }
    ds_input_region::visible_bounds(|offset| {
        env.mem.read(ConstPtr::<u32>::from_bits(region + offset))
    })
}

pub(crate) fn ds_live_regions(env: &mut Environment) -> Vec<(u32, u32, i32, i32, i32, i32)> {
    let fwds: Vec<u32> = match ekey_fwds().lock() {
        Ok(v) => v.clone(),
        Err(_) => Vec::new(),
    };
    let mut out = Vec::new();
    for f in fwds {
        let vp: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(f));
        if vp != 0x00a7d1e8 {
            continue;
        }
        let code: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(f + 0x1c));
        let owner: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(f + 0x8));
        let list: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(f + 0x10));
        if list == 0 {
            continue;
        }
        let on: u8 = env
            .mem
            .read(crate::mem::ConstPtr::<u8>::from_bits(list + 8));
        if on == 0 {
            continue;
        }
        let begin: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(list + 0xc));
        let end: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(list + 0x10));
        if begin == 0 || end <= begin || end - begin > 4096 {
            continue;
        }
        for i in 0..(((end - begin) / 8) as usize).min(8) {
            let r: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(
                begin + (i as u32) * 8,
            ));
            let Some((x, y, w, h)) = ds_live_region_bounds(env, r) else {
                continue;
            };
            out.push((code, owner, x, y, w, h));
        }
    }
    out
}

/// Signature of the last logged live-region set, so `TOUCHHLE_DS_LOG_LIVE_REGIONS`
/// only prints when what is on offer actually changes.
static LIVE_SIG: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// Run one of the player's own action buttons **by code**, through the engine's
/// own `InputForwarderTapToAction::SendAction` (ARM `0x000e55cc`).
///
/// Why: `GameObjectPlayable::OnEvent` dispatches an `0x3f0` action event with a
/// 9-way jump table keyed on `code - 6` (see DECOMPILE.md D-034), and a real tap
/// on a HUD button only reaches it through that forwarder. Calling `SendAction`
/// on the right forwarder is therefore *exactly* the path the game itself takes
/// (including the forwarder's own enable flag at `+0x4` and its dispatch target
/// at `+0x8`), so we neither guess screen points nor poke game state directly.
///
/// `SendAction` re-checks `this+0x8 != null` and `this+0x4 != 0` and does
/// nothing when the button is currently disabled; we prefer the forwarder whose
/// dispatch target is the player object (`GameObjectPlayable`), and return
/// `None` when no enabled forwarder carries `code`.
// The before/after gauge snapshot executes several native getters. Keep it
// explicit: ordinary input dispatch must not perform diagnostic guest calls.
fn ds_action_gauges_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("TOUCHHLE_DS_ACTION_GAUGES").as_deref() == Ok("1"))
}

pub(crate) fn ds_send_action(env: &mut Environment, code: u32) -> Option<u32> {
    let objs = ds_sweep_vptr(env, 0x00a7d1e8);
    let mut pick: Option<u32> = None;
    for a in objs {
        let vp: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(a));
        if vp == 0 {
            continue;
        }
        let c: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(a + 0x1c));
        if c != code {
            continue;
        }
        let on: u8 = env.mem.read(crate::mem::ConstPtr::<u8>::from_bits(a + 4));
        let target: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(a + 8));
        if on == 0 || target == 0 {
            continue;
        }
        let tv: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(target));
        if tv == 0x00a7a4f8 {
            // GameObjectPlayable: this is the player's own button.
            pick = Some(a);
            break;
        }
        if pick.is_none() && code != 0x06 {
            pick = Some(a);
        }
    }
    ds_dispatch_action_forwarder(env, pick?, code)
}

fn ds_dispatch_action_forwarder(env: &mut Environment, a: u32, code: u32) -> Option<u32> {
    let _stutter_span = crate::ds_stutter_probe::Span::new(15, code as u64);
    let tgt = target_of(env, a);
    if ds_action_gauges_enabled() {
        ds_log_player_gauges(env, tgt, &format!("before code={code:#x}"));
    }
    let send = crate::abi::GuestFunction::from_addr_and_thumb_flag(0x000e55cc, false);
    let _: () = send.call_from_host(env, (a,));
    if ds_action_gauges_enabled() {
        ds_log_player_gauges(env, tgt, &format!("after  code={code:#x}"));
    }
    Some(a)
}

// Match the original sweep's live-range eligibility, even when old type
// bytes remain readable after free. Exact heap allocations use a cheap lookup;
// embedded objects conservatively check the same used ranges as the sweep.
fn ds_action_range_live(env: &Environment, address: u32, bytes: u32) -> bool {
    if address == 0 || bytes == 0 { return false; }
    let Some(end) = address.checked_add(bytes) else { return false; };
    if env.mem.malloc_size(crate::mem::ConstVoidPtr::from_bits(address)) >= bytes { return true; }
    env.mem.iter_used_ranges().any(|(base, size)| base <= address && base.checked_add(size).is_some_and(|limit| end <= limit))
}

fn ds_read_player_action(env: &Environment, address: u32) -> Option<crate::ds_player_action_cache::Snapshot> {
    if !ds_action_range_live(env, address, 0x20) { return None; }
    let read = |offset| ds_read_aim_word(env, address.checked_add(offset)?);
    let owner = read(8)?;
    if !ds_action_range_live(env, owner, 4) { return None; }
    let enabled = *env.mem.get_bytes_fallible(crate::mem::ConstVoidPtr::from_bits(address.checked_add(4)?), 1)?.first()?;
    Some(crate::ds_player_action_cache::Snapshot {
        vptr: read(0)?, code: read(0x1c)?, enabled, owner,
        owner_vptr: ds_read_aim_word(env, owner)?,
    })
}

// Only the native PC raise/fire path opts in. Generic E/world action lookup
// keeps the original scene sweep. The original sender rechecks native gates.
fn ds_send_player_fire_action(env: &mut Environment, player: u32) -> Option<u32> {
    let _stutter_span = crate::ds_stutter_probe::Span::new(16, 6);
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let enabled = *ENABLED.get_or_init(|| std::env::var("TOUCHHLE_DS_PLAYER_ACTION_CACHE").as_deref() != Ok("0"));
    if !enabled { return ds_send_action(env, 6); }
    let candidate = env.framework_state.opengles.ds_player_action_cache.candidate(player);
    if let Some(address) = candidate {
        if ds_read_player_action(env, address).is_some_and(|snapshot| snapshot.enabled_for(player, 6)) {
            if crate::ds_stutter_probe::enabled() { crate::ds_diagnostics::event("probe_player_action_cache", [1, crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed), 0, 0]); }
            return ds_dispatch_action_forwarder(env, address, 6);
        }
        env.framework_state.opengles.ds_player_action_cache.clear();
    }
    if crate::ds_stutter_probe::enabled() { crate::ds_diagnostics::event("probe_player_action_cache", [0, crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed), 0, 0]); }
    let address = ds_send_action(env, 6)?;
    if ds_read_player_action(env, address).is_some_and(|snapshot| snapshot.belongs_to(player, 6)) {
        env.framework_state.opengles.ds_player_action_cache.remember(player, address);
    }
    Some(address)
}

/// The dispatch target (`+0x8`) of an action forwarder - for the player's own
/// buttons this is the `GameObjectPlayable` object.
fn target_of(env: &mut Environment, fwd: u32) -> u32 {
    env.mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(fwd + 8))
}

/// Press one of the player's own action buttons, `code`, using **two levels**:
///
/// 1. if the game is currently offering a tappable region for that button, tap
///    its centre - this is exactly the data path the E key uses (the region the
///    engine itself would hit-test against);
/// 2. otherwise drive the button through `SendAction` (`ds_send_action`), which
///    is what happens when the button is not on screen.
///
/// Returns a one-line description for the log.
fn ds_press_action(env: &mut Environment, code: u32) -> String {
    if ekey_fwds().lock().map(|v| v.is_empty()).unwrap_or(true) {
        ds_refresh_action_forwarders(env, 0);
    }
    if let Some((c, o, x, y, w, h)) = ds_live_regions(env)
        .into_iter()
        .find(|(c, _, _, _, w, h)| *c == code && *w > 0 && *h > 0)
    {
        let px = x as f64 + w as f64 / 2.0;
        let py = y as f64 + h as f64 / 2.0;
        env.window_mut()
            .ds_queue_action(format!("tap {} {}", px, py));
        return format!(
            "live region code={c:#x} obj={o:#010x} rect=({x},{y},{w},{h}) -> tap ({px:.0},{py:.0})"
        );
    }
    match ds_send_action(env, code) {
        Some(a) => format!("engine action code={code:#x} -> forwarder {a:#010x} (SendAction)"),
        None => format!("code={code:#x}: no live region and no enabled forwarder"),
    }
}

/// Enter telekinesis on the nearest engine-approved telekinetic object.
///
/// The player action (`0x06`) only transitions into the generic aiming state;
/// it does not choose a scene target. The original touch UI gets the target
/// from the visible object prompt. For keyboard/controller input, select the
/// nearest object whose own virtual `IsTelekinesisable()` is true, then use the
/// game's `GameObjectPlayable::StartTelekinesis(GameObjectInteractive*)`.
fn ds_start_nearest_kinesis_target(env: &mut Environment) -> Option<(u32, f32)> {
    let (_hud, player) = ds_current_hud_player(env)?;
    if ekey_fwds().lock().map(|v| v.is_empty()).unwrap_or(true) {
        ds_refresh_action_forwarders(env, 0);
    }
    let can_telekinesis = GuestFunction::from_addr_and_thumb_flag(0x0003b058, false);
    let can: u32 = can_telekinesis.call_from_host(env, (player,));
    if can == 0 {
        return None;
    }
    let player_body: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x4c));
    if player_body == 0 {
        return None;
    }
    let player_pos = player_body + 0x40;
    let px = read_guest_f32(env, player_pos);
    let py = read_guest_f32(env, player_pos + 4);
    let pz = read_guest_f32(env, player_pos + 8);
    let forwarders: Vec<u32> = match ekey_fwds().lock() {
        Ok(v) => v.clone(),
        Err(_) => return None,
    };
    let mut candidates: Vec<(f32, u32, u32)> = Vec::new();
    for fwd in forwarders {
        let owner: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(fwd + 8));
        if owner == 0 || owner == player {
            continue;
        }
        let vptr: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(owner));
        if vptr < 0x1000 || vptr >= 0x0100_0000 {
            continue;
        }
        // Stored vptr is `__ZTV+8`; IsTelekinesisable is slot 24
        // (symbol offset +0x60, stored-vptr offset +0x58).
        let is_tk_fn: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(vptr + 0x58));
        if is_tk_fn < 0x1000 || is_tk_fn >= 0x0100_0000 {
            continue;
        }
        let is_tk = GuestFunction::from_addr_and_thumb_flag(is_tk_fn, false);
        let accepted: u32 = is_tk.call_from_host(env, (owner,));
        if accepted == 0 {
            continue;
        }
        let rigid_body: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(owner + 0x4c));
        if rigid_body == 0 {
            continue;
        }
        let pos = rigid_body + 0x40;
        let dx = read_guest_f32(env, pos) - px;
        let dy = read_guest_f32(env, pos + 4) - py;
        let dz = read_guest_f32(env, pos + 8) - pz;
        candidates.push((dx * dx + dy * dy + dz * dz, owner, is_tk_fn));
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (distance, target, is_tk_fn) = *candidates.first()?;
    let start = GuestFunction::from_addr_and_thumb_flag(0x0004ae88, false);
    let _: () = start.call_from_host(env, (player, target));
    echo!(
        "[DSf] direct kinesis target={:#010x} is_tk_fn={:#010x} distance2={:.4}",
        target,
        is_tk_fn,
        distance
    );
    Some((target, distance))
}

/// Press a **scene object's** own action button (code `0x10` = interact, i.e.
/// the blue ring) by calling `InputForwarderTapToAction::SendAction` on that
/// object's forwarder.
///
/// Why this exists (2026-10-16, user report): the E key used to inject a *screen
/// tap* at the region centre. A tap is a plain screen event, so whenever the
/// game did not accept it as an interaction it was read as look/fire instead -
/// the user saw "按 E 会开枪或者变成转身". `SendAction` goes straight to the
/// object's own event (`0x3f0` -> `OnInteract`), so nothing else can pick it up.
///
/// Returns `(forwarder, object, rect)` for the region it used.
fn ds_press_scene_action(
    env: &mut Environment,
    code: u32,
    telekinesis_only: bool,
) -> Option<(u32, u32, (i32, i32, i32, i32))> {
    if ekey_fwds().lock().map(|v| v.is_empty()).unwrap_or(true) {
        ds_refresh_action_forwarders(env, 0);
    }
    let fwds: Vec<u32> = match ekey_fwds().lock() {
        Ok(v) => v.clone(),
        Err(_) => Vec::new(),
    };
    let mut best: Option<(u32, u32, (i32, i32, i32, i32))> = None;
    let mut best_rank = (u8::MAX, i64::MAX);
    for f in fwds {
        // Cached addresses can be freed/reused during level transitions.
        let vptr: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(f));
        if vptr != 0x00a7d1e8 {
            continue;
        }
        let c: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(f + 0x1c));
        if c != code {
            continue;
        }
        let on: u8 = env.mem.read(crate::mem::ConstPtr::<u8>::from_bits(f + 4));
        let obj: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(f + 8));
        if on == 0 || obj == 0 {
            continue;
        }
        // Disabled objects can retain old leaf geometry while their HUD fades.
        if env.mem.read(ConstPtr::<u8>::from_bits(obj + 0x164)) == 0 {
            continue;
        }
        let owner_vptr: u32 = env.mem.read(ConstPtr::from_bits(obj));
        let pickup = owner_vptr == ds_interaction_policy::PICKUP_VPTR;
        let telekinetic = {
            let owner_vptr: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(obj));
            if !(0x1000..0x0100_0000).contains(&owner_vptr) {
                continue;
            }
            // GameObjectInteractive::OnInteract itself checks this virtual
            // before choosing StartTelekinesis rather than ordinary OnOpen.
            let is_tk_addr: u32 = env
                .mem
                .read(crate::mem::ConstPtr::<u32>::from_bits(owner_vptr + 0x58));
            if !(0x1000..0x0100_0000).contains(&is_tk_addr) {
                continue;
            }
            let is_tk = GuestFunction::from_addr_with_thumb_bit(is_tk_addr);
            let accepted: u32 = is_tk.call_from_host(env, (obj,));
            accepted != 0
        };
        let pickup_key = (
            f,
            obj,
            if pickup {
                env.mem.read(ConstPtr::<u32>::from_bits(obj + 0x1dc))
            } else {
                0
            },
        );
        let blocked = !telekinesis_only
            && pickup
            && ds_pickup_cooldowns()
                .lock()
                .map(|mut cooldowns| cooldowns.blocked(pickup_key, std::time::Instant::now()))
                .unwrap_or(false);
        if ds_interaction_policy::rank(telekinesis_only, telekinetic, pickup, blocked, 0).is_none()
        {
            continue;
        }
        let list: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(f + 0x10));
        if list == 0 {
            continue;
        }
        let list_on: u8 = env
            .mem
            .read(crate::mem::ConstPtr::<u8>::from_bits(list + 8));
        if list_on == 0 {
            continue;
        }
        let begin: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(list + 0xc));
        let end: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(list + 0x10));
        if begin == 0 || end <= begin || end - begin > 4096 {
            continue;
        }
        for i in 0..(((end - begin) / 8) as usize).min(8) {
            let r: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(
                begin + (i as u32) * 8,
            ));
            let Some((x, y, w, h)) = ds_live_region_bounds(env, r) else {
                continue;
            };
            let dx = (x + w / 2 - 240) as i64;
            let dy = (y + h / 2 - 160) as i64;
            let d = dx * dx + dy * dy;
            let rank =
                ds_interaction_policy::rank(telekinesis_only, telekinetic, pickup, blocked, d)
                    .unwrap();
            if rank < best_rank {
                best_rank = rank;
                best = Some((f, obj, (x, y, w, h)));
            }
        }
    }
    let (f, obj, rect) = best?;
    let pickup = !telekinesis_only
        && env.mem.read(ConstPtr::<u32>::from_bits(obj)) == ds_interaction_policy::PICKUP_VPTR;
    let before = if pickup {
        Some((
            env.mem.read(ConstPtr::<u32>::from_bits(obj + 0x1dc)),
            env.mem.read(ConstPtr::<u32>::from_bits(obj + 0x1e0)),
        ))
    } else {
        None
    };
    let send = crate::abi::GuestFunction::from_addr_and_thumb_flag(0x000e55cc, false);
    let _: () = send.call_from_host(env, (f,));
    if let Some((item_type, quantity)) = before {
        // Only examine fields if the object still has the same runtime type.
        if env.mem.read(ConstPtr::<u32>::from_bits(obj)) == ds_interaction_policy::PICKUP_VPTR {
            let after_quantity = env.mem.read(ConstPtr::<u32>::from_bits(obj + 0x1e0));
            let after_state = env.mem.read(ConstPtr::<u32>::from_bits(obj + 0x1d0));
            if ds_interaction_policy::pickup_failed(quantity, after_quantity, after_state) {
                if let Ok(mut cooldowns) = ds_pickup_cooldowns().lock() {
                    cooldowns.block((f, obj, item_type), std::time::Instant::now());
                }
                echo!(
                    "[DSe] pickup unchanged obj={:#010x} quantity={}; skip this item for 3 seconds",
                    obj,
                    quantity
                );
            }
        }
    }
    Some((f, obj, rect))
}

fn ds_pickup_cooldowns() -> &'static std::sync::Mutex<ds_interaction_policy::PickupCooldowns> {
    static COOLDOWNS: std::sync::OnceLock<
        std::sync::Mutex<ds_interaction_policy::PickupCooldowns>,
    > = std::sync::OnceLock::new();
    COOLDOWNS.get_or_init(Default::default)
}

fn ds_press_kinesis_action(env: &mut Environment) -> Option<(u32, u32, (i32, i32, i32, i32))> {
    let hit = ds_press_scene_action(env, 0x10, true);
    if hit.is_some() || !ds_refresh_action_forwarders(env, 30) {
        return hit;
    }
    echo!("[DSf] refreshed action-forwarder cache after target miss");
    ds_press_scene_action(env, 0x10, true)
}

/// Mirror the swarm handler's candidate projection, then let the original
/// OnGrappleSwarmEvent perform hit testing and detach ONE creature per press.
/// This uses a real PointerSingleTapEvent (RTTI checked by the game).
fn ds_swarm_target(env: &mut Environment, player: u32) -> Option<(u32, i32, i32)> {
    let world: u32 = env.mem.read(ConstPtr::from_bits(player + 0x10));
    if world == 0 {
        return None;
    }
    let camera: u32 = env.mem.read(ConstPtr::from_bits(world + 0x1c));
    if camera == 0 {
        return None;
    }
    let projected = env.mem.alloc(12);
    let mut point = None;
    for slot in 0..8u32 {
        let obj: u32 = env.mem.read(ConstPtr::from_bits(player + 0x3f4 + slot * 8));
        if obj == 0 {
            continue;
        }
        let vptr: u32 = env.mem.read(ConstPtr::from_bits(obj));
        if !(0x1000..0x0100_0000).contains(&vptr) {
            continue;
        }
        let alive: u32 = env.mem.read(ConstPtr::from_bits(vptr + 0x10c));
        let position: u32 = env.mem.read(ConstPtr::from_bits(vptr + 0xc0));
        if !(0x1000..0x0100_0000).contains(&alive) || !(0x1000..0x0100_0000).contains(&position) {
            continue;
        }
        let alive: u32 = GuestFunction::from_addr_with_thumb_bit(alive).call_from_host(env, (obj,));
        let flags: u8 = env.mem.read(ConstPtr::from_bits(obj + 0x40));
        if alive == 0 || flags & 1 != 0 {
            continue;
        }
        let position: u32 =
            GuestFunction::from_addr_with_thumb_bit(position).call_from_host(env, (obj,));
        if position == 0 {
            continue;
        }
        // ARM aggregate return: output pointer precedes this and position.
        let _: () = GuestFunction::from_addr_and_thumb_flag(0x00078da4, false)
            .call_from_host(env, (projected.to_bits(), camera, position));
        let x = read_guest_f32(env, projected.to_bits());
        let y = read_guest_f32(env, projected.to_bits() + 4);
        let z = read_guest_f32(env, projected.to_bits() + 8);
        if x.is_finite()
            && y.is_finite()
            && z >= 0.0
            && (0.0..480.0).contains(&x)
            && (0.0..320.0).contains(&y)
        {
            point = Some((slot, x.round() as i32, y.round() as i32));
            break;
        }
    }
    env.mem.free(projected);
    point
}

fn ds_press_swarm_target(env: &mut Environment, player: u32) -> String {
    let Some((slot, x, y)) = ds_swarm_target(env, player) else {
        return "[DSqte] swarm: no visible attached target".into();
    };
    let event = env.mem.alloc(20);
    for (i, value) in [0x00a63058u32, 1001, x as u32, y as u32, 0]
        .into_iter()
        .enumerate()
    {
        env.mem.write(
            crate::mem::MutPtr::<u32>::from_bits(event.to_bits() + i as u32 * 4),
            value,
        );
    }
    let accepted: u32 = GuestFunction::from_addr_and_thumb_flag(0x0003fc60, false)
        .call_from_host(env, (player, event.to_bits()));
    env.mem.free(event);
    format!("[DSqte] swarm tap: slot={slot} point=({x},{y}) accepted={accepted}")
}

fn read_guest_f32(env: &Environment, addr: u32) -> f32 {
    f32::from_bits(env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(addr)))
}

/// Parse a comma/space separated list of action codes: `"0x06, 0x10"`.
fn ds_parse_code_list(s: &str) -> Vec<u32> {
    s.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .filter_map(ds_parse_u32)
        .collect()
}

/// The action codes the **E** key tries, in order. Defaults to the blue-circle
/// interact code alone; `TOUCHHLE_DS_CODES_E` / `TOUCHHLE_DS_CODE_E` override it
/// (handy when a new object turns out to use a different button).
fn ekey_codes() -> Vec<u32> {
    if let Ok(v) = std::env::var("TOUCHHLE_DS_CODES_E") {
        let l = ds_parse_code_list(&v);
        if !l.is_empty() {
            return l;
        }
    }
    if let Ok(v) = std::env::var("TOUCHHLE_DS_CODE_E") {
        if let Some(c) = ds_parse_u32(&v) {
            return vec![c];
        }
    }
    vec![0x10]
}

/// Like [`ds_press_scene_action`] but walks a list of codes in priority order and
/// reports which code won. Also returns the codes that were live at all, so the
/// caller can log the whole picture when nothing matched.
fn ds_press_scene_action_any(
    env: &mut Environment,
    codes: &[u32],
) -> (Option<(u32, u32, u32, (i32, i32, i32, i32))>, Vec<u32>) {
    let live: Vec<u32> = {
        let mut v: Vec<u32> = ds_live_regions(env)
            .into_iter()
            .map(|(c, _, _, _, _, _)| c)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    for &code in codes {
        if let Some((f, obj, rect)) = ds_press_scene_action(env, code, false) {
            return (Some((code, f, obj, rect)), live);
        }
    }
    (None, live)
}

/// Execute the combat/QTE gesture currently exposed by the HUD/player state.
///
/// Decompiled state sources:
/// * `GameObjectPlayable+0x37c` (`GrappleState`):
///   `5` = tap ring (`RenderHUDTap`), `1`/`7` = directional swipe
///   (`RenderHUDSwipe`).
/// * `Hud+0x1cd` = stomp enabled, `Hud+0x1ce` = slash enabled,
///   `Hud+0x1cc` selects which arrow is currently shown.
///
/// Returning `Some` means the prompt was active and E consumed the press, so
/// the ordinary blue-circle interaction path must not run as well.
fn ds_try_current_prompt_gesture(env: &mut Environment) -> Option<String> {
    let _qte_input_span=crate::ds_stutter_probe::Span::new(14,0);
    let (hud, player) = ds_current_hud_player(env)?;
    let player_state: u32 = env.mem.read(ConstPtr::from_bits(player + 0x208));
    // OnEvent (0x54a50) routes state 10 to the separate swarm handler.
    if player_state == 10 {
        return Some(ds_press_swarm_target(env, player));
    }
    let grapple_state: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x37c));
    match if player_state == 9 {
        grapple_state
    } else {
        u32::MAX
    } {
        5 => {
            // Mirror GameObjectPlayable::OnGrappleEvent's SingleTap branch:
            // ask the grapple target for its current tap slot, then publish the
            // corresponding internal event value directly to +0x3ec.
            let grapple: u32 = env
                .mem
                .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x384));
            if grapple == 0 {
                return Some("[DSqte] tap state, but grapple target is null".to_string());
            }
            let vptr: u32 = env
                .mem
                .read(crate::mem::ConstPtr::<u32>::from_bits(grapple));
            if vptr == 0 {
                return Some("[DSqte] tap state, but grapple vtable is null".to_string());
            }
            let get_slot: u32 = env
                .mem
                .read(crate::mem::ConstPtr::<u32>::from_bits(vptr + 0x34));
            if get_slot == 0 {
                return Some("[DSqte] tap state, but tap-slot method is null".to_string());
            }
            let slot_method = GuestFunction::from_addr_and_thumb_flag(get_slot, get_slot & 1 != 0);
            let selected: u32 = slot_method.call_from_host(env, (grapple,));
            let globals = GuestFunction::from_addr_and_thumb_flag(0x0007e448, false);
            let base: u32 = globals.call_from_host(env, ());
            let event_addr = base + if selected == 0 { 0xbb8 } else { 0xbbc };
            let event_value: u32 = env
                .mem
                .read(crate::mem::ConstPtr::<u32>::from_bits(event_addr));
            env.mem.write(
                crate::mem::MutPtr::<u32>::from_bits(player + 0x3ec),
                event_value,
            );
            Some(format!(
                "[DSqte] grapple tap via engine: slot={} event={:#010x}",
                selected, event_value
            ))
        }
        1 | 7 => {
            // The engine's OnGrappleGestureEvent accepts 12..78 degrees for
            // state 1 and -123..-57 degrees for state 7. Supply a safe point in
            // each interval, construct the guest GestureEvent, and invoke the
            // handler directly.
            // 0x40cd8 loads INTEGER coordinates and calls atan2f(dx, dy),
            // not atan2f(dy, dx). State 7 needs a LEFT swipe (-90 degrees).
            let words = ds_qte::grapple_swipe(grapple_state).unwrap();
            let (sx, sy, ex, ey) = (words[3], words[4], words[5], words[6]);
            let event = env.mem.alloc(0x1c);
            for (i, value) in words.into_iter().enumerate() {
                env.mem.write(
                    crate::mem::MutPtr::<i32>::from_bits(event.to_bits() + i as u32 * 4),
                    value,
                );
            }
            let call = GuestFunction::from_addr_and_thumb_flag(0x00040cb8, false);
            let _: () = call.call_from_host(env, (player, event.to_bits()));
            env.mem.free(event);
            let after: u32 = env.mem.read(ConstPtr::from_bits(player + 0x37c));
            Some(format!(
                "[DSqte] grapple swipe via engine: state={}->{} ({},{})->({},{})",
                grapple_state, after, sx, sy, ex, ey
            ))
        }
        _ => {
            let swipe_override: u8 = env
                .mem
                .read(crate::mem::ConstPtr::<u8>::from_bits(hud + 0x1c4));
            let stomp: u8 = env
                .mem
                .read(crate::mem::ConstPtr::<u8>::from_bits(hud + 0x1cd));
            let slash: u8 = env
                .mem
                .read(crate::mem::ConstPtr::<u8>::from_bits(hud + 0x1ce));
            if swipe_override & 1 == 0 && (stomp != 0 || slash != 0) {
                let select_stomp: u8 = env
                    .mem
                    .read(crate::mem::ConstPtr::<u8>::from_bits(hud + 0x1cc));
                let (addr, label) = if select_stomp != 0 || slash == 0 {
                    (0x0004fc64, "stomp")
                } else {
                    (0x0004fa4c, "slash")
                };
                let call = GuestFunction::from_addr_and_thumb_flag(addr, false);
                let _: () = call.call_from_host(env, (player,));
                return Some(format!("[DSqte] {} prompt -> engine action", label));
            }
            None
        }
    }
}

fn ds_try_death_confirm(env: &mut Environment) -> bool {
    let Some((_hud, player)) = ds_current_hud_player(env) else {
        return false;
    };
    let state: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x208));
    if state != 26 {
        return false;
    }
    env.window_mut().ds_queue_action("tap 389 288".to_owned());
    echo!("[DSdeath] E/A confirms continue");
    true
}

fn ds_try_death_cancel(env: &mut Environment) -> bool {
    let Some((_hud, player)) = ds_current_hud_player(env) else {
        return false;
    };
    let state: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x208));
    if state != 26 {
        return false;
    }
    env.window_mut().ds_queue_action("tap 88 288".to_owned());
    echo!("[DSdeath] Q/B cancels death screen");
    true
}

/// Publish the on-screen QTE/action arrow for the PC host overlay. This never
/// changes the native gesture/action dispatch; it only supplies the position
/// where the overlay can replace the touch glyph with E/A keycaps.
fn ds_publish_qte_overlay(env: &mut Environment) {
    let _stutter_span=crate::ds_stutter_probe::Span::new(4,0);
    if cfg!(target_os = "android")
        || std::env::var_os("TOUCHHLE_DS_QTE_OVERLAY").is_none()
    {
        crate::ds_qte_overlay::publish(None);
        return;
    }
    let Some((hud, player)) = ds_current_hud_player(env) else {
        crate::ds_qte_overlay::publish(None);
        return;
    };
    let player_state: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x208));
    let grapple_state: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x37c));
    crate::ds_qte_geometry::observe_stage(env,player,player_state,grapple_state);
    let grapple_tap = player_state == 9 && grapple_state == 5;
    let grapple_arrow = player_state == 9 && matches!(grapple_state, 1 | 7);
    let swipe_override: u8 = env
        .mem
        .read(crate::mem::ConstPtr::<u8>::from_bits(hud + 0x1c4));
    let stomp: u8 = env
        .mem
        .read(crate::mem::ConstPtr::<u8>::from_bits(hud + 0x1cd));
    let slash: u8 = env
        .mem
        .read(crate::mem::ConstPtr::<u8>::from_bits(hud + 0x1ce));
    let combat_arrow = player_state != 9
        && swipe_override & 1 == 0
        && (stomp != 0 || slash != 0);
    if crate::ds_stutter_probe::sample_due() {
        let frame=crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
        if let Some(transform)=ds_read_aim_word(env,player+0x4c).filter(|p|*p!=0) {
            let bits:[u32;3]=std::array::from_fn(|i|ds_read_aim_word(env,transform+0x40+i as u32*4).unwrap_or(f32::NAN.to_bits()));
            crate::ds_diagnostics::event("probe_player_position_bits",[frame,bits[0] as u64,bits[1] as u64,bits[2] as u64]);
        }
        crate::ds_diagnostics::event("probe_native_hint_flags",[frame,player_state as u64,grapple_state as u64,((swipe_override as u64)<<16)|((stomp as u64)<<8)|slash as u64]);
    }
    if player_state == 26 {
        crate::ds_qte_overlay::publish(Some(crate::ds_qte_overlay::Prompt {
            x: 389.0,
            y: 288.0,
            width: 52.0,
            height: 42.0,
            kind: crate::ds_qte_overlay::PromptKind::Death,
            secondary: Some([88.0, 288.0, 52.0, 42.0]),
        }));
        return;
    }
    if player_state == 10 {
        let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
        let existing = crate::ds_qte_overlay::snapshot();
        if frame % 2 == 0
            || existing.is_none_or(|prompt| {
                prompt.kind != crate::ds_qte_overlay::PromptKind::Tap
            })
        {
            if let Some((_slot, x, y)) = ds_swarm_target(env, player) {
                crate::ds_qte_overlay::publish(Some(crate::ds_qte_overlay::Prompt {
                    x: x as f32,
                    y: y as f32,
                    width: 34.0,
                    height: 34.0,
                    kind: crate::ds_qte_overlay::PromptKind::Tap,
                    secondary: None,
                }));
            } else {
                crate::ds_qte_overlay::publish(None);
            }
        }
        return;
    }
    if grapple_tap || grapple_arrow {
        // QTEs use native HUD layout / draw parameters. A missing or unknown
        // geometry contract leaves the original glyph visible; it must never
        // restart a full-resolution framebuffer scan on every missing hint.
        let native = if grapple_tap {
            crate::ds_qte_geometry::tap(env, hud)
        } else {
            crate::ds_qte_geometry::swipe(env, hud)
        };
        crate::ds_qte_overlay::publish(native.map(|b| crate::ds_qte_overlay::Prompt {
            x:b.x,y:b.y,width:b.width,height:b.height,
            kind:if grapple_tap {crate::ds_qte_overlay::PromptKind::Tap}
                else {crate::ds_qte_overlay::PromptKind::Swipe},secondary:None,
        }));
        return;
    }
    if combat_arrow {
        let select_stomp: u8 = env
            .mem
            .read(crate::mem::ConstPtr::<u8>::from_bits(hud + 0x1cc));
        let action = if select_stomp != 0 || slash == 0 {
            crate::ds_qte_overlay::CombatAction::Stomp
        } else {
            crate::ds_qte_overlay::CombatAction::Slash
        };
        crate::ds_qte_overlay::publish(Some(crate::ds_qte_overlay::Prompt {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            kind: crate::ds_qte_overlay::PromptKind::Combat(action),
            secondary: None,
        }));
        return;
    }
    crate::ds_qte_overlay::publish(None);
}

/// RT and left click launch a held telekinesis object. Action 0x06 is the
/// held-object branch; cancel ordinary queued fire until release so the same
/// input is not consumed twice on the same frame.
fn ds_try_kinesis_launch(
    env: &mut Environment,
    request: &std::sync::atomic::AtomicBool,
    source: &str,
) -> bool {
    if !request.swap(
        false,
        std::sync::atomic::Ordering::Relaxed,
    ) {
        return false;
    }
    let Some((_hud, player)) = ds_current_hud_player(env) else {
        return false;
    };
    let state: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x208));
    if state != 0x12 {
        return false;
    }
    let how = ds_press_action(env, 0x06);
    crate::window::ds_fire_control::enqueue(crate::window::ds_fire_control::Input::Cancel);
    echo!("[DSf] {source} launches held object: {}", how);
    true
}

fn ds_hud_player_cache_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("TOUCHHLE_DS_HUD_PLAYER_CACHE").as_deref() == Ok("1")
    })
}

fn ds_hud_player_cache_report(hit: bool) {
    static TRACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*TRACE.get_or_init(|| {
        std::env::var("TOUCHHLE_DS_HUD_PLAYER_CACHE_TRACE").as_deref() == Ok("1")
    }) {
        return;
    }
    use std::sync::atomic::{AtomicU64, Ordering};
    static CALLBACKS: AtomicU64 = AtomicU64::new(0);
    static HITS: AtomicU64 = AtomicU64::new(0);
    let callback_delta = if hit { 0 } else { 1 };
    let hit_delta = if hit { 1 } else { 0 };
    let callbacks = CALLBACKS.fetch_add(callback_delta, Ordering::Relaxed) + callback_delta;
    let hits = HITS.fetch_add(hit_delta, Ordering::Relaxed) + hit_delta;
    let total = callbacks.saturating_add(hits);
    if total % 256 == 0 {
        echo!("[DSpcHudCache] callbacks={} hits={} total={}", callbacks, hits, total);
    }
}

fn ds_current_hud_player(env: &mut Environment) -> Option<(u32, u32)> {
    if crate::ds_frontend_controller::capture_menu() {
        if ds_hud_player_cache_enabled() {
            let state = &mut env.framework_state.opengles;
            state.ds_hud_player_cache_frame = crate::window::DS_FRAME_COUNTER
                .load(std::sync::atomic::Ordering::Relaxed);
            state.ds_hud_player_cache_thread = env.current_thread;
            state.ds_hud_player_cache_sampled = true;
            state.ds_hud_player_cache_value = None;
        }
        crate::ds_frontend_controller::set_hud_state(crate::ds_frontend_controller::UNKNOWN);
        return None;
    }
    let cache_enabled = ds_hud_player_cache_enabled();
    let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    if cache_enabled {
        let cached = {
            let state = &env.framework_state.opengles;
            (state.ds_hud_player_cache_sampled
                && state.ds_hud_player_cache_frame == frame
                && state.ds_hud_player_cache_thread == env.current_thread)
                .then_some(state.ds_hud_player_cache_value)
        };
        if let Some(cached) = cached {
            ds_hud_player_cache_report(true);
            let Some((hud, player)) = cached else {
                crate::ds_frontend_controller::set_hud_state(crate::ds_frontend_controller::UNKNOWN);
                return None;
            };
            let hud_state = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(hud + 0x234));
            crate::ds_frontend_controller::set_hud_state(hud_state);
            return Some((hud, player));
        }
    }
    if cache_enabled {
        ds_hud_player_cache_report(false);
    }
    let get_hud = GuestFunction::from_addr_and_thumb_flag(0x00030474, false);
    let hud: u32 = get_hud.call_from_host(env, ());
    let result = if hud == 0 {
        None
    } else {
        let world = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(hud + 4));
        let player = if world == 0 {
            0
        } else {
            env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(world + 0xe0))
        };
        let player_vt = if player == 0 {
            0
        } else {
            env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(player))
        };
        (player != 0 && player_vt == 0x00a7a4f8).then(|| {
            let hud_state = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(hud + 0x234));
            (hud, player, hud_state)
        })
    };
    if cache_enabled {
        let state = &mut env.framework_state.opengles;
        state.ds_hud_player_cache_frame = frame;
        state.ds_hud_player_cache_thread = env.current_thread;
        state.ds_hud_player_cache_sampled = true;
        state.ds_hud_player_cache_value = result.map(|(hud, player, _)| (hud, player));
    }
    let Some((hud, player, hud_state)) = result else {
        crate::ds_frontend_controller::set_hud_state(crate::ds_frontend_controller::UNKNOWN);
        return None;
    };
    crate::ds_frontend_controller::set_hud_state(hud_state);
    Some((hud, player))
}

// Native pause resume is itself a HUD command. The ordinary gameplay helper
// deliberately refuses to inspect a HUD while the frontend controller owns a
// modal, so use this narrow verified lookup for BTN_PLAY_PAUSE only.
fn ds_pause_hud_player(env: &mut Environment) -> Option<(u32, u32)> {
    let hud: u32 = GuestFunction::from_addr_and_thumb_flag(0x0003_0474, false).call_from_host(env, ());
    let world = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(hud + 4));
    if hud < 0x1000 || world < 0x1000 { return None; }
    let player = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(world + 0xe0));
    if player < 0x1000 || env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(player)) != 0x00a7_a4f8 { return None; }
    let state = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(hud + 0x234));
    crate::ds_frontend_controller::set_hud_state(state);
    Some((hud, player))
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) fn ds_current_hud_player_public(env: &mut Environment) -> Option<(u32, u32)> {
    ds_current_hud_player(env)
}

/// Match the acquisition portion of original CanStasis (ARM 0x3b080).
/// New games pre-create a WeaponStasis with one charge while available=0;
/// bypassing the hidden HUD button spent it and then flashed "no energy".
/// Never gate by chapter or remaining charge: NG+ owns stasis in chapter one,
/// and an acquired but empty weapon must keep its original failure feedback.
fn ds_stasis_acquired(env: &mut Environment) -> bool {
    if ds_read_aim_word(env, 0x0003_0474) != Some(0xe92d_4080)
        || ds_read_aim_word(env, 0x0003_b080) != Some(0xe590_130c) {
        return false;
    }
    let Some((hud, player)) = ds_current_hud_player(env) else { return false; };
    if ds_read_aim_word(env, hud + 0x234) != Some(0) { return false; }
    let weapon = ds_read_aim_word(env, player + 0x30c).unwrap_or(0);
    let acquired = weapon != 0
        && ds_read_aim_word(env, weapon) == Some(0x00a7_b2f8)
        && env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(weapon + 0x164), 1)
            .is_some_and(|value| value[0] != 0);
    echo!("[DSshortcut] stasis acquisition player={:#x} weapon={:#x} acquired={} stored_charges={}",
        player, weapon, acquired, if weapon != 0 { ds_read_aim_word(env, weapon + 0x168).unwrap_or(0) } else { 0 });
    acquired
}

fn ds_station_label(vt:u32)->Option<&'static str> {
    // These are object vptrs, not __ZTV starts. Itanium ABI stores
    // __ZTV<Class> + 8 in the object.
    match vt {
        0x00a7_b648=>Some("store"),
        0x00a7_b828=>Some("bench"),
        0x00a7_eab8=>Some("control"),
        _=>None,
    }
}

fn ds_station_actors(env:&Environment,world:u32)->Vec<(&'static str,u32,u32)> {
    let _stutter_span=crate::ds_stutter_probe::Span::new(5,0);
    static CACHE:std::sync::Mutex<(u32,Vec<(&'static str,u32,u32)>)>=std::sync::Mutex::new((0,Vec::new()));
    let mut cache=CACHE.lock().unwrap();
    if cache.0!=world || cache.1.is_empty() || cache.1.iter().all(|&(_,actor,vt)|ds_read_aim_word(env,actor)!=Some(vt)) {
        let mut actors=Vec::new();
        // Cache live actors, but never retain a negative loading-time result.
        // Unlike the old
        // per-word diagnostic sweep this reads checked chunks in one pass.
        for (base,size) in env.mem.iter_used_ranges() {
            for off in (0..size.saturating_sub(3)).step_by(4096) {
                let count=4096.min(size-off);
                let Some(bytes)=env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(base+off),count) else {continue;};
                for (i,word) in bytes.chunks_exact(4).enumerate() {
                    let vt=u32::from_le_bytes(word.try_into().unwrap());
                    let Some(label)=ds_station_label(vt) else {continue;};
                    let actor=base+off+i as u32*4;
                    if ds_read_aim_word(env,actor+0x10)==Some(world) {actors.push((label,actor,vt));}
                }
            }
        }
        *cache=(world,actors);
    }
    cache.1.clone()
}

#[cfg(test)]
mod station_vptr_tests {
    use super::ds_station_label;

    #[test]
    fn station_scan_uses_object_vptrs_instead_of_vtable_starts() {
        assert_eq!(ds_station_label(0x00a7_b648), Some("store"));
        assert_eq!(ds_station_label(0x00a7_b828), Some("bench"));
        assert_eq!(ds_station_label(0x00a7_eab8), Some("control"));
        assert_eq!(ds_station_label(0x00a7_eab0), None);
    }
}

/// Store/Bench own their native interaction handlers rather than the generic
/// pickup TapToAction forwarder. A live station could pass CanInteract while E
/// selected ammunition behind it. Use only these two verified native predicates
/// (both ignore their transform argument), then the original OnInteract. The
/// predicates preserve distance, facing, chapter state and active-combat gates.
fn ds_press_native_station(env:&mut Environment)->Option<(&'static str,u32)> {
    if !cfg!(target_os="windows") {return None;}
    let (hud,player)=ds_current_hud_player(env)?;
    if ds_read_aim_word(env,hud+0x234)!=Some(0) {return None;}
    let world=ds_read_aim_word(env,player+0x10)?;
    let mut candidates=Vec::new();
    let player_transform=ds_read_aim_word(env,player+0x4c)?;
    for (label,actor,vt) in ds_station_actors(env,world) {
        if ds_read_aim_word(env,actor)!=Some(vt) || ds_read_aim_word(env,actor+0x10)!=Some(world)
            || !env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(actor+0x164),1).is_some_and(|v|v[0]!=0) {continue;}
        let transform=ds_read_aim_word(env,actor+0x4c)?;
        if transform<0x1000 || env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(transform),0x50).is_none() {continue;}
        let accepted:u32=if label=="control" {
            GuestFunction::from_addr_and_thumb_flag(0x000d_5920,false)
                .call_from_host(env,(actor,transform))
        } else {
            let (predicate,opcode)=if label=="store" {(0x0009_165c,0xe92d_40f0)}else{(0x000b_5b84,0xe92d_4090)};
            if ds_read_aim_word(env,predicate)!=Some(opcode) {continue;}
            GuestFunction::from_addr_and_thumb_flag(predicate,false).call_from_host(env,(actor,transform))
        };
        if accepted==0 {continue;}
        let distance=(0..3).map(|i| {
            let d=read_guest_f32(env,transform+0x40+i*4)-read_guest_f32(env,player_transform+0x40+i*4);d*d
        }).sum::<f32>();
        if distance.is_finite() {candidates.push((distance,label,actor,transform));}
    }
    candidates.sort_by(|a,b|a.0.total_cmp(&b.0));
    let (_,label,actor,transform)=candidates.first().copied()?;
    // Hand off while the player is still in ordinary gameplay. A modal clears
    // our queued inputs, but does not clear vectors already latched in the
    // original engine. Sending stops after OnInteract would overwrite the
    // station's alignment or suit animation instead.
    ds_native_vector(env,player,1,0.0,0.0,false);
    if let Some(stop)=env.framework_state.opengles.ds_pc_look.cancel(player) {
        ds_native_vector(env,player,stop.kind,0.0,0.0,false);
    }
    env.framework_state.opengles.ds_pc_move_active=false;
    env.framework_state.opengles.ds_pc_move_player=0;
    echo!("[DSstationInput] released PC motion before native {} interaction",label);
    if label=="control" {
        let _: () = GuestFunction::from_addr_and_thumb_flag(0x0014_8738,false)
            .call_from_host(env,(actor,));
        return Some((label,actor));
    }
    let activate: u32 = if label=="store" {0x0009_2f84}else{0x000b_58fc};
    let _ = transform;
    let accepted:u32=GuestFunction::from_addr_and_thumb_flag(activate,false).call_from_host(env,(actor,));
    (accepted!=0).then_some((label,actor))
}

/// Read-only spatial evidence for isolated inbox regression. GetPos's exact
/// ARM body is `ldr r0,[r0,#0x4c]; add r0,r0,#0x40; bx lr`. Never move an
/// actor or bypass interaction gates here; diagnostics are outside A/B timing.
fn ds_log_station_positions(env:&mut Environment) {
    if std::env::var("TOUCHHLE_DS_STATION_TRACE").as_deref()!=Ok("1")
        || ds_read_aim_word(env,0x0003_a310)!=Some(0xe590_004c)
        || ds_read_aim_word(env,0x0003_a314)!=Some(0xe280_0040) {return;}
    let Some((_,player))=ds_current_hud_player(env) else {return;};
    let world=ds_read_aim_word(env,player+0x10).unwrap_or(0);
    // The visible station ring alone is not an interaction acceptance test:
    // Store::CanInteract and Bench::CanInteract also reject active combat.
    // Observe their original read-only predicates, never force an OnInteract.
    if world!=0 && ds_read_aim_word(env,0x0000_dd18)==Some(0xe92d_40f0) {
        let enemies:u32=GuestFunction::from_addr_and_thumb_flag(0x0000_dd18,false).call_from_host(env,(world,));
        echo!("[DSstationProbe] world={:#x} enemies_active={}",world,enemies);
    }
    let mut actors=ds_station_actors(env,world);
    actors.insert(0,("player",player,0x00a7_a4f8));
    for (label,actor,vt) in actors {
        if ds_read_aim_word(env,actor)!=Some(vt) {continue;}
        let transform=ds_read_aim_word(env,actor+0x4c).unwrap_or(0);
        if transform==0 {continue;}
        let pos:[f32;3]=std::array::from_fn(|i|f32::from_bits(ds_read_aim_word(env,transform+0x40+i as u32*4).unwrap_or(f32::NAN.to_bits())));
        let right:[f32;3]=std::array::from_fn(|i|f32::from_bits(ds_read_aim_word(env,transform+0x10+i as u32*0x10).unwrap_or(0)));
        let forward:[f32;3]=std::array::from_fn(|i|f32::from_bits(ds_read_aim_word(env,transform+0x18+i as u32*0x10).unwrap_or(0)));
        if pos.iter().all(|v|v.is_finite()) {echo!("[DSstationProbe] {} actor={:#x} world={:#x} pos={:?} right={:?} forward={:?}",label,actor,world,pos,right,forward);}
        let predicate=match label {"store"=>Some((0x0009_165c,0xe92d_40f0,0x1d0)),"bench"=>Some((0x000b_5b84,0xe92d_4090,0x214)),_=>None};
        if let Some((address,opcode,state_offset))=predicate {
            if ds_read_aim_word(env,address)==Some(opcode) {
                let accepted:u32=GuestFunction::from_addr_and_thumb_flag(address,false).call_from_host(env,(actor,transform));
                echo!("[DSstationProbe] {} actor={:#x} native_can_interact={} state={:?} enabled={:?}",label,actor,accepted,ds_read_aim_word(env,actor+state_offset),env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(actor+0x164),1));
            }
        }
    }
}

fn ds_call_gamepad_combat(env: &mut Environment, label: &str, addr: u32) {
    let Some((_hud, player)) = ds_current_hud_player(env) else {
        echo!("[DSpad] {} ignored: no player", label);
        return;
    };
    let call = GuestFunction::from_addr_and_thumb_flag(addr, false);
    let _: () = call.call_from_host(env, (player,));
    echo!("[DSpad] {} -> engine action {:#x}", label, addr);
}

fn ds_native_vector(env: &mut Environment, player: u32, kind: u32, x: f32, y: f32, sprint: bool) -> bool {
    // Exact class layout captured from original SendDPadEvent, including RTTI.
    // Only synchronous OnEvent receives this borrowed POD event; no destructor.
    if ds_read_aim_word(env,0x00a62e14)!=Some(0x00a61e98) || !x.is_finite() || !y.is_finite() { return false; }
    let event=env.mem.alloc(24);
    for (i,word) in crate::window::ds_pc_input::vector_event(kind,x,y,sprint).into_iter().enumerate() {
        env.mem.write(crate::mem::MutPtr::<u32>::from_bits(event.to_bits()+i as u32*4),word);
    }
    let _:u32=GuestFunction::from_addr_and_thumb_flag(0x00053dc8,false).call_from_host(env,(player,event.to_bits()));
    env.mem.free(event);true
}

fn ds_apply_pc_inputs(env:&mut Environment) {
    let _stutter_span=crate::ds_stutter_probe::Span::new(3,0);
    if !crate::window::ds_native_inputs() { return; }
    let input={let mut pending=crate::window::DS_PC_INPUT.lock().unwrap();let v=*pending;pending.look_x=0.0;pending.look_y=0.0;v};
    let Some((hud,player))=ds_current_hud_player(env) else {
        crate::window::DS_GAMEPLAY_ACTIVE.store(false,std::sync::atomic::Ordering::Relaxed);
        env.framework_state.opengles.ds_pc_move_player=0;
        env.framework_state.opengles.ds_pc_look=Default::default();
        return;
    };
    let hud_state:u32=env.mem.read(crate::mem::ConstPtr::from_bits(hud+0x234));
    let player_state=ds_read_aim_word(env,player+0x208).unwrap_or(u32::MAX);
    crate::ds_diagnostics::player(crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),hud_state,player_state);
    if std::env::var("TOUCHHLE_DS_STATION_TRACE").as_deref()==Ok("1") {
        static LAST:std::sync::Mutex<(u32,u32,u32)>=std::sync::Mutex::new((0,u32::MAX,u32::MAX));
        let state=player_state;
        let mut last=LAST.lock().unwrap();
        if *last!=(player,hud_state,state) {
            echo!("[DSstationState] frame={} player={:#x} hud={} state={} move=({:.3},{:.3}) look=({:.3},{:.3})",crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),player,hud_state,state,input.x,input.y,input.look_x,input.look_y);
            *last=(player,hud_state,state);
        }
    }
    let station_owns_input=crate::window::ds_pc_input::station_owns_input(player_state);
    crate::window::DS_GAMEPLAY_ACTIVE.store(hud_state==0 && !station_owns_input,std::sync::atomic::Ordering::Relaxed);
    if station_owns_input {
        // Do not emit even a neutral vector after the native cutscene owns the
        // player. In particular UpgradeRig has HUD=0 with no modal UI.
        env.framework_state.opengles.ds_pc_look=Default::default();
        env.framework_state.opengles.ds_pc_move_active=false;
        env.framework_state.opengles.ds_pc_move_player=0;
        return;
    }
    if hud_state!=0 {
        if let Some(stop)=env.framework_state.opengles.ds_pc_look.cancel(player) {
            ds_native_vector(env,player,stop.kind,stop.x,stop.y,false);
        }
        env.framework_state.opengles.ds_pc_move_player=0;
        return;
    }
    let aiming:u8=env.mem.read(crate::mem::ConstPtr::from_bits(player+0x1e1));
    let cap:f32=std::env::var("TOUCHHLE_DS_AIM_FORWARD_LIMIT").ok().and_then(|v|v.parse().ok()).unwrap_or(crate::window::ds_pc_input::DEFAULT_AIM_WALK_LIMIT);
    let aimed_walk=std::env::var("TOUCHHLE_DS_AIM_WALK").as_deref()!=Ok("0");
    let movement=crate::window::ds_pc_input::movement(input.x,input.y,aiming!=0 && aimed_walk,cap);
    let active=movement.x.abs()>0.0001 || movement.y.abs()>0.0001;
    let changed_context=env.framework_state.opengles.ds_pc_move_player!=player;
    if active || env.framework_state.opengles.ds_pc_move_active || changed_context {
        ds_native_vector(env,player,1,movement.x,movement.y,movement.sprint);
    }
    if movement.limited!=env.framework_state.opengles.ds_pc_forward_limited || active!=env.framework_state.opengles.ds_pc_move_active {
        echo!("[DSpcMove] raw=({:.3},{:.3}) native=({:.3},{:.3}) aiming={} forward_limit={} sprint={}",input.x,input.y,movement.x,movement.y,aiming,movement.limited,movement.sprint);
    }
    env.framework_state.opengles.ds_pc_move_player=player;
    env.framework_state.opengles.ds_pc_move_active=active;
    env.framework_state.opengles.ds_pc_forward_limited=movement.limited;
    let state:u32=env.mem.read(crate::mem::ConstPtr::from_bits(player+0x208));
    let frame=crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    // Type 5 is the held-attack camera; neutralize the type actually sent last.
    // Several glClear calls can occur per presented frame: do not cancel a
    // fresh delta again before the guest has had a frame to consume it.
    let look=env.framework_state.opengles.ds_pc_look.events(frame,player,if state==4 {5}else{3},input.look_x,input.look_y);
    for event in look.into_iter().flatten() {
        ds_native_vector(env,player,event.kind,event.x,event.y,false);
    }
    let after:u8=env.mem.read(crate::mem::ConstPtr::from_bits(player+0x1e1));
    crate::window::DS_AIM_ACTIVE.store(after!=0,std::sync::atomic::Ordering::Relaxed);
}

fn ds_pc_fire_current_mode(env:&mut Environment) {
    let Some((hud,player))=ds_current_hud_player(env) else{return;};
    let state:u32=env.mem.read(crate::mem::ConstPtr::from_bits(hud+0x234));
    if state==0 {let result=ds_send_player_fire_action(env,player);echo!("[DSpc] native tap/fire current mode dispatched={}",result.is_some());}
}

const DS_HELD_AIM_TIMEOUT: u32 = 0x7fff_ffff;

fn ds_read_aim_word(env: &Environment, address: u32) -> Option<u32> {
    let bytes = env.mem.get_bytes_fallible(crate::mem::ConstVoidPtr::from_bits(address), 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn ds_restore_aim_timeout(env: &mut Environment, lease: (u32, u32)) {
    if ds_read_aim_word(env, lease.0) == Some(DS_HELD_AIM_TIMEOUT) {
        env.mem.write(crate::mem::MutPtr::<u32>::from_bits(lease.0), lease.1);
        echo!("[DSaimControl] restored native idle timeout={}ms", lease.1);
    }
}

pub(super) fn ds_release_aim_timeout(env: &mut Environment) {
    env.framework_state.opengles.ds_aim_hold_requested = false;
    if let Some(lease) = env.framework_state.opengles.ds_aim_timeout_lease.take() {
        ds_restore_aim_timeout(env, lease);
    }
}

fn ds_rearm_held_controller_aim(env: &mut Environment) {
    if std::env::var("TOUCHHLE_DS_CONTROLLER_AIM_MODE").as_deref() == Ok("toggle")
        || env.framework_state.opengles.ds_aim_hold_requested
        || env.framework_state.opengles.ds_aim_timeout_lease.is_some()
        || !crate::window::DS_CONTROLLER_LT_HELD.load(std::sync::atomic::Ordering::Relaxed)
    {
        return;
    }
    // A held LT edge can be consumed while stasis/kinesis owns the player.
    // Requeue only while the native player is still busy or temporarily
    // unavailable; state 0 with a deliberate native cancellation remains
    // untouched.
    let busy = ds_current_hud_player(env)
        .map(|(hud, _)| env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(hud + 0x234)) != 0)
        .unwrap_or(true);
    if busy {
        crate::window::DS_AIM_REQUEST.store(1, std::sync::atomic::Ordering::Relaxed);
    }
}

fn ds_update_aim_timeout(env: &mut Environment) {
    ds_retry_pending_aim_release(env);
    ds_rearm_held_controller_aim(env);
    if !env.framework_state.opengles.ds_aim_hold_requested && env.framework_state.opengles.ds_aim_timeout_lease.is_none() { return; }
    let mut eligible = false;
    if let Some((hud, player)) = ds_current_hud_player(env) {
        let state: u32 = env.mem.read(crate::mem::ConstPtr::from_bits(hud + 0x234));
        if player == env.framework_state.opengles.ds_aim_hold_player && state == 0 {
            eligible = env.framework_state.opengles.ds_aim_hold_requested
                && env.mem.read(crate::mem::ConstPtr::<u8>::from_bits(player + 0x1e1)) != 0;
        } else { env.framework_state.opengles.ds_aim_hold_requested = false; }
    } else { env.framework_state.opengles.ds_aim_hold_requested = false; }
    if !eligible {
        if let Some(lease) = env.framework_state.opengles.ds_aim_timeout_lease.take() { ds_restore_aim_timeout(env, lease); }
        return;
    }
    if env.framework_state.opengles.ds_aim_timeout_lease.is_some() { return; }
    // Exact original OnUpdate instruction: ldr r0,[r0,#0x1bc] after Tweaks::get.
    // Only the idle timeout is leased. Sprint/damage/reload/QTE can still lower
    // the weapon; this never re-enters aim after a native cancellation.
    if ds_read_aim_word(env, 0x00055518) != Some(0xe59001bc) {
        env.framework_state.opengles.ds_aim_hold_requested = false;
        echo!("[DSaimControl] hold timeout disabled: executable signature mismatch");
        return;
    }
    let tweaks: u32 = GuestFunction::from_addr_and_thumb_flag(0x0007e448, false).call_from_host(env, ());
    let Some(address) = tweaks.checked_add(0x1bc).filter(|_| tweaks != 0) else { return; };
    let Some(original) = ds_read_aim_word(env, address).filter(|v| (1..=600_000).contains(v)) else {
        env.framework_state.opengles.ds_aim_hold_requested = false;
        echo!("[DSaimControl] hold timeout disabled: unrecognised native timeout");
        return;
    };
    env.mem.write(crate::mem::MutPtr::<u32>::from_bits(address), DS_HELD_AIM_TIMEOUT);
    env.framework_state.opengles.ds_aim_timeout_lease = Some((address, original));
    echo!("[DSaimControl] persistent aim leases native idle timeout {}ms; native cancellation remains active", original);
}

fn ds_apply_aim_request(env: &mut Environment, request: i32) {
    if request == 0 {
        env.framework_state.opengles.ds_aim_hold_requested = false;
        // Keep a release edge alive when the native player is hidden by a
        // control/QTE. The old one-shot request was consumed while the
        // lookup returned None, leaving +0x1e1 set and restoring aim after
        // the control ended.
        env.framework_state.opengles.ds_aim_release_pending = true;
        // Release can arrive while stasis/kinesis/death owns the player and
        // the native player lookup is temporarily unavailable. Keep retrying
        // until native aiming is actually cleared.
        let applied = ds_gamepad_set_aim(env, false);
        if applied {
            env.framework_state.opengles.ds_aim_release_pending = false;
        }
        return;
    }
    // A fresh LT press supersedes an old release that was waiting for the
    // player object to become visible again.
    env.framework_state.opengles.ds_aim_release_pending = false;
    let Some((_hud, player)) = ds_current_hud_player(env) else { ds_release_aim_timeout(env); return; };
    let aiming: u8 = env.mem.read(crate::mem::ConstPtr::from_bits(player + 0x1e1));
    env.framework_state.opengles.ds_aim_hold_requested = request == 1 || (request == 2 && aiming == 0);
    env.framework_state.opengles.ds_aim_hold_player = player;
    let entering=if request==2 {aiming==0}else{true};
    if entering && aiming==0 && crate::window::ds_native_inputs() {
        // SetRunning refuses changes while already aiming. Apply the small
        // native movement first, so a stick already at full forward stops
        // sprinting before the native raise-weapon event executes.
        let raw=*crate::window::DS_PC_INPUT.lock().unwrap();
        let limit=std::env::var("TOUCHHLE_DS_AIM_FORWARD_LIMIT").ok().and_then(|v|v.parse().ok()).unwrap_or(crate::window::ds_pc_input::DEFAULT_AIM_WALK_LIMIT);
        let enabled=std::env::var("TOUCHHLE_DS_AIM_WALK").as_deref()!=Ok("0");
        let movement=crate::window::ds_pc_input::movement(raw.x,raw.y,enabled,limit);
        ds_native_vector(env,player,1,movement.x,movement.y,movement.sprint);
    }
    ds_gamepad_set_aim(env, entering);
}

fn ds_retry_pending_aim_release(env: &mut Environment) {
    if !env.framework_state.opengles.ds_aim_release_pending
        || crate::window::DS_CONTROLLER_LT_HELD.load(std::sync::atomic::Ordering::Relaxed)
    {
        return;
    }
    if ds_gamepad_set_aim(env, false) {
        env.framework_state.opengles.ds_aim_release_pending = false;
        echo!("[DSpad] delayed aim release applied after native control");
    }
}

fn ds_gamepad_set_aim(env: &mut Environment, entering: bool) -> bool {
    let Some((_hud, player)) = ds_current_hud_player(env) else {
        echo!("[DSpad] aim request ignored: no player");
        return false;
    };
    let before: u8 = env
        .mem
        .read(crate::mem::ConstPtr::<u8>::from_bits(player + 0x1e1));
    if before == entering as u8 {
        return true;
    }
    if entering {
        let state: u32 = env
            .mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x208));
        if state != 0 && state != 3 {
            echo!("[DSpad] aim enter ignored: player state {state}");
            return false;
        }
    }
    if entering {
        // SetAiming alone leaves the idle timer (+0x1e4) untouched, so an
        // idle player immediately lowers the weapon on the next update.
        // Action 6 while not aiming follows the original raise-weapon path:
        // it checks the native guards and resets that timer before SetAiming.
        // Never send it when already aiming: that would fire a shot.
        if ds_send_player_fire_action(env, player).is_none() {
            echo!("[DSpad] aim enter ignored: no enabled raise-weapon forwarder");
            return false;
        }
    } else {
        let set_aiming = GuestFunction::from_addr_and_thumb_flag(0x0003f884, false);
        let _: () = set_aiming.call_from_host(env, (player, 0u32));
    }
    let after: u8 = env
        .mem
        .read(crate::mem::ConstPtr::<u8>::from_bits(player + 0x1e1));
    crate::window::DS_AIM_ACTIVE.store(after!=0,std::sync::atomic::Ordering::Relaxed);
    echo!(
        "[DSpad] native aim {}: {before} -> {after}",
        if entering { "enter" } else { "release" }
    );
    after == entering as u8
}

fn ds_invoke_hud_string(env: &mut Environment, hud: u32, command: crate::window::ds_hud_commands::HudCommand) -> Option<u32> {
    let (_name, address) = command.spec();
    let expected = command.string_bytes();
    if env.mem.get_bytes_fallible(crate::mem::ConstVoidPtr::from_bits(address), expected.len() as u32) != Some(expected.as_slice()) {
        echo!("[DShud] {:?} rejected: executable string signature mismatch", command); return None;
    }
    // This verified handler consumes only a borrowed wchar32 begin/end pair.
    // It neither owns this view nor receives it via generic event dispatch.
    let event = env.mem.alloc(24);
    for (index, word) in command.borrowed_view().into_iter().enumerate() {
        env.mem.write(crate::mem::MutPtr::<u32>::from_bits(event.to_bits() + index as u32 * 4), word);
    }
    let accepted: u32 = GuestFunction::from_addr_and_thumb_flag(0x00194158, false)
        .call_from_host(env, (hud, event.to_bits()));
    env.mem.free(event);
    Some(accepted)
}

fn ds_native_hud_command(env: &mut Environment, command: crate::window::ds_hud_commands::HudCommand) {
    use crate::window::ds_hud_commands::HudCommand;
    if command == HudCommand::Locator { ds_player_shortcuts::locator(env); return; }
    let Some((hud, player)) = (if command == HudCommand::Pause {
        ds_pause_hud_player(env)
    } else {
        ds_current_hud_player(env)
    }) else {
        echo!("[DShud] {:?} ignored: no live HUD/player", command); return;
    };
    let mut state: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(hud + 0x234));
    let prepared_rig = command.needs_rig(state);
    if command.needs_rig(state) || (command == HudCommand::Rig && state == 0) {
        // State 0 is ordinary closed HUD, not a cutscene-only state. Enter
        // through the game's own CanEnterRig + BTN_RIG path; never set state.
        let can_enter: u32 = GuestFunction::from_addr_and_thumb_flag(0x0018b9ec, false)
            .call_from_host(env, (hud,));
        if can_enter == 0 {
            echo!("[DShud] {:?} ignored: CanEnterRig rejected", command); return;
        }
        if command != HudCommand::Rig {
            if ds_invoke_hud_string(env, hud, HudCommand::Rig).is_none() { return; }
            let after: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(hud + 0x234));
            echo!("[DShud] prepare BTN_RIG HUD={state}->{after}");
            state = after;
            if state != 1 { echo!("[DShud] {:?} not dispatched: native RIG did not enter state 1", command); return; }
        }
    }
    if !command.allowed(state) {
        echo!("[DShud] {:?} ignored: HUD state {}", command, state); return;
    }
    let Some(accepted) = ds_invoke_hud_string(env, hud, command) else { return; };
    let (name, _) = command.spec();
    let mut state_after: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(hud + 0x234));
    // Selection/locator should not leave a wheel that this request opened.
    // Close it via the original toggle event, preserving a pre-existing wheel.
    if prepared_rig && command != HudCommand::Pause && state_after == 1 {
        if ds_invoke_hud_string(env, hud, HudCommand::Rig).is_some() {
            state_after = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(hud + 0x234));
        }
    }
    let weapon: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x2ec));
    let expected = command
        .weapon_index()
        .map(|index| format!(", expected weapon={index}"))
        .unwrap_or_default();
    echo!(
        "[DShud] {name} HUD={state}->{state_after}{expected}, current weapon={weapon}, native result={accepted}; gameplay outcome not inferred"
    );
}

fn ds_gamepad_context_combat(env: &mut Environment) {
    let Some((hud, _player)) = ds_current_hud_player(env) else {
        echo!("[DSpad] LB ignored: no player");
        return;
    };
    // This selector belongs to Hud, not Player. Reading Player+1cc always
    // chose slash at stomp crates, despite E reading the correct HUD fields.
    let selected: u8 = env
        .mem
        .read(crate::mem::ConstPtr::<u8>::from_bits(hud + 0x1cc));
    let (addr, label) = if selected != 0 {
        (0x0004fc64, "LB stomp")
    } else {
        (0x0004fa4c, "LB melee")
    };
    ds_call_gamepad_combat(env, label, addr);
}

fn ds_fire_snapshot(env:&mut Environment)->Option<crate::window::ds_fire_control::Snapshot> {
    let (hud,player)=ds_current_hud_player(env)?;
    if ds_read_aim_word(env,hud+0x234)!=Some(0){return None;}
    let index=ds_read_aim_word(env,player+0x2ec)?;if index>5{return None;}
    let weapon=ds_read_aim_word(env,player+0x2d4+index*4)?;if weapon<0x1000{return None;}
    let vt=ds_read_aim_word(env,weapon)?;
    let has_mode=matches!(vt,0x00a7_ace8|0x00a7_b9f8|0x00a7_c1c8|0x00a7_dea8);
    if !has_mode && vt!=0x00a8_a498{return None;}
    let is_alt=ds_read_aim_word(env,vt+0x19c)?;
    let alt:u32=GuestFunction::from_addr_with_thumb_bit(is_alt).call_from_host(env,(weapon,));
    let aiming:u8=env.mem.read(crate::mem::ConstPtr::from_bits(player+0x1e1));
    let idle=if matches!(vt,0x00a7_b9f8|0x00a7_c1c8|0x00a7_dea8){ds_read_aim_word(env,weapon+0x23c)==Some(0)}else{true};
    let settled=if vt==0x00a7_ace8 {
        // SetAltMode snaps this filter and skips the native rotate/audio path.
        // OnTilt sets its target; allow the actual model to reach that target
        // before dispatching the first shot, rather than selecting and firing
        // in the same glClear callback.
        let target:u32=GuestFunction::from_addr_and_thumb_flag(0x0005_d520,false).call_from_host(env,(weapon+0x244,));
        let current:u32=GuestFunction::from_addr_and_thumb_flag(0x0005_d660,false).call_from_host(env,(weapon+0x244,));
        let (target,current)=(f32::from_bits(target),f32::from_bits(current));
        target.is_finite()&&current.is_finite()&&(target-current).abs()<=0.5
    }else{true};
    Some(crate::window::ds_fire_control::Snapshot{player,weapon,alt:alt!=0,has_mode,can_tilt:aiming!=0&&idle,can_fire:aiming!=0&&idle&&settled})
}
fn ds_tick_fire_control(env:&mut Environment){
    use crate::window::ds_fire_control::{self,Effect,Input,Source};
    if !crate::window::ds_native_inputs() && !crate::ds_frontend_controller::configured(){return;}
    #[cfg(all(windows, target_arch = "x86_64"))]
    crate::ds_pc_jit_prewarm::maybe(env);
    if crate::window::DS_FIRE_RELEASE_REQUEST.swap(false,std::sync::atomic::Ordering::Relaxed){ds_fire_control::enqueue(Input::Cancel);}
    // Keep the old diagnostic atomic compatible; normal physical events now
    // retain their ordered press/release identity in the native input queue.
    match crate::window::DS_FIRE_MODE_REQUEST.swap(0,std::sync::atomic::Ordering::Relaxed){
        1=>ds_fire_control::enqueue(Input::Fire(Source::Diagnostic,Some(false))),
        2=>ds_fire_control::enqueue(Input::Fire(Source::Diagnostic,Some(true))),
        3=>ds_fire_control::enqueue(Input::Fire(Source::Diagnostic,None)),_=>{},
    }
    let snapshot=ds_fire_snapshot(env);
    let effects=ds_fire_control::tick(snapshot,crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed));
    for effect in effects {match effect {
        Effect::Stop=>ds_gamepad_release_fire(env),
        Effect::Tilt=>if let Some(before)=snapshot {
            let vt=ds_read_aim_word(env,before.weapon).unwrap_or(0);
            if let Some(call)=ds_read_aim_word(env,vt+0x180){
                let _:()=GuestFunction::from_addr_with_thumb_bit(call).call_from_host(env,(before.weapon,));
                echo!("[DSfire] native OnTilt weapon={:#x} alt_before={} frame={}",before.weapon,before.alt,crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed));
            }
        },
        Effect::Fire=>{ds_pc_fire_current_mode(env);if let Some(s)=snapshot {echo!("[DSfire] native fire weapon={:#x} alt={} frame={}",s.weapon,s.alt,crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed));}},
    }}
}

fn ds_gamepad_release_fire(env: &mut Environment) {
    let Some((_hud, player)) = ds_current_hud_player(env) else {
        echo!("[DSpad] fire release ignored: no player");
        return;
    };
    let index: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x2ec));
    if index > 5 {
        return;
    }
    let weapon: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(
        player + 0x2d4 + index * 4,
    ));
    if weapon == 0 {
        return;
    }
    let vtable: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(weapon));
    let is_holding_addr: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(vtable + 0x190));
    let on_abort_addr: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(vtable + 0x188));
    if is_holding_addr == 0 || on_abort_addr == 0 {
        return;
    }
    let is_holding =
        GuestFunction::from_addr_and_thumb_flag(is_holding_addr, is_holding_addr & 1 != 0);
    let holding: u32 = is_holding.call_from_host(env, (weapon,));
    if holding == 0 {
        return;
    }
    let on_abort = GuestFunction::from_addr_and_thumb_flag(on_abort_addr, on_abort_addr & 1 != 0);
    let _: () = on_abort.call_from_host(env, (weapon,));
    echo!("[DSpad] weapon {index} fire released -> native OnAbort");
}

/// The player object: the dispatch target of any player-owned action forwarder
/// (`InputSchemeBasic` hands the `GameObjectPlayable` pointer to each button).
fn ds_find_player(env: &mut Environment) -> Option<u32> {
    for a in ds_sweep_vptr(env, 0x00a7d1e8) {
        let t = target_of(env, a);
        if t == 0 {
            continue;
        }
        let vt: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(t));
        if vt == 0x00a7a4f8 {
            return Some(t);
        }
    }
    None
}

/// Publish "the player is aiming" (`GameObjectPlayable+0x1e1`, set by
/// `SetAiming` 0x0003f884) to the window layer, which uses it for
/// `TOUCHHLE_DS_MOUSE_LOOK=aim`. The player pointer is cached, so the common case
/// is two memory reads per frame; a re-sweep happens at most once every 120
/// frames (and only until the player is found).
fn ds_publish_aim_flag(env: &mut Environment) {
    crate::ds_startup_report::player_state(
        crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed), u32::MAX);

    use std::sync::atomic::{AtomicU32, Ordering};
    static PLAYER: AtomicU32 = AtomicU32::new(0);
    static LAST_SWEEP: AtomicU32 = AtomicU32::new(0);
    let mut p = if crate::window::ds_native_inputs() {
        if ds_read_aim_word(env,0x00a62e14)!=Some(0x00a61e98) { return; }
        let p=ds_current_hud_player(env).map(|(_,p)|p).unwrap_or(0);PLAYER.store(p,Ordering::Relaxed);
        if p==0 { crate::window::DS_AIM_ACTIVE.store(false,Ordering::Relaxed);return; } p
    } else {PLAYER.load(Ordering::Relaxed)};
    if p != 0 {
        let vt: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(p));
        if vt != 0x00a7a4f8 {
            PLAYER.store(0, Ordering::Relaxed);
            p = 0;
        }
    }
    if p == 0 {
        let frame = crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed) as u32;
        if frame.saturating_sub(LAST_SWEEP.load(Ordering::Relaxed)) >= 120 {
            LAST_SWEEP.store(frame, Ordering::Relaxed);
            if let Some(found) = ds_find_player(env) {
                PLAYER.store(found, Ordering::Relaxed);
                p = found;
            }
        }
    }
    if p != 0 {
        if crate::ds_startup_report::benchmark_active() && crate::ds_chapter_overlay::benchmark_scene_ready(env,p) {
            let state: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(p + 0x208));
            crate::ds_startup_report::player_state(
                crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed), state);
        }
        let aiming: u8 = env
            .mem
            .read(crate::mem::ConstPtr::<u8>::from_bits(p + 0x1e1));
        crate::window::DS_AIM_ACTIVE.store(aiming != 0, std::sync::atomic::Ordering::Relaxed);
        // `TOUCHHLE_DS_AIM_TRACE=1`: log every change of the aiming byte with the
        // presented-frame number. The flag is momentary (SetAiming(1) is called
        // from StateTransition, so it only exists for the frames of the
        // aim/fire state), and without this trace a probe that lands a second
        // later always reads 0 - which is exactly what misled the first
        // "how do you aim" experiments.
        if std::env::var_os("TOUCHHLE_DS_AIM_TRACE").is_some() {
            static LAST: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);
            let prev = LAST.swap(aiming as u32, std::sync::atomic::Ordering::Relaxed);
            if prev != aiming as u32 {
                echo!(
                    "[DSaim] aiming {} -> {} f={} state={}",
                    if prev == u32::MAX { -1 } else { prev as i32 },
                    aiming,
                    crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),
                    env.mem
                        .read(crate::mem::ConstPtr::<u32>::from_bits(p + 0x208))
                );
            }
        }
    }
}

/// Print the player's own gauges, read through the engine's own getters, so an
/// action's effect is a **number** instead of a screenshot:
///
/// * `GameObjectPlayable::GetStasis()` (`0x0003b38c`, float)
/// * `GameObjectPlayable::GetStasisMax()` (`0x0003b3cc`, float)
/// * `GameObjectPlayable::GetWeaponAmmo(WeaponIndex)` (`0x00041a70`, int)
///
/// plus the raw fields the action table itself reads: the selected weapon index
/// (`+0x2ec`), the weapon slot 0 pointer (`+0x2d4`), the "current usable object"
/// (`+0x30c`), the state (`+0x208`) and the flag word (`+0x1a4`). Every action
/// in `ds_send_action` logs one `before`/`after` pair, which is how the
/// unmapped codes in DECOMPILE.md D-034 get pinned down.
fn ds_log_player_gauges(env: &mut Environment, player: u32, tag: &str) {
    if crate::window::ds_native_inputs() && player!=0 {
        let pitch:f32=env.mem.read(crate::mem::ConstPtr::from_bits(player+0x1a8));
        let x:f32=env.mem.read(crate::mem::ConstPtr::from_bits(player+0x1b0));
        let z:f32=env.mem.read(crate::mem::ConstPtr::from_bits(player+0x1b8));
        let running:u8=env.mem.read(crate::mem::ConstPtr::from_bits(player+0x1e0));
        echo!("[DSpcProbe] {} pitch={:.4} move=({:.4},{:.4}) running={}",tag,pitch,x,z,running);
    }

    if player == 0 {
        return;
    }
    let vt: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(player));
    if vt != 0x00a7a4f8 {
        echo!(
            "[DSgauge] {} target={:#010x} vptr={:#010x} (not GameObjectPlayable)",
            tag,
            player,
            vt
        );
        return;
    }
    let sel: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x2ec));
    let state: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x208));
    let flags: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x1a4));
    // `GameObjectPlayable::SetAiming()` (0x0003f884) stores the aim flag at
    // `+0x1e1` (and mirrors it as bit 6 of `+0x204`), so this byte *is* "the
    // player is aiming right now" - the hook the mouse-look mode needs.
    let aiming: u8 = env
        .mem
        .read(crate::mem::ConstPtr::<u8>::from_bits(player + 0x1e1));
    let use_obj: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x30c));
    let w0: u32 = env
        .mem
        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x2d4));
    let get_stasis = GuestFunction::from_addr_and_thumb_flag(0x0003b38c, false);
    let get_max = GuestFunction::from_addr_and_thumb_flag(0x0003b3cc, false);
    let stasis: f32 = get_stasis.call_from_host(env, (player,));
    let max: f32 = get_max.call_from_host(env, (player,));
    // Only ask for ammo when weapon slot 0 exists: the getter dereferences the
    // slot without a null check.
    let ammo: i32 = if w0 != 0 {
        let get_ammo = GuestFunction::from_addr_and_thumb_flag(0x00041a70, false);
        get_ammo.call_from_host(env, (player, 0i32))
    } else {
        -1
    };
    // The class of `+0x30c` (the object the "act" action code 0x0b talks to) and
    // of the selected weapon slot decide *what* firing means: the vptr is
    // resolved to a class name offline (`tools/arm-vtl.py` / `tools/syms.py`).
    let sel_obj: u32 = if sel <= 8 {
        env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(
            player + 0x2d4 + sel * 4,
        ))
    } else {
        0
    };
    let use_vt: u32 = if use_obj != 0 {
        env.mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(use_obj))
    } else {
        0
    };
    let w0_vt: u32 = if w0 != 0 {
        env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(w0))
    } else {
        0
    };
    let sel_vt: u32 = if sel_obj != 0 {
        env.mem
            .read(crate::mem::ConstPtr::<u32>::from_bits(sel_obj))
    } else {
        0
    };
    // Clip state of the *selected* weapon: `Weapon::GetAmmoInClip()` (0x0007224c)
    // and `WeaponPlasmaCutter::GetAmmoPerClip()` (0x00072d00) - a virtual call in
    // the engine, but these two concrete entry points are what the shipped build
    // resolves to, and they are the numbers `TryReload` itself compares.
    let (clip, clip_max) = if sel_obj != 0 {
        let get_clip = GuestFunction::from_addr_and_thumb_flag(0x0007224c, false);
        let get_clip_max = GuestFunction::from_addr_and_thumb_flag(0x00072d00, false);
        let a: u32 = get_clip.call_from_host(env, (sel_obj,));
        let b: u32 = get_clip_max.call_from_host(env, (sel_obj,));
        (a as i32, b as i32)
    } else {
        (-1, -1)
    };
    echo!(
        "[DSgauge] {} aiming={} stasis={:.4}/{:.4} ammo0={} clip={}/{} sel={} state={} flags={:#x} \
         use={:#010x}(vptr {:#010x}) w0={:#010x}(vptr {:#010x}) selobj={:#010x}(vptr {:#010x})",
        tag,
        aiming,
        stasis,
        max,
        ammo,
        clip,
        clip_max,
        sel,
        state,
        flags,
        use_obj,
        use_vt,
        w0,
        w0_vt,
        sel_obj,
        sel_vt
    );
}

/// `TOUCHHLE_DS_SEND_ACTION="0x0a,0x08,…"`: fire one engine action per
/// **G key** press (the recommended way: an input script controls the timing
/// exactly). Setting `TOUCHHLE_DS_SEND_ACTION_AT=<frame>` switches to
/// frame-scheduled mode instead: one action every
/// `TOUCHHLE_DS_SEND_ACTION_GAP` frames (default 150) from that frame on.
/// Exists so a single VM run can probe every action code while the
/// code -> action table is being confirmed (DECOMPILE.md D-034).
fn ds_scripted_actions(env: &mut Environment) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    static INITED: AtomicUsize = AtomicUsize::new(0);
    let Ok(spec) = std::env::var("TOUCHHLE_DS_SEND_ACTION") else {
        return;
    };
    if INITED.load(Ordering::Relaxed) == 0 {
        INITED.store(1, Ordering::Relaxed);
        echo!("[DSact] scripted codes: {spec}");
    }
    let codes: Vec<u32> = spec.split(',').filter_map(|v| ds_parse_u32(v)).collect();
    if codes.is_empty() {
        return;
    }
    let frame = crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed);
    let at: Option<u64> = std::env::var("TOUCHHLE_DS_SEND_ACTION_AT")
        .ok()
        .and_then(|v| v.trim().parse().ok());
    let gap: u64 = std::env::var("TOUCHHLE_DS_SEND_ACTION_GAP")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(150)
        .max(1);
    let mut keyed = crate::window::DS_NEXT_ACTION.swap(false, Ordering::Relaxed);
    loop {
        let i = NEXT.load(Ordering::Relaxed);
        if i >= codes.len() {
            if keyed {
                echo!("[DSact] no codes left (list had {})", codes.len());
            }
            return;
        }
        let due = match at {
            Some(at) => frame >= at + (i as u64) * gap,
            None => false,
        };
        if !due && !keyed {
            return;
        }
        keyed = false;
        NEXT.store(i + 1, Ordering::Relaxed);
        let code = codes[i];
        match ds_send_action(env, code) {
            Some(a) => echo!(
                "[DSact] f={} code={:#x} -> forwarder {:#010x} (SendAction) [{} of {}]",
                frame,
                code,
                a,
                i + 1,
                codes.len()
            ),
            None => echo!(
                "[DSact] f={} code={:#x} -> no enabled forwarder (button not on offer) [{} of {}]",
                frame,
                code,
                i + 1,
                codes.len()
            ),
        }
    }
}

/// One-shot, on-demand memory sweep for objects whose first word is `want`.
/// The E key uses it for `InputForwarderTapToAction` (vptr 0x00a7d1e8) so the
/// data-coordinate path needs **no** environment variables at all: press E and
/// it finds the action forwarders itself.
pub(crate) fn ds_sweep_vptr(env: &mut Environment, want: u32) -> Vec<u32> {
    let _stutter_span=crate::ds_stutter_probe::Span::new(12,want as u64);
    let mut found: Vec<u32> = Vec::new();
    let ranges: Vec<(u32, u32)> = env.mem.iter_used_ranges().collect();
    for (base, size) in ranges {
        let mut off: u32 = 0;
        while off + 4 <= size {
            let start = base + off;
            let len = 4096.min(size - off);
            let hit = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut v: Vec<u32> = Vec::new();
                let mut a = start;
                while a + 4 <= start + len {
                    let w: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(a));
                    if w == want {
                        v.push(a);
                    }
                    a += 4;
                }
                v
            }));
            if let Ok(v) = hit {
                for a in v {
                    if !found.contains(&a) {
                        found.push(a);
                    }
                    if found.len() >= 512 {
                        return found;
                    }
                }
            }
            off += len;
        }
    }
    found
}

/// Parse `123`, `0x7b` into a u32 (the project's switches accept both).
fn ds_parse_u32(s: &str) -> Option<u32> {
    let t = s.trim();
    match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(h, 16).ok(),
        None => t.parse().ok(),
    }
}

/// Deferred tracker tap: the frame at which to fire (`0` = nothing pending) and
/// the packed (x << 16) | y point. Set when the tracked byte turns non-zero and
/// `TOUCHHLE_DS_TRACK_TAP_DELAY` is non-zero; the tracker block fires it once
/// that frame arrives. (Tapping on the flip frame itself does nothing - see
/// DECOMPILE.md D-016/D-018.)
pub(crate) static TAP_AT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub(crate) static TAP_XY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// 3x5 bitmap digits (plus `#`, `M`, `-`) so a dumped frame can carry its own
/// shot number in the corner. Five rows, low three bits of each.
fn ds_glyph(c: char) -> [u8; 5] {
    match c {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '#' => [0b101, 0b111, 0b101, 0b111, 0b101],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        _ => [0b000; 5],
    }
}

/// Draw `text` into a P6 buffer (header already written) at the top-left.
fn ds_burn_label(out: &mut [u8], header: usize, sw: usize, sh: usize, text: &str) {
    if sw < 16 || sh < 16 {
        return;
    }
    let ts = (sw / 150).max(2);
    let cw = 4 * ts;
    let ch = 6 * ts;
    let total_w = cw * text.len() + ts;
    if total_w > sw {
        return;
    }
    // black plate behind the text so it reads on any background
    for y in 0..(ch + ts).min(sh) {
        for x in 0..total_w.min(sw) {
            let i = header + (y * sw + x) * 3;
            if i + 2 < out.len() {
                out[i] = 0;
                out[i + 1] = 0;
                out[i + 2] = 0;
            }
        }
    }
    for (n, c) in text.chars().enumerate() {
        let g = ds_glyph(c);
        let ox = ts + n * cw;
        for (ry, row) in g.iter().enumerate() {
            for rx in 0..3 {
                if row & (1 << (2 - rx)) == 0 {
                    continue;
                }
                for sy in 0..ts {
                    for sx in 0..ts {
                        let x = ox + rx * ts + sx;
                        let y = ts + ry * ts + sy;
                        if x >= sw || y >= sh {
                            continue;
                        }
                        let i = header + (y * sw + x) * 3;
                        if i + 2 < out.len() {
                            out[i] = 255;
                            out[i + 1] = 255;
                            out[i + 2] = 255;
                        }
                    }
                }
            }
        }
    }
}

#[track_caller]
fn with_ctx_and_mem<T, U: Default>(env: &mut Environment, f: T) -> U
where
    T: FnOnce(&mut dyn GLES, &mut Mem) -> U,
{
    let current_ctx = env
        .framework_state
        .opengles
        .effective_ctx_id(env.current_thread);
    let Some(current_ctx) = current_ctx else {
        log_dbg!(
            "Skipping GLES call without context (line {})",
            std::panic::Location::caller().line()
        );
        return U::default();
    };
    let trace = env.options.trace_gl_errors;
    let caller = std::panic::Location::caller();
    let _stutter_span=crate::ds_stutter_probe::Span::new(1,caller.line() as u64);
    // The context ID was already resolved above; bind that exact context
    // without repeating the ThreadId lookup. A context can still disappear
    // between the lookup and borrow, so preserve the existing fallback.
    let Some(mut gles) = super::sync_context_with_id(
        &mut env.framework_state.opengles,
        &mut env.objc,
        env.window
            .as_mut()
            .expect("OpenGL ES is not supported in headless mode"),
        env.current_thread,
        current_ctx,
    ) else {
        log_dbg!(
            "Skipping GLES call after sync_context returned None (line {})",
            caller.line()
        );
        return U::default();
    };
    let res = f(gles.as_mut(), &mut env.mem);
    if trace {
        let err = unsafe { gles.GetError() };
        if err != 0 {
            log!(
                "[--trace-gl-errors] glGetError() = {:#x} after host GLES call \
                 dispatched from {}:{}",
                err,
                caller.file(),
                caller.line()
            );
        }
    }
    #[allow(clippy::let_and_return)]
    res
}

#[track_caller]
fn with_ctx_and_mem_no_skip<T, U: Default>(env: &mut Environment, f: T) -> U
where
    T: FnOnce(&mut dyn GLES, &mut Mem) -> U,
{
    let trace = env.options.trace_gl_errors;
    let caller = std::panic::Location::caller();
    let _stutter_span=crate::ds_stutter_probe::Span::new(1,caller.line() as u64);
    // _no_skip historically panicked if there was no current context,
    // but real games (HyperHLE log #5 / Resident Evil 4) hit this on
    // worker threads that issue GL calls after `setCurrentContext:nil`.
    // Apple's documented behaviour is "GL calls silently fail" — mirror
    // that by returning the type's default instead of aborting the
    // emulator. The trade-off vs. with_ctx_and_mem is unchanged: we still
    // attempt the call when a context exists, even if it isn't the one
    // the guest expects.
    let Some(mut gles) = super::sync_context(
        &mut env.framework_state.opengles,
        &mut env.objc,
        env.window
            .as_mut()
            .expect("OpenGL ES is not supported in headless mode"),
        env.current_thread,
    ) else {
        log!(
            "Warning: with_ctx_and_mem_no_skip dispatched from {}:{} found \
             no current GL context; returning default.",
            caller.file(),
            caller.line()
        );
        return U::default();
    };
    let res = f(gles.as_mut(), &mut env.mem);
    if trace {
        let err = unsafe { gles.GetError() };
        if err != 0 {
            log!(
                "[--trace-gl-errors] glGetError() = {:#x} after host GLES call \
                 dispatched from {}:{}",
                err,
                caller.file(),
                caller.line()
            );
        }
    }
    #[allow(clippy::let_and_return)]
    res
}

fn glGetError(env: &mut Environment) -> GLenum {
    let ignore_gl_errors = env.options.ignore_gl_errors;
    with_ctx_and_mem(env, |gles, _mem| {
        let err = unsafe { gles.GetError() };
        if err != 0 {
            crate::ds_diagnostics::gl_error(err);
            if ignore_gl_errors {
                return 0;
            }
            // Many engines (Unity, Cocos2d, Mono Game) call glGetError
            // every frame as an instrumentation hook. Once an error is
            // sticky in the host GL state machine (e.g. a strict Mali
            // driver returns GL_INVALID_ENUM for a call that the
            // emulator tolerated), every subsequent app-side
            // glGetError gets the same code, flooding the log. We
            // remember which error codes we've already reported and
            // demote repeats of the same code to log_dbg so the
            // diagnostic information is preserved (developers can
            // still use `--trace-gl-errors` to find the originating
            // call) but the console isn't drowning in identical lines.
            //
            // The set is per-process (no thread synchronisation
            // needed because GL contexts are accessed serialised
            // through `with_ctx_and_mem`); we cap it at 16 distinct
            // codes which is more than the entire OpenGL ES error
            // enum space.
            use std::sync::atomic::{AtomicU32, Ordering};
            const MAX_REPORTED: usize = 16;
            static REPORTED: [AtomicU32; MAX_REPORTED] = [
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
                AtomicU32::new(0),
            ];
            let mut already_seen = false;
            for slot in &REPORTED {
                let cur = slot.load(Ordering::Relaxed);
                if cur == err {
                    already_seen = true;
                    break;
                }
                if cur == 0
                    && slot
                        .compare_exchange(0, err, Ordering::Relaxed, Ordering::Relaxed)
                        .is_ok()
                {
                    break;
                }
            }
            if already_seen {
                log_dbg!("glGetError() returned {:#x} (already reported)", err);
            } else {
                log!(
                    "Warning: glGetError() returned {:#x} (subsequent repeats \
                     of the same code are silenced; rerun with \
                     --trace-gl-errors to identify the originating GL call)",
                    err
                );
            }
        }
        err
    })
}
fn glEnable(env: &mut Environment, cap: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Enable(cap) });
}
fn glIsEnabled(env: &mut Environment, cap: GLenum) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.IsEnabled(cap) })
}
fn glDisable(env: &mut Environment, cap: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Disable(cap) });
}
fn glClientActiveTexture(env: &mut Environment, texture: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.ClientActiveTexture(texture)
    })
}
fn glEnableClientState(env: &mut Environment, array: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.EnableClientState(array) });
}
fn glDisableClientState(env: &mut Environment, array: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.DisableClientState(array) });
}

fn glGetBooleanv(env: &mut Environment, pname: GLenum, params: MutPtr<GLboolean>) {
    if env
        .framework_state
        .opengles
        .effective_ctx_missing(env.current_thread)
    {
        env.mem.write(params, 0);
        return;
    }
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 16);
        unsafe { gles.GetBooleanv(pname, params) };
    });
}
fn glGetFloatv(env: &mut Environment, pname: GLenum, params: MutPtr<GLfloat>) {
    if env
        .framework_state
        .opengles
        .effective_ctx_missing(env.current_thread)
    {
        env.mem.write(params, 0.0);
        return;
    }
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 16);
        unsafe { gles.GetFloatv(pname, params) };
    });
}
fn glGetIntegerv(env: &mut Environment, pname: GLenum, params: MutPtr<GLint>) {
    match pname {
        gles11::NUM_COMPRESSED_TEXTURE_FORMATS => {
            env.mem
                .write(params, SUPPORTED_COMPRESSED_TEXTURE_FORMATS.len() as _);
        }
        gles11::COMPRESSED_TEXTURE_FORMATS => {
            for (idx, &format) in SUPPORTED_COMPRESSED_TEXTURE_FORMATS.iter().enumerate() {
                env.mem.write(params + idx as GuestUSize, format as _);
            }
        }
        0x8cdf | 0x8d57 => {
            env.mem.write(params, 1 as _);
        }
        gles11::FRAMEBUFFER_BINDING_OES => {
            // `glBindFramebufferOES(..., 0)` is redirected to the drawable
            // alias FBO, so report 0 back to the guest for that object.
            if env
                .framework_state
                .opengles
                .effective_ctx_missing(env.current_thread)
            {
                env.mem.write(params, 0);
                return;
            }
            let alias = drawable_fbo_of_current_ctx(env);
            with_ctx_and_mem(env, |gles, mem| {
                let mut value: GLint = 0;
                unsafe { gles.GetIntegerv(pname, &mut value) };
                if alias == Some(value as GLuint) {
                    value = 0;
                }
                mem.write(params, value);
            })
        }
        gles11::MAX_TEXTURE_SIZE => {
            env.mem.write(params, 2048 as _);
        }
        _ => {
            if env
                .framework_state
                .opengles
                .effective_ctx_missing(env.current_thread)
            {
                env.mem.write(params, 1);
                return;
            }
            with_ctx_and_mem(env, |gles, mem| {
                let params = mem.ptr_at_mut(params, 16);
                unsafe { gles.GetIntegerv(pname, params) };
            });
        }
    }
}
fn glGetFixedv(env: &mut Environment, pname: GLenum, params: MutPtr<GLfixed>) {
    if env
        .framework_state
        .opengles
        .effective_ctx_missing(env.current_thread)
    {
        env.mem.write(params, 0);
        return;
    }
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 16);
        unsafe { gles.GetFixedv(pname, params) };
    });
}
fn glGetPointerv(env: &mut Environment, pname: GLenum, params: MutPtr<ConstVoidPtr>) {
    if env
        .framework_state
        .opengles
        .effective_ctx_missing(env.current_thread)
    {
        env.mem.write(params, Ptr::null());
        return;
    }
    use crate::gles::gles1_on_gl2::{ArrayInfo, ARRAYS};
    let &ArrayInfo { buffer_binding, .. } =
        ARRAYS.iter().find(|info| info.pointer == pname).unwrap();
    with_ctx_and_mem(env, |gles, mem| {
        let mut host_pointer_or_offset = std::ptr::null();
        let guest_pointer_or_offset = unsafe {
            gles.GetPointerv(pname, &mut host_pointer_or_offset);
            translate_pointer_or_offset_to_guest(gles, mem, host_pointer_or_offset, buffer_binding)
        };
        mem.write(params, guest_pointer_or_offset);
    });
}
fn glGetTexEnviv(env: &mut Environment, target: GLenum, pname: GLenum, params: MutPtr<GLint>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 16);
        unsafe { gles.GetTexEnviv(target, pname, params) };
    });
}
fn glGetTexEnvfv(env: &mut Environment, target: GLenum, pname: GLenum, params: MutPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 16);
        unsafe { gles.GetTexEnvfv(target, pname, params) };
    });
}

fn glHint(env: &mut Environment, target: GLenum, mode: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Hint(target, mode) })
}
fn glFinish(env: &mut Environment) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Finish() })
}
fn glFlush(env: &mut Environment) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Flush() })
}
fn glGetString(env: &mut Environment, name: GLenum) -> ConstPtr<GLubyte> {
    // Per Apple's `EAGLContext` / OpenGL ES Programming Guide, the values
    // returned by `glGetString(GL_VERSION)` and `GL_SHADING_LANGUAGE_VERSION`
    // depend on which `kEAGLRenderingAPI` the context was created with:
    //
    //   * `kEAGLRenderingAPIOpenGLES1` – reports a `"OpenGL ES-CM 1.1 …"`
    //     version string (the `-CM` denotes the Common profile) and no
    //     `GL_SHADING_LANGUAGE_VERSION` entry.
    //   * `kEAGLRenderingAPIOpenGLES2` – reports `"OpenGL ES 2.0 …"` (note:
    //     NO `-CM`) plus `GL_SHADING_LANGUAGE_VERSION == "OpenGL ES GLSL ES
    //     1.00 …"`. Apps such as Unity 3.5 (Bad Piggies, iOS 4.0 build) use
    //     this string to decide whether to follow their ES 2.0 renderer code
    //     path. If we return the ES-CM 1.1 string while serving an ES 2.0
    //     context, Unity leaves itself in a half-initialised state and the
    //     resulting frames look torn / overlapped on screen.
    //
    // We therefore branch on the *currently-bound context's* API instead of
    // hard-coding the ES 1.1 strings. The cache is keyed by `(is_es2, name)`
    // because the two profiles share the same `name` enum values.
    let is_es2 = with_ctx_and_mem(env, |gles, _mem| gles.is_es2());

    if let Some(&str) = env
        .framework_state
        .opengles
        .strings_cache
        .get(&(is_es2, name))
    {
        return str;
    }

    let s: &[u8] = if !is_es2 {
        match name {
            gles11::VENDOR => b"Imagination Technologies",
            gles11::RENDERER => b"PowerVR MBXLite with VGPLite",
            gles11::VERSION => b"OpenGL ES-CM 1.1 (76)",
            // Includes GL_OES_matrix_palette: the real PowerVR SGX in the
            // iPhone 3GS exposes it, and touchHLE now emulates palette
            // skinning CPU-side (see gles1_on_gl2's skin_vertices). Games such
            // as LEGO Ninjago: Rise of the Snakes feature-test this string and
            // use the palette path for skinned character meshes.
            gles11::EXTENSIONS => b"GL_APPLE_framebuffer_multisample GL_APPLE_texture_max_level GL_EXT_discard_framebuffer GL_EXT_texture_filter_anisotropic GL_EXT_texture_lod_bias GL_IMG_read_format GL_IMG_texture_compression_pvrtc GL_IMG_texture_format_BGRA8888 GL_OES_blend_subtract GL_OES_compressed_paletted_texture GL_OES_depth24 GL_OES_draw_texture GL_OES_framebuffer_object GL_OES_mapbuffer GL_OES_matrix_palette GL_OES_point_size_array GL_OES_point_sprite GL_OES_read_format GL_OES_rgb8_rgba8 GL_OES_texture_mirrored_repeat GL_OES_vertex_array_object ",
            _ => b"Unknown",
        }
    } else {
        // Strings reported by an iPhone 3GS / iPhone 4 running iOS 4.0 in an
        // `kEAGLRenderingAPIOpenGLES2` context. Crucially the `VERSION`
        // string does NOT contain the `-CM` Common-profile suffix, and the
        // `SHADING_LANGUAGE_VERSION` query is supported.
        match name {
            gles11::VENDOR => b"Imagination Technologies",
            gles11::RENDERER => b"PowerVR SGX 535",
            gles11::VERSION => b"OpenGL ES 2.0 IMGSGX535-63.27",
            // `GL_SHADING_LANGUAGE_VERSION` is 0x8B8C in both ES 2.0 and
            // desktop GL; reference it by numeric literal so we don't have
            // to pull in the ES 2.0 enum table here.
            0x8B8C => b"OpenGL ES GLSL ES 1.00",
            gles11::EXTENSIONS => b"GL_APPLE_framebuffer_multisample GL_APPLE_texture_max_level GL_EXT_debug_label GL_EXT_discard_framebuffer GL_EXT_texture_filter_anisotropic GL_EXT_texture_lod_bias GL_IMG_read_format GL_IMG_texture_compression_pvrtc GL_IMG_texture_format_BGRA8888 GL_OES_depth24 GL_OES_depth_texture GL_OES_packed_depth_stencil GL_OES_rgb8_rgba8 GL_OES_standard_derivatives GL_OES_texture_float GL_OES_texture_half_float GL_OES_vertex_array_object GL_OES_vertex_half_float ",
            _ => b"Unknown",
        }
    };
    let new_str = env.mem.alloc_and_write_cstr(s).cast_const();
    env.framework_state
        .opengles
        .strings_cache
        .insert((is_es2, name), new_str);
    new_str
}

fn glAlphaFunc(env: &mut Environment, func: GLenum, ref_: GLclampf) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.AlphaFunc(func, ref_) })
}
fn glAlphaFuncx(env: &mut Environment, func: GLenum, ref_: GLclampx) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.AlphaFuncx(func, ref_) })
}
fn glBlendFunc(env: &mut Environment, sfactor: GLenum, dfactor: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.BlendFunc(sfactor, dfactor)
    })
}
fn glBlendEquationOES(env: &mut Environment, mode: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.BlendEquationOES(mode) })
}

fn glColorMask(
    env: &mut Environment,
    red: GLboolean,
    green: GLboolean,
    blue: GLboolean,
    alpha: GLboolean,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.ColorMask(red, green, blue, alpha)
    })
}
fn glClipPlanef(env: &mut Environment, plane: GLenum, equation: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let equation = mem.ptr_at(equation, 4);
        unsafe { gles.ClipPlanef(plane, equation) }
    })
}
fn glClipPlanex(env: &mut Environment, plane: GLenum, equation: ConstPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let equation = mem.ptr_at(equation, 4);
        unsafe { gles.ClipPlanex(plane, equation) }
    })
}
fn glCullFace(env: &mut Environment, mode: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.CullFace(mode) })
}
fn glDepthFunc(env: &mut Environment, func: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.DepthFunc(func) })
}
fn glDepthMask(env: &mut Environment, flag: GLboolean) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.DepthMask(flag) })
}
fn glDepthRangef(env: &mut Environment, near: GLclampf, far: GLclampf) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.DepthRangef(near, far) })
}
fn glDepthRangex(env: &mut Environment, near: GLclampx, far: GLclampx) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.DepthRangex(near, far) })
}
fn glFrontFace(env: &mut Environment, mode: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.FrontFace(mode) })
}
fn glPolygonOffset(env: &mut Environment, factor: GLfloat, units: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.PolygonOffset(factor, units)
    })
}
fn glPolygonOffsetx(env: &mut Environment, factor: GLfixed, units: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.PolygonOffsetx(factor, units)
    })
}
fn glSampleCoverage(env: &mut Environment, value: GLclampf, invert: GLboolean) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.SampleCoverage(value, invert)
    })
}
fn glSampleCoveragex(env: &mut Environment, value: GLclampx, invert: GLboolean) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.SampleCoveragex(value, invert)
    })
}
fn glShadeModel(env: &mut Environment, mode: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.ShadeModel(mode) })
}
fn glScissor(env: &mut Environment, x: GLint, y: GLint, width: GLsizei, height: GLsizei) {
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glScissor({}, {}, {}, {}) [this log will only be shown once]",
                x,
                y,
                width,
                height
            );
        }
    }
    // scale-hack enlarges only the EAGL drawable. Render-to-texture targets
    // already have the exact dimensions the guest requested (Dead Space's
    // pickup popup is one example). Scaling their scissor box makes the game
    // render only into the lower-left 1/scale_hack² of the offscreen texture.
    let factor = scale_hack_for_current_framebuffer(env);
    let (x, y) = (x * factor, y * factor);
    let (width, height) = (width * factor, height * factor);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Scissor(x, y, width, height)
    })
}
fn glViewport(env: &mut Environment, x: GLint, y: GLint, width: GLsizei, height: GLsizei) {
    // ULTRAHLE_MINIONJUMP_VIEWPORT_BEGIN
    let (x, y, width, height) = if matches!(
        env.bundle.bundle_identifier(),
        "com.apprisetec9.minionjump" | "com.risinghighapps.kingdomprincepro"
    ) && x == 0
        && y == 0
        && width == 768
        && height == 1024
    {
        log!("UltraHLE MinionJump: viewport swap 768x1024 -> 1024x768");
        (0, 0, 1024, 768)
    } else if std::env::var_os("TOUCHHLE_FORCE_IPAD_LANDSCAPE_SCREEN").is_some()
        && x == 0
        && y == 0
        && width == 768
        && height == 1024
    {
        log!("UltraHLE MinionJump: env iPad landscape viewport swap 768x1024 -> 1024x768");
        (0, 0, 1024, 768)
    } else {
        (x, y, width, height)
    };
    // ULTRAHLE_MINIONJUMP_VIEWPORT_END
    let (mut x, mut y, mut width, mut height) = (x, y, width, height);

    if std::env::var_os("TOUCHHLE_FORCE_LANDSCAPE_VIEWPORT").is_some() {
        // PotatoGold/adrastea-style landscape apps can end up with a 20px
        // status-bar-shortened portrait-derived viewport, e.g. 460x320,
        // even after UIScreen/EAGL have been made landscape. That leaves the
        // final frame cropped/scuffed. In this compatibility mode, promote
        // the common iPhone landscape viewport cases to the full 480x320
        // logical viewport.
        let should_force = (x == 0 && y == 0 && width == 460 && height == 320)
            || (x == 0 && y == 0 && width == 320 && height == 460)
            || (x == 0 && y == 0 && width == 320 && height == 480);

        if should_force {
            log!(
                "TOUCHHLE_FORCE_LANDSCAPE_VIEWPORT=1: overriding glViewport({}, {}, {}, {}) to glViewport(0, 0, 480, 320)",
                x,
                y,
                width,
                height
            );
            x = 0;
            y = 0;
            width = 480;
            height = 320;
        }
    }

    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glViewport({}, {}, {}, {}) on thread {} [this log will only be shown once]",
                x,
                y,
                width,
                height,
                env.current_thread
            );
        }
    }
    // See glScissor above. Applying scale-hack to an offscreen viewport makes
    // layout-generated UI textures contain only their lower-left quadrant,
    // even though the render target itself is correctly sized.
    let factor = scale_hack_for_current_framebuffer(env);
    let (x, y) = (x * factor, y * factor);
    let (width, height) = (width * factor, height * factor);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Viewport(x, y, width, height)
    })
}
fn glLineWidth(env: &mut Environment, val: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.LineWidth(val) })
}
fn glLineWidthx(env: &mut Environment, val: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.LineWidthx(val) })
}
fn glStencilFunc(env: &mut Environment, func: GLenum, ref_: GLint, mask: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.StencilFunc(func, ref_, mask)
    });
}
fn glStencilOp(env: &mut Environment, sfail: GLenum, dpfail: GLenum, dppass: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.StencilOp(sfail, dpfail, dppass)
    });
}
fn glStencilMask(env: &mut Environment, mask: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.StencilMask(mask) });
}
fn glLogicOp(env: &mut Environment, opcode: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.LogicOp(opcode) });
}
fn glPointSize(env: &mut Environment, size: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.PointSize(size) })
}
fn glPointSizex(env: &mut Environment, size: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.PointSizex(size) })
}
fn glPointParameterf(env: &mut Environment, pname: GLenum, param: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.PointParameterf(pname, param)
    })
}
fn glPointParameterx(env: &mut Environment, pname: GLenum, param: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.PointParameterx(pname, param)
    })
}
fn glPointParameterfv(env: &mut Environment, pname: GLenum, params: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.PointParameterfv(pname, params) }
    })
}
fn glPointParameterxv(env: &mut Environment, pname: GLenum, params: ConstPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.PointParameterxv(pname, params) }
    })
}

/// Dead Space debug aid: `TOUCHHLE_DS_LOG_FOG_SET=1` logs the fog parameters the
/// guest actually passes, deduplicated, so they can be compared with what the
/// driver ends up holding.
fn ds_log_fog_set(pname: GLenum, value: GLfloat) {
    if std::env::var_os("TOUCHHLE_DS_LOG_FOG_SET").is_none() {
        return;
    }
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let key = format!("pname=0x{:x} value={}", pname, value);
    let set = SEEN.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    let mut set = set.lock().unwrap();
    if set.insert(key.clone()) {
        log!("[DSfogset] {}", key);
    }
}

fn glFogf(env: &mut Environment, pname: GLenum, param: GLfloat) {
    ds_log_fog_set(pname, param);
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Fogf(pname, param) })
}
fn glFogx(env: &mut Environment, pname: GLenum, param: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Fogx(pname, param) })
}
fn glFogfv(env: &mut Environment, pname: GLenum, params: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe {
            let base = params as *const GLfloat;
            ds_log_fog_set(pname, *base);
            gles.Fogfv(pname, params)
        }
    })
}
fn glFogxv(env: &mut Environment, pname: GLenum, params: ConstPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.Fogxv(pname, params) }
    })
}
fn glLightf(env: &mut Environment, light: GLenum, pname: GLenum, param: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Lightf(light, pname, param)
    })
}
fn glLightx(env: &mut Environment, light: GLenum, pname: GLenum, param: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Lightx(light, pname, param)
    })
}
fn glLightfv(env: &mut Environment, light: GLenum, pname: GLenum, params: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.Lightfv(light, pname, params) }
    })
}
fn glLightxv(env: &mut Environment, light: GLenum, pname: GLenum, params: ConstPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.Lightxv(light, pname, params) }
    })
}
fn glLightModelf(env: &mut Environment, pname: GLenum, param: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.LightModelf(pname, param) })
}
fn glLightModelx(env: &mut Environment, pname: GLenum, param: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.LightModelx(pname, param) })
}
fn glLightModelfv(env: &mut Environment, pname: GLenum, params: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.LightModelfv(pname, params) }
    })
}
fn glLightModelxv(env: &mut Environment, pname: GLenum, params: ConstPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.LightModelxv(pname, params) }
    })
}
fn glMaterialf(env: &mut Environment, face: GLenum, pname: GLenum, param: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Materialf(face, pname, param)
    })
}
fn glMaterialx(env: &mut Environment, face: GLenum, pname: GLenum, param: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Materialx(face, pname, param)
    })
}
fn glMaterialfv(env: &mut Environment, face: GLenum, pname: GLenum, params: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.Materialfv(face, pname, params) }
    })
}
fn glMaterialxv(env: &mut Environment, face: GLenum, pname: GLenum, params: ConstPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.Materialxv(face, pname, params) }
    })
}

fn glIsBuffer(env: &mut Environment, buffer: GLuint) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.IsBuffer(buffer) })
}
fn glGenBuffers(env: &mut Environment, n: GLsizei, buffers: MutPtr<GLuint>) {
    if env
        .framework_state
        .opengles
        .effective_ctx_missing(env.current_thread)
    {
        for i in 0..n {
            env.mem
                .write(buffers + (i as GuestUSize), (i + 1) as GLuint);
        }
        return;
    }
    with_ctx_and_mem(env, |gles, mem| {
        let n_usize: GuestUSize = n.try_into().unwrap();
        let buffers = mem.ptr_at_mut(buffers, n_usize);
        unsafe { gles.GenBuffers(n, buffers) }
    })
}
fn glDeleteBuffers(env: &mut Environment, n: GLsizei, buffers: ConstPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| {
        let n_usize: GuestUSize = n.try_into().unwrap();
        let buffers = mem.ptr_at(buffers, n_usize);
        unsafe { gles.DeleteBuffers(n, buffers) }
    })
}
fn glBindBuffer(env: &mut Environment, target: GLenum, buffer: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.BindBuffer(target, buffer) })
}
fn glBufferData(
    env: &mut Environment,
    target: GLenum,
    size: GuestGLsizeiptr,
    data: ConstPtr<GLvoid>,
    usage: GLenum,
) {
    let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::BufferUpload,if data.is_null(){0}else{size.max(0) as u64},target as u64);
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let data: *const GLvoid = if data.is_null() {
            std::ptr::null()
        } else {
            mem.ptr_at(data.cast::<u8>(), size.try_into().unwrap())
                .cast()
        };
        gles.BufferData(target, size as HostGLsizeiptr, data, usage);
        if std::env::var_os("TOUCHHLE_DS_BUFFER_UPLOAD_FINISH").is_some() {
            gles.Finish();
            log_once!("TOUCHHLE_DS_BUFFER_UPLOAD_FINISH: synchronizing buffer uploads");
        }
    })
}
fn glBufferSubData(
    env: &mut Environment,
    target: GLenum,
    offset: GuestGLintptr,
    size: GuestGLsizeiptr,
    data: ConstPtr<GLvoid>,
) {
    let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::BufferUpload,if data.is_null(){0}else{size.max(0) as u64},target as u64);
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let data = if data.is_null() {
            std::ptr::null()
        } else {
            mem.ptr_at(data.cast::<u8>(), size.try_into().unwrap())
                .cast()
        };
        gles.BufferSubData(target, offset as HostGLintptr, size as HostGLsizeiptr, data);
        if std::env::var_os("TOUCHHLE_DS_BUFFER_UPLOAD_FINISH").is_some() {
            gles.Finish();
        }
    })
}

fn glColor4f(env: &mut Environment, red: GLfloat, green: GLfloat, blue: GLfloat, alpha: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Color4f(red, green, blue, alpha)
    })
}
fn glColor4x(env: &mut Environment, red: GLfixed, green: GLfixed, blue: GLfixed, alpha: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Color4x(red, green, blue, alpha)
    })
}
fn glColor4ub(env: &mut Environment, red: GLubyte, green: GLubyte, blue: GLubyte, alpha: GLubyte) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Color4ub(red, green, blue, alpha)
    })
}
fn glNormal3f(env: &mut Environment, nx: GLfloat, ny: GLfloat, nz: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Normal3f(nx, ny, nz) })
}
fn glNormal3x(env: &mut Environment, nx: GLfixed, ny: GLfixed, nz: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Normal3x(nx, ny, nz) })
}

unsafe fn translate_pointer_or_offset_to_host(
    gles: &mut dyn GLES,
    mem: &Mem,
    pointer_or_offset: ConstVoidPtr,
    which_binding: GLenum,
) -> *const GLvoid {
    let mut buffer_binding = 0;
    gles.GetIntegerv(which_binding, &mut buffer_binding);
    if buffer_binding != 0 {
        let offset = pointer_or_offset.to_bits();
        offset as usize as *const _
    } else if pointer_or_offset.is_null() {
        std::ptr::null()
    } else {
        mem.unchecked_ptr_at(pointer_or_offset.cast::<u8>(), 0)
            .cast::<GLvoid>()
    }
}
unsafe fn translate_pointer_or_offset_to_guest(
    gles: &mut dyn GLES,
    mem: &Mem,
    pointer_or_offset: *const GLvoid,
    which_binding: GLenum,
) -> ConstVoidPtr {
    let mut buffer_binding = 0;
    gles.GetIntegerv(which_binding, &mut buffer_binding);
    if buffer_binding != 0 {
        let offset = pointer_or_offset as usize;
        Ptr::from_bits(u32::try_from(offset).unwrap())
    } else if pointer_or_offset.is_null() {
        Ptr::null()
    } else {
        mem.host_ptr_to_guest_ptr(pointer_or_offset)
    }
}

fn glColorPointer(
    env: &mut Environment,
    size: GLint,
    type_: GLenum,
    stride: GLsizei,
    pointer: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let pointer =
            translate_pointer_or_offset_to_host(gles, mem, pointer, gles11::ARRAY_BUFFER_BINDING);
        gles.ColorPointer(size, type_, stride, pointer)
    })
}
fn glNormalPointer(env: &mut Environment, type_: GLenum, stride: GLsizei, pointer: ConstVoidPtr) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let pointer =
            translate_pointer_or_offset_to_host(gles, mem, pointer, gles11::ARRAY_BUFFER_BINDING);
        gles.NormalPointer(type_, stride, pointer)
    })
}
fn glTexCoordPointer(
    env: &mut Environment,
    size: GLint,
    type_: GLenum,
    stride: GLsizei,
    pointer: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let pointer =
            translate_pointer_or_offset_to_host(gles, mem, pointer, gles11::ARRAY_BUFFER_BINDING);
        // Same frame gate as `log_ds_texture_draw`: this dump is emitted per
        // texture-coordinate-pointer call, so on a long run it dominates both
        // the log size and the run time (measured: 200k lines / 54 MB and a
        // ~2-4x slowdown). Keeping it behind the same gate is what makes a
        // late-frame capture affordable at all.
        let trace_now = std::env::var_os("TOUCHHLE_DS_TEXTURE_TRACE").is_some()
            && std::env::var("TOUCHHLE_DS_TRACE_FROM_FRAME")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map_or(true, |from| {
                    crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed)
                        >= from
                });
        if trace_now {
            let mut buffer_binding: GLint = 0;
            gles.GetIntegerv(gles11::ARRAY_BUFFER_BINDING, &mut buffer_binding);
            if buffer_binding == 0 && !pointer.is_null() {
                let raw: Vec<f64> = (0..8)
                    .map(|i| {
                        let p = pointer.cast::<u8>().add(i as usize * 2);
                        (p as *const i16).read_unaligned() as f64
                    })
                    .collect();
                let normalized: Vec<f32> = raw.iter().map(|v| (v / 32768.0) as f32).collect();
                log!(
                    "[DSTEX tcptr] size={} type=0x{:x} stride={} short_raw={:?} short_norm={:?}",
                    size,
                    type_,
                    stride,
                    raw,
                    normalized
                );
            } else {
                log!(
                    "[DSTEX tcptr] size={} type=0x{:x} stride={} buffer={} ptr={:?}",
                    size,
                    type_,
                    stride,
                    buffer_binding,
                    pointer
                );
            }
        }
        gles.TexCoordPointer(size, type_, stride, pointer)
    })
}
fn glVertexPointer(
    env: &mut Environment,
    size: GLint,
    type_: GLenum,
    stride: GLsizei,
    pointer: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let pointer =
            translate_pointer_or_offset_to_host(gles, mem, pointer, gles11::ARRAY_BUFFER_BINDING);
        gles.VertexPointer(size, type_, stride, pointer)
    })
}

/// `glPointSizePointerOES` (`GL_OES_point_size_array`). Forwards through the
/// GLES trait so per-vertex point sizes are applied by backends that support
/// them (native ES 1.1).
fn glPointSizePointerOES(
    env: &mut Environment,
    type_: GLenum,
    stride: GLsizei,
    pointer: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let pointer =
            translate_pointer_or_offset_to_host(gles, mem, pointer, gles11::ARRAY_BUFFER_BINDING);
        gles.PointSizePointerOES(type_, stride, pointer)
    })
}

fn glGetTexParameteriv(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 1);
        unsafe { gles.GetTexParameteriv(target, pname, params) }
    })
}
fn glGetTexParameterfv(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: MutPtr<GLfloat>,
) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 1);
        unsafe { gles.GetTexParameterfv(target, pname, params) }
    })
}
fn glGetTexParameterxv(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: MutPtr<GLfixed>,
) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 1);
        unsafe { gles.GetTexParameterxv(target, pname, params) }
    })
}
fn glGetTexEnvxv(env: &mut Environment, target: GLenum, pname: GLenum, params: MutPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 16);
        unsafe { gles.GetTexEnvxv(target, pname, params) }
    })
}
fn glGetClipPlanef(env: &mut Environment, plane: GLenum, equation: MutPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let equation = mem.ptr_at_mut(equation, 4);
        unsafe { gles.GetClipPlanef(plane, equation) }
    })
}
fn glGetClipPlanex(env: &mut Environment, plane: GLenum, equation: MutPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let equation = mem.ptr_at_mut(equation, 4);
        unsafe { gles.GetClipPlanex(plane, equation) }
    })
}
fn glGetLightfv(env: &mut Environment, light: GLenum, pname: GLenum, params: MutPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 4);
        unsafe { gles.GetLightfv(light, pname, params) }
    })
}
fn glGetLightxv(env: &mut Environment, light: GLenum, pname: GLenum, params: MutPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 4);
        unsafe { gles.GetLightxv(light, pname, params) }
    })
}
fn glGetMaterialfv(env: &mut Environment, face: GLenum, pname: GLenum, params: MutPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 4);
        unsafe { gles.GetMaterialfv(face, pname, params) }
    })
}
fn glGetMaterialxv(env: &mut Environment, face: GLenum, pname: GLenum, params: MutPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 4);
        unsafe { gles.GetMaterialxv(face, pname, params) }
    })
}

fn glCompressedTexSubImage2D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    xoffset: GLint,
    yoffset: GLint,
    width: GLsizei,
    height: GLsizei,
    format: GLenum,
    image_size: GLsizei,
    data: ConstVoidPtr,
) {
    if let Some(state)=super::eagl::shader_compat_state(env) {state.borrow().invalidate_upload(target,level);}

    with_ctx_and_mem(env, |gles, mem| unsafe {
        let data = mem
            .ptr_at(data.cast::<u8>(), image_size.try_into().unwrap())
            .cast();
        gles.CompressedTexSubImage2D(
            target, level, xoffset, yoffset, width, height, format, image_size, data,
        )
    })
}

fn glDrawTexfOES(
    env: &mut Environment,
    x: GLfloat,
    y: GLfloat,
    z: GLfloat,
    width: GLfloat,
    height: GLfloat,
) {
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glDrawTexfOES({}, {}, {}, {}, {}) [this log will only be shown once]",
                x,
                y,
                z,
                width,
                height
            );
        }
    }
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.DrawTexfOES(x, y, z, width, height)
    })
}
fn glDrawTexiOES(env: &mut Environment, x: GLint, y: GLint, z: GLint, width: GLint, height: GLint) {
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glDrawTexiOES({}, {}, {}, {}, {}) [this log will only be shown once]",
                x,
                y,
                z,
                width,
                height
            );
        }
    }
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.DrawTexiOES(x, y, z, width, height)
    })
}
fn glDrawTexxOES(
    env: &mut Environment,
    x: GLfixed,
    y: GLfixed,
    z: GLfixed,
    width: GLfixed,
    height: GLfixed,
) {
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glDrawTexxOES({}, {}, {}, {}, {}) [this log will only be shown once]",
                x,
                y,
                z,
                width,
                height
            );
        }
    }
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.DrawTexxOES(x, y, z, width, height)
    })
}
fn glDrawTexfvOES(env: &mut Environment, coords: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let coords = mem.ptr_at(coords, 5);
        unsafe { gles.DrawTexfvOES(coords) }
    })
}
fn glDrawTexivOES(env: &mut Environment, coords: ConstPtr<GLint>) {
    with_ctx_and_mem(env, |gles, mem| {
        let coords = mem.ptr_at(coords, 5);
        unsafe { gles.DrawTexivOES(coords) }
    })
}
fn glDrawTexxvOES(env: &mut Environment, coords: ConstPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let coords = mem.ptr_at(coords, 5);
        unsafe { gles.DrawTexxvOES(coords) }
    })
}
fn glDrawTexsOES(env: &mut Environment, x: i16, y: i16, z: i16, width: i16, height: i16) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.DrawTexsOES(x, y, z, width, height)
    })
}
fn glDrawTexsvOES(env: &mut Environment, coords: ConstPtr<i16>) {
    with_ctx_and_mem(env, |gles, mem| {
        let coords = mem.ptr_at(coords, 5);
        unsafe { gles.DrawTexsvOES(coords) }
    })
}
fn glRenderbufferStorageMultisampleAPPLE(
    env: &mut Environment,
    target: GLenum,
    samples: GLsizei,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
) {
    // Apply --scale-hack so an MSAA renderbuffer matches the size of the
    // single-sample one it'll be resolved into.
    let factor = env.options.scale_hack.get() as GLsizei;
    let (width, height) = (width * factor, height * factor);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.RenderbufferStorageMultisampleAPPLE(target, samples, internalformat, width, height)
    })
}
fn glResolveMultisampleFramebufferAPPLE(env: &mut Environment) {
    // Apple's MSAA pattern: the app binds the sample (multisample) framebuffer
    // to GL_READ_FRAMEBUFFER_APPLE and the resolve (single-sample) framebuffer
    // (whose color renderbuffer is the CAEAGLLayer drawable) to
    // GL_DRAW_FRAMEBUFFER_APPLE, then calls this to copy the resolved pixels
    // into the drawable. Without this copy the resolve renderbuffer stays
    // empty and `presentRenderbuffer:` has nothing to display.
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.ResolveMultisampleFramebufferAPPLE()
    })
}
fn glDiscardFramebufferEXT(
    env: &mut Environment,
    _target: GLenum,
    _numAttachments: GLsizei,
    _attachments: ConstPtr<GLenum>,
) {
    with_ctx_and_mem(env, |_gles, _mem| {
        // GL_EXT_discard_framebuffer is a hint; safe to ignore.
    })
}

/// `glPushGroupMarkerEXT` — debug marker from `GL_EXT_debug_marker`.
/// No-op on hosts that don't expose the extension.
fn glPushGroupMarkerEXT(_env: &mut Environment, _length: GLsizei, _marker: ConstPtr<u8>) {
    // Debug markers are hints; safe to ignore.
}

/// `glPopGroupMarkerEXT` — debug marker from `GL_EXT_debug_marker`.
/// No-op on hosts that don't expose the extension.
fn glPopGroupMarkerEXT(_env: &mut Environment) {
    // Debug markers are hints; safe to ignore.
}

fn glBindVertexArrayOES(env: &mut Environment, array: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        if gles.supports_vao_oes() {
            gles.BindVertexArrayOES(array);
        }
        // Otherwise no-op: without real VAO support all vertex state lives in
        // the single default array object, so there is nothing to switch.
    });
}
fn glDeleteVertexArraysOES(env: &mut Environment, n: GLsizei, arrays: ConstPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        if gles.supports_vao_oes() {
            let slice = mem.bytes_at(arrays.cast(), (n.max(0) as GuestUSize) * 4);
            gles.DeleteVertexArraysOES(n, slice.as_ptr().cast());
        }
    });
}
fn glGenVertexArraysOES(env: &mut Environment, n: GLsizei, arrays: MutPtr<GLuint>) {
    let supported = with_ctx_and_mem(env, |gles, _mem| gles.supports_vao_oes());
    if supported {
        with_ctx_and_mem(env, |gles, mem| unsafe {
            let slice = mem.bytes_at_mut(arrays.cast(), (n.max(0) as GuestUSize) * 4);
            gles.GenVertexArraysOES(n, slice.as_mut_ptr().cast());
        });
    } else {
        // Fallback emulation: just hand out sequential non-zero names.
        for i in 0..n {
            env.mem.write(arrays + (i as GuestUSize), (i + 1) as GLuint);
        }
    }
}
fn glIsVertexArrayOES(env: &mut Environment, array: GLuint) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        if gles.supports_vao_oes() {
            gles.IsVertexArrayOES(array)
        } else {
            0
        }
    })
}
fn glCurrentPaletteMatrixOES(env: &mut Environment, matrixpaletteindex: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.CurrentPaletteMatrixOES(matrixpaletteindex)
    })
}
fn glLoadPaletteFromModelViewMatrixOES(env: &mut Environment) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.LoadPaletteFromModelViewMatrixOES()
    })
}
fn glMatrixIndexPointerOES(
    env: &mut Environment,
    size: GLint,
    type_: GLenum,
    stride: GLsizei,
    pointer: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let pointer =
            translate_pointer_or_offset_to_host(gles, mem, pointer, gles11::ARRAY_BUFFER_BINDING);
        gles.MatrixIndexPointerOES(size, type_, stride, pointer)
    })
}
fn glWeightPointerOES(
    env: &mut Environment,
    size: GLint,
    type_: GLenum,
    stride: GLsizei,
    pointer: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let pointer =
            translate_pointer_or_offset_to_host(gles, mem, pointer, gles11::ARRAY_BUFFER_BINDING);
        gles.WeightPointerOES(size, type_, stride, pointer)
    })
}
fn glGetBufferPointervOES(
    env: &mut Environment,
    _target: GLenum,
    _pname: GLenum,
    params: MutPtr<ConstVoidPtr>,
) {
    env.mem.write(params, Ptr::null());
}

/// Guard against a whole-emulator crash caused by enabled *client-side* vertex
/// attribute arrays whose pointer is not actually inside guest memory.
///
/// When no buffer object is bound for an attribute array, the host GL driver
/// reads the vertex data straight from the pointer touchHLE gave it. That
/// pointer is only safe if it addresses guest memory. A guest can leave an
/// array enabled with a bogus pointer — e.g. a stale offset into a vertex
/// buffer that was unbound or deleted before the draw (observed in My Talking
/// Tom: tapping Tom issues a glDrawElements whose attribute #3 still carries the
/// raw offset 0x9c0). The driver (both Mesa/llvmpipe and real GPU drivers) then
/// dereferences that wild address and segfaults the process.
///
/// To stay robust we check every enabled array that has no buffer bound and
/// temporarily disable any whose pointer falls outside guest memory, restoring
/// them after the draw. The draw then renders with default attribute values
/// instead of taking the whole emulator down.
///
/// This guard relies on vertex-attrib query entry points that are not valid on
/// native ES 1.1 backends. On real ES 1.1 drivers (e.g. Qualcomm Adreno) those
/// queries return GL_INVALID_OPERATION and poison the error queue, so we only
/// apply this on the GLES1-on-GL2 emulation backend where the queries are
/// supported.
unsafe fn guard_client_vertex_arrays(gles: &mut dyn GLES, mem: &Mem) -> Vec<GLuint> {
    if gles.is_native_es1() {
        return Vec::new();
    }

    const VERTEX_ATTRIB_ARRAY_ENABLED: GLenum = 0x8622;
    const VERTEX_ATTRIB_ARRAY_BUFFER_BINDING: GLenum = 0x889F;
    const VERTEX_ATTRIB_ARRAY_POINTER: GLenum = 0x8645;
    const MAX_VERTEX_ATTRIBS: GLenum = 0x8869;

    let mut max_attribs: GLint = 0;
    gles.GetIntegerv(MAX_VERTEX_ATTRIBS, &mut max_attribs);
    // Be defensive about a backend that doesn't answer the query.
    let max_attribs = if (1..=64).contains(&max_attribs) {
        max_attribs as GLuint
    } else {
        16
    };

    let mut disabled = Vec::new();
    for index in 0..max_attribs {
        let mut enabled: GLint = 0;
        gles.GetVertexAttribiv(index, VERTEX_ATTRIB_ARRAY_ENABLED, &mut enabled);
        if enabled == 0 {
            continue;
        }
        let mut bound: GLint = 0;
        gles.GetVertexAttribiv(index, VERTEX_ATTRIB_ARRAY_BUFFER_BINDING, &mut bound);
        if bound != 0 {
            // Data comes from a buffer object; the driver handles bounds.
            continue;
        }
        let mut ptr: *mut GLvoid = std::ptr::null_mut();
        gles.GetVertexAttribPointerv(index, VERTEX_ATTRIB_ARRAY_POINTER, &mut ptr);
        if ptr.is_null() {
            // A null client pointer is the OpenGL default for an array that
            // has not been populated yet. Keep the enabled array state intact:
            // the fixed-function backend supplies its normal default attribute
            // values, while disabling it changes the guest-visible state and
            // can make later draws lose their vertex streams.
            continue;
        }
        if mem.is_host_ptr_in_guest_mem(ptr) {
            // A legitimate client-side array pointing into guest memory.
            continue;
        }
        log!(
            "Warning: disabling enabled vertex attribute array #{} for this draw: \
             no buffer is bound and its client pointer {:?} is outside guest memory \
             (would crash the host GL driver)",
            index,
            ptr
        );
        gles.DisableVertexAttribArray(index);
        disabled.push(index);
    }
    disabled
}

fn glDrawArrays(env: &mut Environment, mode: GLenum, first: GLint, count: GLsizei) {
    let shader_fog_key=super::eagl::shader_no_fog_key(env);
    let _scene=crate::ds_scene_diagnostics::Scope::draw(crate::ds_scene_diagnostics::Kind::DrawArrays,count.max(0) as u64,mode as u64);
    {
        use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glDrawArrays(mode=0x{:x}, first={}, count={}) (app submitting first draw) [this log will only be shown once]",
                mode, first, count
            );
        }

        if trace_potatogold_render() {
            static COUNT: AtomicU32 = AtomicU32::new(0);
            let n = COUNT.fetch_add(1, Ordering::Relaxed);
            if n < 120 {
                log!(
                    "[POTATO RENDER TRACE] glDrawArrays #{} mode=0x{:x} first={} count={}",
                    n + 1,
                    mode,
                    first,
                    count
                );
            }
        }
    }
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let disabled_arrays = guard_client_vertex_arrays(gles, mem);
        let fog_state_backup = clamp_fog_state_values(gles);
        let shader_fog_saved=shader_fog_key.and_then(|key|crate::gles::ds_fog_scope::begin(gles,key));
        if std::env::var_os("TOUCHHLE_POTATO_NATIVE_GLES2_PC_STATE").is_some() {
            static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !SEEN.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log!(
                    "TOUCHHLE_POTATO_NATIVE_GLES2_PC_STATE=1: disabling strict native GLES2 scissor/depth/cull state before Potato draws [this log will only be shown once]"
                );
            }

            // PC desktop GL path is more forgiving about stale/clipping state.
            // Adreno native GLES2 can happily draw only a tiny clipped piece.
            gles.Disable(0x0c11); // GL_SCISSOR_TEST
            gles.Disable(0x0b71); // GL_DEPTH_TEST
            gles.Disable(0x0b44); // GL_CULL_FACE
        }

        gles.DrawArrays(mode, first, count);
        crate::gles::ds_fog_scope::end(gles,shader_fog_saved);
        restore_fog_state_values(gles, fog_state_backup);
        for index in disabled_arrays {
            gles.EnableVertexAttribArray(index);
        }
    })
}
fn glDrawElements(
    env: &mut Environment,
    mode: GLenum,
    count: GLsizei,
    type_: GLenum,
    indices: ConstVoidPtr,
) {
    let shader_fog_key=super::eagl::shader_no_fog_key(env);
    let _scene=crate::ds_scene_diagnostics::Scope::draw(crate::ds_scene_diagnostics::Kind::DrawElements,count.max(0) as u64,mode as u64);
    {
        use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glDrawElements(mode=0x{:x}, count={}, type=0x{:x}) (app submitting first indexed draw) [this log will only be shown once]",
                mode, count, type_
            );
        }

        if trace_potatogold_render() {
            static COUNT: AtomicU32 = AtomicU32::new(0);
            let n = COUNT.fetch_add(1, Ordering::Relaxed);
            if n < 160 {
                log!(
                    "[POTATO RENDER TRACE] glDrawElements #{} mode=0x{:x} count={} type=0x{:x} indices=0x{:x}",
                    n + 1,
                    mode,
                    count,
                    type_,
                    indices.to_bits()
                );
            }
        }
    }
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let disabled_arrays = guard_client_vertex_arrays(gles, mem);
        let fog_state_backup = clamp_fog_state_values(gles);
        let shader_fog_saved=shader_fog_key.and_then(|key|crate::gles::ds_fog_scope::begin(gles,key));
        if std::env::var_os("TOUCHHLE_POTATO_NATIVE_GLES2_PC_STATE").is_some() {
            static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !SEEN.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log!(
                    "TOUCHHLE_POTATO_NATIVE_GLES2_PC_STATE=1: disabling strict native GLES2 scissor/depth/cull state before Potato draws [this log will only be shown once]"
                );
            }

            // PC desktop GL path is more forgiving about stale/clipping state.
            // Adreno native GLES2 can happily draw only a tiny clipped piece.
            gles.Disable(0x0c11); // GL_SCISSOR_TEST
            gles.Disable(0x0b71); // GL_DEPTH_TEST
            gles.Disable(0x0b44); // GL_CULL_FACE
        }

        let indices = translate_pointer_or_offset_to_host(
            gles,
            mem,
            indices,
            gles11::ELEMENT_ARRAY_BUFFER_BINDING,
        );
        gles.DrawElements(mode, count, type_, indices);
        crate::gles::ds_fog_scope::end(gles,shader_fog_saved);
        restore_fog_state_values(gles, fog_state_backup);
        for index in disabled_arrays {
            gles.EnableVertexAttribArray(index);
        }
    })
}

/// Dead Space E-key aid: locate the pale-cyan *interaction ring* (door /
/// console / kinesis prompt) in the current colour buffer and return its centre
/// in landscape iOS points, plus the matched pixel count.
///
/// Calibrated from on-device screenshots (2026-10-08):
///   ring pixels are ~rgb(170,255,255)  (very bright, low saturation)
///   space background is rgb(25,107,149); suit lights and scan lines are
///   much smaller components, so the dominant-blob centroid picks the ring.
unsafe fn ds_find_interaction_ring(
    gles: &mut dyn GLES,
    portrait_pt_w: u32,
    family_override: Option<u8>,
) -> Option<(f64, f64, u32, Option<(f32, f32)>, (f64, f64))> {
    let mut vp = [0i32; 4];
    gles.GetIntegerv(gles11::VIEWPORT, vp.as_mut_ptr());
    let (w, h) = (vp[2], vp[3]);
    if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
        return None;
    }
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            echo!(
                "[DSe] ring finder: viewport={}x{} unrotated_pt_w={} (scale would be {:.2})",
                w,
                h,
                portrait_pt_w,
                if portrait_pt_w > 0 {
                    w as f64 / portrait_pt_w as f64
                } else {
                    1.0
                }
            );
        }
    }
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    gles.ReadPixels(
        0,
        0,
        w,
        h,
        gles11::RGBA,
        gles11::UNSIGNED_BYTE,
        buf.as_mut_ptr() as *mut _,
    );
    // Build the same landscape top-down image the frame dump produces with
    // ROTATE=90, so the centre maps directly to the DS-script point space.
    let (ow, oh) = (h, w);
    // Two prompt families (measured 2026-10-08):
    //   cyan  (G>=B) -> door/console ring, kinesis, O2, reload, stomp/chainsaw
    //   blue  (B>G)  -> QTE ring / QTE arrow (also survives the red damage tint)
    let family: u8 = family_override.unwrap_or_else(|| {
        std::env::var("TOUCHHLE_DS_PROMPT_FAMILY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    });
    let mut pts: Vec<(u32, u32)> = Vec::new();
    for oy in 0..oh {
        for ox in 0..ow {
            let (sx, sy) = (oy, h - 1 - ox);
            let idx = (((h - 1 - sy) * w + sx) * 4) as usize;
            let r = buf[idx] as i32;
            let g = buf[idx + 1] as i32;
            let b = buf[idx + 2] as i32;
            let cyan = g >= 200 && b >= 190 && g >= b && r <= 180;
            let blue = b >= 150 && b > r + 60 && b > g + 40;
            let hit = match family {
                1 => blue,
                2 => cyan,
                _ => cyan || blue,
            };
            if hit {
                pts.push((ox as u32, oy as u32));
            }
        }
    }
    // Threshold calibration (measured 2026-10-08, offline replay of this exact
    // colour criterion; see REPORT 13.162):
    //   * 10 real-device screenshots WITH a prompt (1.38 Mpx) -> the single
    //     largest *connected* cyan blob is 358..8705 px. The ring scales with
    //     distance, so the smallest real prompt is kinesis-far at 358 px
    //     (~159 px once scaled to our 614 kpx target).
    //   * emulator frames with NO prompt, incl. the cyan-bordered save-slot
    //     menu -> largest connected blob 15..38 px (the *total* mask can reach
    //     ~180 px, which is why a global count-based threshold cannot work).
    // 0.014% (>=80 px at 640x960) sits between the two with a ~2x margin each
    // way. Raise the absolute floor if a cyan-heavy scene ever false-fires.
    let screen_px = (w as usize) * (h as usize);
    let min_px = (screen_px / 7000).max(80);
    if pts.len() < min_px {
        return None;
    }
    // REAL connected components. The previous version counted every cyan pixel
    // within 300 px of a drifting centroid, so *scattered* cyan pixels (menu
    // borders, HUD) summed into a fake "area" and could false-fire - measured
    // on a live menu frame: reported area=124 while the largest *connected*
    // blob was only 38 px. Take the largest single blob instead, and require a
    // plausible ring-like shape (roughly square bounding box).
    let mut mask = vec![0u8; (ow as usize) * (oh as usize)];
    for &(x, y) in &pts {
        mask[y as usize * ow as usize + x as usize] = 1;
    }
    // Vertical morphological closing.
    //
    // The prompt ring is often drawn over the game's own horizontal scan-line
    // shading, which chops the strict colour mask into thin horizontal slivers.
    // A plain 4-connected blob then misses the ring entirely and the "largest
    // blob" rule picks some unrelated diffuse cyan surface instead - measured
    // on the console-with-text reference frame: without closing the winner was
    // a soft cyan ceiling light at (0.51, 0.07) of the screen (top edge!), while
    // the actual ring is at (0.64, 0.46). Closing with a 5 px tall vertical
    // element bridges those gaps and picks the ring, and leaves the two
    // door-ring reference frames on exactly the same centre as before.
    // `TOUCHHLE_DS_PROMPT_CLOSE=0` disables it, `=1` restores the old behaviour.
    let close_h: usize = std::env::var("TOUCHHLE_DS_PROMPT_CLOSE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    if close_h > 1 && oh as usize > close_h {
        let r = close_h / 2;
        let mut dil = vec![0u8; mask.len()];
        for x in 0..ow as usize {
            for y in 0..oh as usize {
                if mask[y * ow as usize + x] == 0 {
                    continue;
                }
                let lo = y.saturating_sub(r);
                let hi = (y + r).min(oh as usize - 1);
                for yy in lo..=hi {
                    dil[yy * ow as usize + x] = 1;
                }
            }
        }
        let mut ero = vec![0u8; mask.len()];
        for x in 0..ow as usize {
            for y in 0..oh as usize {
                let lo = y.saturating_sub(r);
                let hi = (y + r).min(oh as usize - 1);
                if (lo..=hi).all(|yy| dil[yy * ow as usize + x] != 0) {
                    ero[y * ow as usize + x] = 1;
                }
            }
        }
        mask = ero;
    }
    let mut best_area = 0usize;
    let mut best_sum = (0.0f64, 0.0f64);
    let mut best_bbox = (0u32, 0u32, 0u32, 0u32); // minx,miny,maxx,maxy
    let mut stack: Vec<(u32, u32)> = Vec::new();
    // Seed from the mask itself rather than from `pts`: the closing above can
    // leave filled-in pixels that never appeared in the original colour mask.
    let mut seeds: Vec<(u32, u32)> = Vec::new();
    for y in 0..oh as usize {
        for x in 0..ow as usize {
            if mask[y * ow as usize + x] == 1 {
                seeds.push((x as u32, y as u32));
            }
        }
    }
    for &(x0, y0) in &seeds {
        if mask[y0 as usize * ow as usize + x0 as usize] != 1 {
            continue;
        }
        stack.clear();
        stack.push((x0, y0));
        mask[y0 as usize * ow as usize + x0 as usize] = 2;
        let (mut area, mut sx, mut sy) = (0usize, 0.0f64, 0.0f64);
        let (mut mnx, mut mny, mut mxx, mut mxy) = (x0, y0, x0, y0);
        while let Some((x, y)) = stack.pop() {
            area += 1;
            sx += x as f64;
            sy += y as f64;
            mnx = mnx.min(x);
            mny = mny.min(y);
            mxx = mxx.max(x);
            mxy = mxy.max(y);
            for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                let nx = x as i64 + dx;
                let ny = y as i64 + dy;
                if nx < 0 || ny < 0 || nx >= ow as i64 || ny >= oh as i64 {
                    continue;
                }
                let idx = ny as usize * ow as usize + nx as usize;
                if mask[idx] == 1 {
                    mask[idx] = 2;
                    stack.push((nx as u32, ny as u32));
                }
            }
        }
        // Reject candidates hugging the bottom edge of the frame.
        //
        // Measured 2026-10-09 against the 11 reference prompt screenshots
        // (`vm\user-ref-1008\4E-*.png`): every real prompt sits at row <= 0.71
        // of the landscape image, while a cyan-tinted *floor* patch in the
        // oxygen-marker scene sits at 0.93 and won the "largest blob" contest -
        // so the E key tapped bare floor instead of the O2 diamond (the real
        // marker was only blob #2, area 3657 vs 16259). A 0.88 cut removes that
        // false positive and keeps all 11 legitimate prompts unchanged.
        // `TOUCHHLE_DS_PROMPT_MAX_ROW=<0..1>` overrides the cut.
        let cy_norm = (mny + mxy) as f64 / 2.0 / oh as f64;
        let max_row = std::env::var("TOUCHHLE_DS_PROMPT_MAX_ROW")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.88);
        if cy_norm > max_row {
            continue;
        }
        // Reject candidates that are clipped by the frame border (2026-10-10).
        //
        // Measured against the whole reference prompt set (`vm\user-ref-1008\*`,
        // replayed by `tools\ring-replay.py`): every real prompt is fully inside
        // the frame (normalised centre x 0.136..0.912, no bbox touching a border).
        // A live false positive in the chapter-12 corridor reported
        // `tap (10.5, 105.1)` - landscape x = 2.2% of the width - and that blob
        // hugged the left edge, so a bare scene element (cyan wall light) won the
        // largest-blob contest and the E key tapped bare wall. Requiring the blob
        // to be strictly inside the frame removes it and changed no reference
        // prompt. `TOUCHHLE_DS_PROMPT_EDGE=0` restores the old behaviour for A/B.
        let edge_enabled = std::env::var("TOUCHHLE_DS_PROMPT_EDGE").map_or(true, |v| v != "0");
        if edge_enabled && (mnx == 0 || mny == 0 || mxx >= ow as u32 - 1 || mxy >= oh as u32 - 1) {
            continue;
        }
        // Reject candidates too close to the left edge (2026-10-10).
        //
        // The chapter-12 corridor has a bright cyan *wall light* hugging the
        // left wall - a solid bar (bbox x3..39 y200..224 of the 960x640
        // landscape image, fill 0.52) whose centre lands at x = 2.2% of the
        // width. It beat the real blobs and E tapped bare wall.
        // Position is what separates it: replaying the whole reference prompt
        // set (`vm\user-ref-1008\*`, `tools\ring-replay.py`) gives every real
        // prompt a normalised centre x of 0.136..0.912, so a 0.08 floor keeps
        // them all with >0.055 margin and drops the wall light. Fill ratio does
        // NOT separate them (real prompts 0.150..0.644 vs the light 0.519).
        // `TOUCHHLE_DS_PROMPT_MIN_COL=<0..1>` overrides the floor.
        let min_col = std::env::var("TOUCHHLE_DS_PROMPT_MIN_COL")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.08);
        if ((mnx + mxx) as f64 / 2.0 / ow as f64) < min_col {
            continue;
        }
        if area > best_area {
            best_area = area;
            best_sum = (sx / area as f64, sy / area as f64);
            best_bbox = (mnx, mny, mxx, mxy);
        }
    }
    if best_area < min_px {
        return None;
    }
    let (bw, bh) = (
        (best_bbox.2 - best_bbox.0 + 1) as f64,
        (best_bbox.3 - best_bbox.1 + 1) as f64,
    );
    let aspect = bw / bh.max(1.0);
    // Interaction rings / markers are roughly square. This rejects long thin
    // cyan UI lines and streaks. Measured ring aspects: 0.6..1.7.
    if !(0.4..=2.5).contains(&aspect) {
        return None;
    }
    // Axial width profile: tells a QTE *arrow* from an interaction *ring*.
    // Measured on-device (strict mask, REPORT 13.166/13.167):
    //   rings  (QTE tap ring, gum ring, door ring, kinesis) -> asym 1.02..2.40
    //   arrows (combat side-swipe, stomp/chainsaw)          -> asym 3.33/3.35
    // Take the axis with the strongest end asymmetry; the wide end is the head.
    let (x0, y0, x1, y1) = best_bbox;
    let (rw, rh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
    let mut col_w = vec![0f64; rw];
    let mut row_w = vec![0f64; rh];
    for &(px, py) in &pts {
        if px >= x0 && px <= x1 && py >= y0 && py <= y1 {
            // Count the *original* colour-mask pixels only, not the pixels the
            // morphological closing filled in: the ring-vs-arrow asymmetry
            // thresholds (REPORT 13.166/13.167) were calibrated on this mask,
            // so keeping it here means changing the closing height cannot
            // silently invalidate them.
            col_w[(px - x0) as usize] += 1.0;
            row_w[(py - y0) as usize] += 1.0;
        }
    }
    let asym_of = |prof: &[f64]| -> (f64, bool) {
        let n = prof.len();
        if n < 3 {
            return (1.0, true);
        }
        let at = |f: f64| prof[((f * (n - 1) as f64).round() as usize).min(n - 1)];
        let a10 = at(0.10);
        let a90 = at(0.90);
        if a10 <= 0.0 || a90 <= 0.0 {
            return (1.0, true);
        }
        let head_at_low = a10 >= a90;
        let asym = if head_at_low { a10 / a90 } else { a90 / a10 };
        (asym, head_at_low)
    };
    let (asym_h, head_low_h) = asym_of(&col_w);
    let (asym_v, head_low_v) = asym_of(&row_w);
    // Off by default. Re-measured 2026-10-09 against the 11 reference prompts
    // (`vm\user-ref-1008\4E-*.png`) with the *exact* code path below
    // (segment on the closed mask, profile the original mask) and the ring vs
    // arrow separation does NOT hold:
    //
    //   rings that would be sent a swipe:  door-ring  3.14 / 4.60
    //                                      stasis     8.00
    //                                      zero-G     10.00
    //                                      ammo HUD   3.86
    //   arrows that would get a tap:       combat side-swipe 2.00
    //   the stomp arrow scores 3.30 but the *wrong* way (it points down, the
    //   heuristic says up) - for a real arrow glyph both ends are narrow and
    //   the widest part is the barbs in the middle, so "wide end = head" is
    //   the wrong model.
    //
    // Sending a swipe at a door ring / console ring breaks normal interaction,
    // which is far more common than a QTE arrow, so the swipe branch is gated
    // off until it is driven by the draw/prompt layer instead of the colour
    // blob. `TOUCHHLE_DS_PROMPT_SWIPE=1` restores the old behaviour for A/B.
    let swipe_enabled = std::env::var("TOUCHHLE_DS_PROMPT_SWIPE").map_or(false, |v| v == "1");
    let swipe: Option<(f32, f32)> = if swipe_enabled && asym_h.max(asym_v) >= 2.8 {
        if asym_h >= asym_v {
            // horizontal drag; head at the low-x end => swipe right-to-left
            Some((if head_low_h { -1.0 } else { 1.0 }, 0.0))
        } else {
            Some((0.0, if head_low_v { -1.0 } else { 1.0 }))
        }
    } else {
        None
    };
    let (cx, cy) = best_sum;
    let keep = best_area;
    let scale = if portrait_pt_w > 0 {
        w as f64 / portrait_pt_w as f64
    } else {
        1.0
    };
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            echo!(
                "[DSe] ring finder: viewport={}x{} unrotated_pt_w={} scale={:.2} -> point ({:.1},{:.1})",
                w,
                h,
                portrait_pt_w,
                scale,
                cx / scale,
                cy / scale
            );
        }
    }
    Some((
        cx / scale,
        cy / scale,
        keep as u32,
        swipe,
        (bw / scale, bh / scale),
    ))
}

/// Dead Space R-key aid: locate the pale-cyan **ammo / weapon-status box** the
/// game draws beside the weapon while aiming, and return its centre in
/// landscape iOS points.
///
/// Calibrated 2026-10-08 from five aiming screenshots
/// (`vm\user-ref-1008\4B-*.png`, 1440x960 = `--scale-hack=3`):
///   * box bounding boxes 110x60 .. 194x109 px -> aspect 1.4 .. 4.6
///     (it widens with the digit count, so aspect alone cannot isolate it)
///   * centre sits at x 187..255 of 480 and y 135..261 of 320 landscape points
///
/// The box **tracks the weapon pose** - y moves by ~120 pt between stances -
/// so a fixed tap point cannot work. That is why reload is code and not a plain
/// macro. Returns `None` when nothing plausible is on screen; the caller then
/// does nothing at all, because a stray tap in aim mode fires the weapon.
unsafe fn ds_find_weapon_box(gles: &mut dyn GLES, portrait_pt_w: u32) -> Option<(f64, f64, u32)> {
    let mut vp = [0i32; 4];
    gles.GetIntegerv(gles11::VIEWPORT, vp.as_mut_ptr());
    let (w, h) = (vp[2], vp[3]);
    if w <= 0 || h <= 0 || w > 4096 || h > 4096 || portrait_pt_w == 0 {
        return None;
    }
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    gles.ReadPixels(
        0,
        0,
        w,
        h,
        gles11::RGBA,
        gles11::UNSIGNED_BYTE,
        buf.as_mut_ptr() as *mut _,
    );
    // Same landscape top-down layout as ds_find_interaction_ring, so the
    // centre maps directly onto the DS-script point space.
    let (ow, oh) = (h, w);
    let mut mask = vec![0u8; (ow as usize) * (oh as usize)];
    for oy in 0..oh {
        for ox in 0..ow {
            let (sx, sy) = (oy, h - 1 - ox);
            let idx = (((h - 1 - sy) * w + sx) * 4) as usize;
            let r = buf[idx] as i32;
            let g = buf[idx + 1] as i32;
            let b = buf[idx + 2] as i32;
            if g >= 200 && b >= 190 && g >= b && r <= 180 {
                mask[oy as usize * ow as usize + ox as usize] = 1;
            }
        }
    }
    // Same vertical closing as the ring finder: the game's scan-line shading
    // chops the strict colour mask into horizontal slivers.
    let close_h: usize = std::env::var("TOUCHHLE_DS_PROMPT_CLOSE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    if close_h > 1 && oh as usize > close_h {
        let r = close_h / 2;
        let mut dil = vec![0u8; mask.len()];
        for x in 0..ow as usize {
            for y in 0..oh as usize {
                if mask[y * ow as usize + x] == 0 {
                    continue;
                }
                let lo = y.saturating_sub(r);
                let hi = (y + r).min(oh as usize - 1);
                for yy in lo..=hi {
                    dil[yy * ow as usize + x] = 1;
                }
            }
        }
        let mut ero = vec![0u8; mask.len()];
        for x in 0..ow as usize {
            for y in 0..oh as usize {
                let lo = y.saturating_sub(r);
                let hi = (y + r).min(oh as usize - 1);
                if (lo..=hi).all(|yy| dil[yy * ow as usize + x] != 0) {
                    ero[y * ow as usize + x] = 1;
                }
            }
        }
        mask = ero;
    }
    // Connected components, keeping every blob so the selector below can weigh
    // them by shape *and* position rather than taking the biggest one blindly
    // (aiming scenes also contain big cyan scenery, e.g. the ice cave).
    let mut best: Option<(usize, f64, f64, f64, f64)> = None;
    let mut stack: Vec<(u32, u32)> = Vec::new();
    let screen_px = (w as usize) * (h as usize);
    let min_px = (screen_px / 2000).max(200);
    for y0 in 0..oh as usize {
        for x0 in 0..ow as usize {
            if mask[y0 * ow as usize + x0] != 1 {
                continue;
            }
            stack.clear();
            stack.push((x0 as u32, y0 as u32));
            mask[y0 * ow as usize + x0] = 2;
            let (mut area, mut sx, mut sy) = (0usize, 0.0f64, 0.0f64);
            let (mut mnx, mut mny, mut mxx, mut mxy) = (x0, y0, x0, y0);
            while let Some((x, y)) = stack.pop() {
                area += 1;
                sx += x as f64;
                sy += y as f64;
                let (xu, yu) = (x as usize, y as usize);
                mnx = mnx.min(xu);
                mny = mny.min(yu);
                mxx = mxx.max(xu);
                mxy = mxy.max(yu);
                for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                    let nx = x as i64 + dx;
                    let ny = y as i64 + dy;
                    if nx < 0 || ny < 0 || nx >= ow as i64 || ny >= oh as i64 {
                        continue;
                    }
                    let idx = ny as usize * ow as usize + nx as usize;
                    if mask[idx] == 1 {
                        mask[idx] = 2;
                        stack.push((nx as u32, ny as u32));
                    }
                }
            }
            if area < min_px {
                continue;
            }
            let (cx, cy) = (sx / area as f64, sy / area as f64);
            let (bw, bh) = ((mxx - mnx + 1) as f64, (mxy - mny + 1) as f64);
            let aspect = bw / bh.max(1.0);
            // Position window: the ammo box lives just above/right of the
            // weapon, i.e. a wide band across the lower middle of the screen.
            // Normalised to the landscape image.
            let nx = cx / ow as f64;
            let ny = cy / oh as f64;
            // Measured ammo boxes span x 185..261 pt of 480 (= 0.39..0.54) and
            // y 132..269 of 320 (= 0.41..0.84). The window is deliberately a
            // bit wider than that but *not* wider than 0.60,-: the big "开启 /
            // 运行" console box sits at x 309 pt (0.64) and is the worst
            // false-positive in the reference set. Shape and fill ratio cannot
            // separate the ammo box from that box or from QTE arrows (checked
            // offline: aspect 1.42..2.75 and fill 0.16..0.46 both overlap), so
            // position and size are all we have.
            if !(0.28..=0.60).contains(&nx) || !(0.32..=0.92).contains(&ny) {
                continue;
            }
            // Wide, short-ish rectangle. The floor stops digit-free slivers;
            // the ceiling stops full-screen cyan UI panels.
            if !(1.3..=6.0).contains(&aspect) || !(24.0..=210.0).contains(&bw) {
                continue;
            }
            let better = match best {
                None => true,
                Some((ba, ..)) => area > ba,
            };
            if better {
                best = Some((area, cx, cy, bw, bh));
            }
        }
    }
    let (area, cx, cy, bw, bh) = best?;
    let scale = w as f64 / portrait_pt_w as f64;
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            echo!(
                "[DSr] ammo box finder: viewport={}x{} scale={:.2} -> box {:.0}x{:.0} px, centre ({:.1},{:.1})",
                w,
                h,
                scale,
                bw,
                bh,
                cx / scale,
                cy / scale
            );
        }
    }
    Some((cx / scale, cy / scale, area as u32))
}

fn glClear(env: &mut Environment, mask: GLbitfield) {
    let _stutter_span=crate::ds_stutter_probe::Span::new(2,mask as u64);
    if mask & gles11::COLOR_BUFFER_BIT != 0 {
        crate::ds_frontend_trace::prime(env);
        crate::ds_host_overlay::prepare();
        crate::ds_chapter_overlay::observe(env);
        crate::ds_subtitle_overlay::observe(env);
        crate::ds_objective_overlay::observe(env);
        crate::ds_tutorial_overlay::observe(env);
        ds_publish_qte_overlay(env);
        ds_test_assist::observe(env);
    }
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glClear(mask=0x{:x}) [this log will only be shown once]",
                mask
            );
        }
    }
    if mask & gles11::DEPTH_BUFFER_BIT != 0 {
        let drawable = crate::frameworks::opengles::gles_guest::drawable_fbo_of_current_ctx(env);
        if drawable.is_some() {
            let predicted_frame = crate::window::DS_FRAME_COUNTER
                .load(std::sync::atomic::Ordering::Relaxed)
                .saturating_add(1);
            with_ctx_and_mem(env, |gles, _mem| unsafe {
                let mut framebuffer: GLint = 0;
                gles.GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut framebuffer);
                if drawable == Some(framebuffer as GLuint) {
                    crate::ds_msaa::note_guest_drawable_depth_clear(predicted_frame);
                }
            });
        }
    }
    // Debug aid: `TOUCHHLE_DS_DUMP_FRAMES=<dir>` writes the colour buffer to a
    // PPM just before each colour clear, i.e. one file per rendered frame.
    // Frames are thinned by `TOUCHHLE_DS_DUMP_FRAME_EVERY` (default 60) because
    // a full-rate dump is ~1.8 MB per frame at 640x960.
    // Debug aid: `TOUCHHLE_DS_SCAN_VTABLE="0x00a84790[,0x...]"` searches guest
    // memory once (at `TOUCHHLE_DS_SCAN_AT_FRAME`, default 60) for live C++
    // objects whose vptr is one of those addresses. This answers "was this
    // class ever constructed?" for code paths that draw tracing cannot see,
    // without needing a CPU-level hook.
    if mask & gles11::COLOR_BUFFER_BIT != 0 {
        if let Ok(dir) = std::env::var("TOUCHHLE_DS_DUMP_FBO_DIR") {
            let wanted_frame = std::env::var("TOUCHHLE_DS_DUMP_FBO_FRAME")
                .ok()
                .and_then(|value| value.parse::<u64>().ok());
            let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
            if wanted_frame == Some(frame) {
                with_ctx_and_mem(env, |gles, _mem| unsafe {
                    let mut fbo: GLint = 0;
                    gles.GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut fbo);
                    let mut viewport = [0i32; 4];
                    gles.GetIntegerv(gles11::VIEWPORT, viewport.as_mut_ptr());
                    let (w, h) = (viewport[2], viewport[3]);
                    if w <= 0 || h <= 0 || w > 4096 || h > 4096 {
                        return;
                    }
                    for dump_fbo in 0..=4u32 {
                        if dump_fbo != 0 && gles.IsFramebufferOES(dump_fbo) == 0 {
                            continue;
                        }
                        gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, dump_fbo);
                        let mut rgba = vec![0u8; (w * h * 4) as usize];
                        gles.ReadPixels(
                            0,
                            0,
                            w,
                            h,
                            gles11::RGBA,
                            gles11::UNSIGNED_BYTE,
                            rgba.as_mut_ptr().cast(),
                        );
                        let mut ppm = Vec::with_capacity((w * h * 3) as usize + 32);
                        ppm.extend_from_slice(format!("P6\n{} {}\n255\n", w, h).as_bytes());
                        for y in 0..h {
                            for x in 0..w {
                                let i = (((h - 1 - y) * w + x) * 4) as usize;
                                ppm.extend_from_slice(&rgba[i..i + 3]);
                            }
                        }
                        use std::sync::atomic::{AtomicUsize, Ordering as AOrd};
                        static CLEAR_SEQ: AtomicUsize = AtomicUsize::new(0);
                        let seq = CLEAR_SEQ.fetch_add(1, AOrd::Relaxed) + 1;
                        let _ = std::fs::create_dir_all(&dir);
                        let path = std::path::Path::new(&dir).join(format!(
                            "clear_fbo{}_{:03}_f{:06}.ppm",
                            dump_fbo, seq, frame
                        ));
                        if std::fs::write(&path, ppm).is_ok() {
                            echo!("[DSfbo] clear dumped {} -> {}", dump_fbo, path.display());
                        }
                    }
                    gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, fbo as GLuint);
                });
            }
        }
        if let Ok(spec) = std::env::var("TOUCHHLE_DS_SCAN_VTABLE") {
            use std::sync::atomic::{AtomicBool, Ordering as AOrd};
            static SCANNED: AtomicBool = AtomicBool::new(false);
            let at: usize = std::env::var("TOUCHHLE_DS_SCAN_AT_FRAME")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            let fnow =
                crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed) as usize;
            // `TOUCHHLE_DS_SCAN_RETRY=<n>`: if the object is not there yet,
            // scan again every n frames (the Hud object does not exist during
            // the first seconds of the app, so a one-shot scan at a small
            // frame number silently finds nothing).
            let retry: usize = std::env::var("TOUCHHLE_DS_SCAN_RETRY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let due = !SCANNED.load(AOrd::Relaxed)
                && fnow >= at
                && (retry == 0 || (fnow - at) % retry == 0);
            if due {
                SCANNED.store(true, AOrd::Relaxed);
                // How many words of each hit to print (default 4). Raise it to
                // inspect an object's fields in one run instead of many.
                let dump: u32 = std::env::var("TOUCHHLE_DS_SCAN_DUMP")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(4)
                    .clamp(4, 128);
                // The state tracker names the object it wants; make sure that
                // address is in the scan set even if the user didn't list it.
                let track_want: Option<u32> = std::env::var("TOUCHHLE_DS_TRACK_VTABLE")
                    .ok()
                    .and_then(|v| u32::from_str_radix(v.trim().trim_start_matches("0x"), 16).ok())
                    .filter(|v| *v != 0);
                let mut targets: Vec<u32> = spec
                    .split(',')
                    .filter_map(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
                    .collect();
                if let Some(w) = track_want {
                    if !targets.contains(&w) {
                        targets.push(w);
                    }
                }
                // `TOUCHHLE_DS_POKE_VTABLE=<vptr>`: also remember the objects of
                // that class, for the OR-poke further down (it has to be able to
                // touch a different class than the tracked one).
                let poke_want: Option<u32> = std::env::var("TOUCHHLE_DS_POKE_VTABLE")
                    .ok()
                    .and_then(|v| ds_parse_u32(&v))
                    .filter(|v| *v != 0);
                if let Some(w) = poke_want {
                    if !targets.contains(&w) {
                        targets.push(w);
                    }
                }
                // `TOUCHHLE_DS_CHASE="<vptr>:<offset>"`: for every object of that
                // class, dereference `+offset` and (a) log the pointee and its
                // vptr, (b) add the pointee to the poke set. Built for
                // DECOMPILE.md D-021: `GameWorld::Update` acts on the object it
                // holds at `+0xe0`, and we need to know which class that is.
                // "vptr:off" or "vptr:off:words" - the third field dumps that
                // many words at the instance *and* at the pointee, which is how
                // an `InputRegionList` (the object that decides whether a tap
                // counts, DECOMPILE.md D-023) can be read out.
                // "vptr:off[>off2[>off3]]:words" - follow a pointer path and dump
                // `words` words at every address along it (the object itself
                // included). That is how a whole `InputRegionList` subtree can be
                // read out in one run (DECOMPILE.md D-024/D-025).
                let chase: Option<(u32, Vec<u32>, usize)> =
                    std::env::var("TOUCHHLE_DS_CHASE").ok().and_then(|v| {
                        let mut it = v.split(':');
                        let a = ds_parse_u32(it.next()?)?;
                        let path: Vec<u32> = it
                            .next()?
                            .split('/')
                            .map(|s| ds_parse_u32(s).unwrap_or(0))
                            .collect();
                        let n = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                        Some((a, path, n))
                    });
                if let Some((w, _, _)) = &chase {
                    if !targets.contains(&w) {
                        targets.push(*w);
                    }
                }
                // `TOUCHHLE_DS_EKEY_FWD_VTABLE=<InputForwarderTapToAction vptr>`:
                // remember those objects for the E key's region lookup.
                let fwd_want: Option<u32> = std::env::var("TOUCHHLE_DS_EKEY_FWD_VTABLE")
                    .ok()
                    .and_then(|v| ds_parse_u32(&v))
                    .filter(|v| *v != 0);
                if let Some(w) = fwd_want {
                    if !targets.contains(&w) {
                        targets.push(w);
                    }
                }
                let ranges: Vec<(u32, u32)> = env.mem.iter_used_ranges().collect();
                echo!(
                    "[DSscan] frame {}: scanning {} range(s) for {:?}",
                    fnow,
                    ranges.len(),
                    targets
                );
                let mut hits = 0usize;
                let mut words: u64 = 0;
                for (base, size) in ranges {
                    let mut off: u32 = 0;
                    while off + 4 <= size {
                        let start = base + off;
                        let len = 4096.min(size - off);
                        let found = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let mut v: Vec<u32> = Vec::new();
                            let mut a = start;
                            while a + 4 <= start + len {
                                let w: u32 =
                                    env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(a));
                                if targets.contains(&w) {
                                    v.push(a);
                                }
                                a += 4;
                            }
                            v
                        }));
                        if let Ok(v) = found {
                            words += (len / 4) as u64;
                            for addr in v {
                                hits += 1;
                                // Remember the object the state tracker wants,
                                // so tracking costs 32 bytes per frame rather
                                // than a second full-memory sweep.
                                let vptr: u32 =
                                    env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(addr));
                                if let Some(fw) = fwd_want {
                                    if vptr == fw {
                                        if let Ok(mut o) = ekey_fwds().lock() {
                                            if !o.contains(&addr) {
                                                o.push(addr);
                                            }
                                        }
                                    }
                                }
                                if let Some((cw, path, cdump)) = &chase {
                                    if vptr == *cw {
                                        if let Ok(mut o) = poke_objs().lock() {
                                            if !o.contains(&addr) {
                                                o.push(addr);
                                            }
                                        }
                                        let mut cur = addr;
                                        let hops = path.len();
                                        for (lvl, off) in std::iter::once(None)
                                            .chain(path.iter().map(Some))
                                            .enumerate()
                                        {
                                            if lvl > 0 {
                                                let o = off.unwrap_or(&0);
                                                cur = env.mem.read(
                                                    crate::mem::ConstPtr::<u32>::from_bits(
                                                        cur + *o,
                                                    ),
                                                );
                                                if cur == 0 {
                                                    break;
                                                }
                                            }
                                            let vp: u32 = env
                                                .mem
                                                .read(crate::mem::ConstPtr::<u32>::from_bits(cur));
                                            if *cdump > 0 {
                                                let mut w = String::new();
                                                for i in 0..(*cdump).min(32) {
                                                    let a: u32 = env.mem.read(
                                                        crate::mem::ConstPtr::<u32>::from_bits(
                                                            cur + (i as u32) * 4,
                                                        ),
                                                    );
                                                    w.push_str(&format!(" {:08x}", a));
                                                }
                                                echo!(
                                                    "[DSchase{}] base={:#010x} hop={}/{} addr={:#010x} vptr={:#010x}:{}",
                                                    lvl,
                                                    addr,
                                                    lvl,
                                                    hops,
                                                    cur,
                                                    vp,
                                                    w
                                                );
                                            } else {
                                                echo!(
                                                    "[DSchase{}] base={:#010x} hop={}/{} addr={:#010x} vptr={:#010x}",
                                                    lvl,
                                                    addr,
                                                    lvl,
                                                    hops,
                                                    cur,
                                                    vp
                                                );
                                            }
                                            if lvl == hops {
                                                if let Ok(mut o) = poke_objs().lock() {
                                                    if !o.contains(&cur) {
                                                        o.push(cur);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                if let Some(want) = track_want {
                                    if let Some(pw) = poke_want {
                                        if vptr == pw {
                                            if let Ok(mut o) = poke_objs().lock() {
                                                if !o.contains(&addr) {
                                                    o.push(addr);
                                                    echo!(
                                                        "[DSpoke] found poke target {:#010x} (vptr {:#010x})",
                                                        addr,
                                                        vptr
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    if vptr == want && TRACK_OBJ.load(AOrd::Relaxed) == 0 {
                                        TRACK_OBJ.store(addr, AOrd::Relaxed);
                                        echo!(
                                            "[DStrack] will track object {:#010x} (vptr {:#010x})",
                                            addr,
                                            vptr
                                        );
                                    }
                                    // `TOUCHHLE_DS_TRACK_ALL=1`: also collect every
                                    // *other* matching object. A level holds several
                                    // instances of a class (measured 2026-10-11: four
                                    // GameObjectDoor objects), so tracking only the
                                    // first can watch a door that is not the one the
                                    // player is standing at.
                                    if vptr == want
                                        && std::env::var_os("TOUCHHLE_DS_TRACK_ALL").is_some()
                                    {
                                        if let Ok(mut v) = track_all().lock() {
                                            if !v.contains(&addr) {
                                                v.push(addr);
                                            }
                                        }
                                    }
                                }
                                if hits <= 40 {
                                    // `TOUCHHLE_DS_TRACK_POKE="off:value"`: write a u32
                                    // into every scanned object of the tracked class.
                                    // Diagnostic for a suspected state field - e.g.
                                    // force a door's +0x1d0 to 4 (= "open" per
                                    // DECOMPILE.md D-001) without depending on the
                                    // game accepting any input.
                                    // One-shot variant (at scan time); the per-frame
                                    // version with an optional guard is below.
                                    if let Ok(spec) = std::env::var("TOUCHHLE_DS_TRACK_POKE_ONCE") {
                                        let mut it = spec.split(&[':', ','][..]);
                                        let parse = |s: &str| -> u32 {
                                            let t = s.trim();
                                            match t
                                                .strip_prefix("0x")
                                                .or_else(|| t.strip_prefix("0X"))
                                            {
                                                Some(h) => u32::from_str_radix(h, 16).unwrap_or(0),
                                                None => t.parse().unwrap_or(0),
                                            }
                                        };
                                        if let (Some(a), Some(b)) = (it.next(), it.next()) {
                                            let poff = parse(a);
                                            let pval = parse(b);
                                            let _ = std::panic::catch_unwind(
                                                std::panic::AssertUnwindSafe(|| {
                                                    env.mem.write(
                                                        crate::mem::MutPtr::<u32>::from_bits(
                                                            addr + poff,
                                                        ),
                                                        pval,
                                                    )
                                                }),
                                            );
                                            echo!(
                                                "[DSpoke] obj={:#010x} +{:#x} <- {:#x}",
                                                addr,
                                                poff,
                                                pval
                                            );
                                        }
                                    }
                                    let mut line = format!("[DSscan] object at {:#010x}:", addr);
                                    let mut o = 0u32;
                                    while o < dump {
                                        let v: u32 = env
                                            .mem
                                            .read(crate::mem::ConstPtr::<u32>::from_bits(addr + o));
                                        line.push_str(&format!(" +{:x}={:08x}", o, v));
                                        o += 4;
                                    }
                                    echo!("{}", line);
                                }
                            }
                        }
                        off += len;
                    }
                }
                echo!("[DSscan] done: {} hit(s), {} words scanned", hits, words);
                // Keep re-scanning while the E key's forwarder list is still
                // empty too: those objects are created later than the class the
                // tracker wants, so "found the tracked class" alone would stop
                // the scan before they exist (measured 2026-10-11: take 52's E
                // press logged "no live InputRegion found").
                let fwds_missing =
                    fwd_want.is_some() && ekey_fwds().lock().map(|v| v.is_empty()).unwrap_or(true);
                if retry > 0 && (hits == 0 || fwds_missing) {
                    // nothing yet - arm another attempt
                    SCANNED.store(false, AOrd::Relaxed);
                }
            }
        }
        // Hands-free capture: once the object with vtable
        // `TOUCHHLE_DS_TRACK_VTABLE` has been located by the scan above,
        // record its state bytes once per frame and remember the frames where
        // they change. A HUD hint appearing (the QTE arrow, a prompt ring)
        // flips one of these bytes, so the user only has to play until the
        // moment happens - no keypress timing.
        {
            use std::sync::atomic::{AtomicBool, Ordering as AOrd};
            static CHANGE: AtomicBool = AtomicBool::new(false);
            static FORCED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            // Per-object snapshots: with `TOUCHHLE_DS_TRACK_ALL=1` several
            // instances are compared in the same frame, and one shared buffer
            // made them look "changed" every single frame (measured 2026-10-11:
            // two doors alternated 01/00 forever).
            static PREV: std::sync::OnceLock<
                std::sync::Mutex<std::collections::HashMap<u32, Vec<u8>>>,
            > = std::sync::OnceLock::new();
            let fnow_now =
                crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed) as usize;
            // Fire a deferred tracker tap (TOUCHHLE_DS_TRACK_TAP_DELAY).
            {
                let at = TAP_AT.load(AOrd::Relaxed);
                if at != 0 && fnow_now >= at as usize {
                    TAP_AT.store(0, AOrd::Relaxed);
                    crate::window::AUTO_TAP.store(
                        TAP_XY.load(AOrd::Relaxed),
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    echo!("[DStap] deferred tap fired at f={}", fnow_now);
                }
            }
            if let Ok(spec) = std::env::var("TOUCHHLE_DS_TRACK_VTABLE") {
                let want =
                    u32::from_str_radix(spec.trim().trim_start_matches("0x"), 16).unwrap_or(0);
                let off: u32 = std::env::var("TOUCHHLE_DS_TRACK_OFFSET")
                    .ok()
                    .and_then(|v| {
                        // Accept both `464` and `0x1d0`: a bare parse() used to
                        // reject the hex spelling and silently keep 0x1b8.
                        let t = v.trim().to_string();
                        match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                            Some(h) => u32::from_str_radix(h, 16).ok(),
                            None => t.parse().ok(),
                        }
                    })
                    .unwrap_or(0x1b8);
                let len: u32 = std::env::var("TOUCHHLE_DS_TRACK_LEN")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(32)
                    .clamp(4, 128);
                if want != 0 {
                    // The address is filled in by the vtable scan above (which
                    // already sweeps guest memory); finding it here would cost
                    // a second full sweep every frame.
                    let obj =
                        crate::frameworks::opengles::gles_guest::TRACK_OBJ.load(AOrd::Relaxed);
                    let mut objs: Vec<u32> = Vec::new();
                    if obj != 0 {
                        objs.push(obj);
                    }
                    if let Ok(v) = track_all().lock() {
                        for a in v.iter() {
                            if !objs.contains(a) {
                                objs.push(*a);
                            }
                        }
                    }
                    for &obj in &objs {
                        let mut cur = Vec::with_capacity(len as usize);
                        let mut ok = true;
                        for i in 0..len {
                            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                env.mem
                                    .read(crate::mem::ConstPtr::<u8>::from_bits(obj + off + i))
                            })) {
                                Ok(v) => cur.push(v),
                                Err(_) => {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if ok {
                            let cell = PREV.get_or_init(|| {
                                std::sync::Mutex::new(std::collections::HashMap::new())
                            });
                            let mut map = cell.lock().unwrap();
                            let prev = map.entry(obj).or_insert_with(|| vec![0xFFu8; cur.len()]);
                            if prev.len() != cur.len() {
                                *prev = vec![0xFFu8; cur.len()];
                            }
                            if *prev != cur {
                                // Rising edge 0 -> non-zero: the game just started
                                // treating this object as interactive, which is
                                // exactly when it draws the ring (D-004). Ask the
                                // window layer for a tap at that instant.
                                if prev.first() == Some(&0u8)
                                    && cur.first().map_or(false, |b| *b != 0)
                                {
                                    if let Ok(spec) = std::env::var("TOUCHHLE_DS_TRACK_TAP") {
                                        let mut it = spec.split(',');
                                        let x: f32 = it
                                            .next()
                                            .and_then(|v| v.trim().parse().ok())
                                            .unwrap_or(240.0);
                                        let y: f32 = it
                                            .next()
                                            .and_then(|v| v.trim().parse().ok())
                                            .unwrap_or(160.0);
                                        let packed =
                                            (((x as u32) & 0xFFFF) << 16) | ((y as u32) & 0xFFFF);
                                        // `TOUCHHLE_DS_TRACK_TAP_DELAY=<frames>`:
                                        // the successful tool click in
                                        // DECOMPILE.md D-016 happened ~1-3 s
                                        // AFTER the flag flipped, while tapping on
                                        // the flip frame itself does nothing, so
                                        // allow the tap to be deferred.
                                        let delay: usize =
                                            std::env::var("TOUCHHLE_DS_TRACK_TAP_DELAY")
                                                .ok()
                                                .and_then(|v| v.parse().ok())
                                                .unwrap_or(0);
                                        TAP_AT.store(
                                            (fnow_now + delay) as u32,
                                            std::sync::atomic::Ordering::Relaxed,
                                        );
                                        TAP_XY.store(packed, std::sync::atomic::Ordering::Relaxed);
                                        echo!(
                                            "[DStap] obj={:#010x} became interactive -> request tap ({},{})",
                                            obj,
                                            x,
                                            y
                                        );
                                    }
                                }
                                let hex: String =
                                    cur.iter().map(|b| format!("{:02x}", b)).collect();
                                echo!(
                                    "[DStrack] f={} obj={:#010x} CHANGED off={:#x} bytes={}",
                                    fnow_now,
                                    obj,
                                    off,
                                    hex
                                );
                                *prev = cur;
                                CHANGE.store(true, AOrd::Relaxed);
                            }
                        }
                        // Optional second window (`TOUCHHLE_DS_TRACK_OFFSET2`,
                        // `TOUCHHLE_DS_TRACK_LEN2`): lets one run watch both the
                        // interaction trigger (+0x164) and the door's open state
                        // (+0x1d0, DECOMPILE.md D-001) instead of needing two runs.
                        if let Ok(spec2) = std::env::var("TOUCHHLE_DS_TRACK_OFFSET2") {
                            let off2: u32 = {
                                let t = spec2.trim().to_string();
                                match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                                    Some(h) => u32::from_str_radix(h, 16).unwrap_or(0),
                                    None => t.parse().unwrap_or(0),
                                }
                            };
                            let len2: u32 = std::env::var("TOUCHHLE_DS_TRACK_LEN2")
                                .ok()
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(4)
                                .clamp(1, 128);
                            static PREV2: std::sync::OnceLock<
                                std::sync::Mutex<std::collections::HashMap<u32, Vec<u8>>>,
                            > = std::sync::OnceLock::new();
                            if off2 != 0 {
                                let cell2 = PREV2.get_or_init(|| {
                                    std::sync::Mutex::new(std::collections::HashMap::new())
                                });
                                let mut map2 = cell2.lock().unwrap();
                                for &obj2 in &objs {
                                    let mut cur2 = Vec::with_capacity(len2 as usize);
                                    let mut ok2 = true;
                                    for i in 0..len2 {
                                        match std::panic::catch_unwind(
                                            std::panic::AssertUnwindSafe(|| {
                                                env.mem.read(crate::mem::ConstPtr::<u8>::from_bits(
                                                    obj2 + off2 + i,
                                                ))
                                            }),
                                        ) {
                                            Ok(v) => cur2.push(v),
                                            Err(_) => {
                                                ok2 = false;
                                                break;
                                            }
                                        }
                                    }
                                    if !ok2 {
                                        continue;
                                    }
                                    let prev2 = map2
                                        .entry(obj2)
                                        .or_insert_with(|| vec![0xFFu8; cur2.len()]);
                                    if prev2.len() != cur2.len() {
                                        *prev2 = vec![0xFFu8; cur2.len()];
                                    }
                                    if *prev2 != cur2 {
                                        let hex2: String =
                                            cur2.iter().map(|b| format!("{:02x}", b)).collect();
                                        echo!(
                                            "[DStrack2] f={} obj={:#010x} CHANGED off={:#x} bytes={}",
                                            fnow_now,
                                            obj2,
                                            off2,
                                            hex2
                                        );
                                        *prev2 = cur2;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // The frame dumper below honours this: a state change forces that
            //
            // `TOUCHHLE_DS_TRACK_POKE="off:value[:guard]"` (checked every frame,
            // unlike the one-shot POKE_ONCE at scan time): write `value` into
            // every tracked object's `off`, but only when the u32 at `guard` is
            // non-zero. Guard on 0x164 so only the *currently interactable* door
            // is poked - the previous unguarded run poked 4 doors and could not
            // tell whether the door in front of the player was among them
            // (DECOMPILE.md D-010).
            if let Ok(spec) = std::env::var("TOUCHHLE_DS_TRACK_POKE") {
                let mut it = spec.split(&[':', ','][..]);
                let parse = |s: &str| -> u32 {
                    let t = s.trim();
                    match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                        Some(h) => u32::from_str_radix(h, 16).unwrap_or(0),
                        None => t.parse().unwrap_or(0),
                    }
                };
                if let (Some(a), Some(b)) = (it.next(), it.next()) {
                    let poff = parse(a);
                    let pval = parse(b);
                    let guard = it.next().map(parse).filter(|g| *g != 0);
                    let mut done = 0usize;
                    // This block sits outside the tracker scope, so rebuild the
                    // object list from the same sources the tracker used.
                    let mut objs3: Vec<u32> = Vec::new();
                    let tracked = crate::frameworks::opengles::gles_guest::TRACK_OBJ
                        .load(std::sync::atomic::Ordering::Relaxed);
                    if tracked != 0 {
                        objs3.push(tracked);
                    }
                    if let Ok(v) = crate::frameworks::opengles::gles_guest::track_all().lock() {
                        for a in v.iter() {
                            if !objs3.contains(a) {
                                objs3.push(*a);
                            }
                        }
                    }
                    for &obj3 in &objs3 {
                        let go = match guard {
                            Some(g) => {
                                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    // Read ONE BYTE: the flag at +0x164 is a byte
                                    // (`ldrb` in RenderHUDInteractions, D-004) and a
                                    // u32 read picks up the 0xcdcdcd padding behind
                                    // it, which made the guard always true.
                                    env.mem
                                        .read(crate::mem::ConstPtr::<u8>::from_bits(obj3 + g))
                                })) {
                                    Ok(v) => v != 0,
                                    Err(_) => false,
                                }
                            }
                            None => true,
                        };
                        if !go {
                            continue;
                        }
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            env.mem
                                .write(crate::mem::MutPtr::<u32>::from_bits(obj3 + poff), pval)
                        }));
                        done += 1;
                    }
                    if done > 0 {
                        log_once!("TOUCHHLE_DS_TRACK_POKE: poked the first guarded object(s)");
                        echo!(
                            "[DSpoke] f={} +{:#x} <- {:#x} on {} object(s){}",
                            fnow_now,
                            poff,
                            pval,
                            done,
                            if guard.is_some() { " (guarded)" } else { "" }
                        );
                    }
                }
            }
            // `TOUCHHLE_DS_POKE_VTABLE` + `TOUCHHLE_DS_POKE_OFF` +
            // `TOUCHHLE_DS_POKE_MASK` (+ `TOUCHHLE_DS_POKE_MODE=or|set`,
            // `TOUCHHLE_DS_POKE_FROM=<frame>`): read-modify-write a u32 field of
            // another class, every frame. Built to answer one question from
            // DECOMPILE.md D-021: `GameWorld::Update` calls
            // `player->OnInteract()` exactly when bit 29 of the player's flag
            // word (+0x40) is set, and nothing else in the binary sets it. If
            // OR-ing that bit in opens the door, then the missing piece is the
            // input->"interaction requested" hop, not the tap position.
            if std::env::var_os("TOUCHHLE_DS_POKE_VTABLE").is_some()
                || std::env::var_os("TOUCHHLE_DS_CHASE").is_some()
            {
                let poff = std::env::var("TOUCHHLE_DS_POKE_OFF")
                    .ok()
                    .and_then(|v| ds_parse_u32(&v))
                    .unwrap_or(0x40);
                let pmask = std::env::var("TOUCHHLE_DS_POKE_MASK")
                    .ok()
                    .and_then(|v| ds_parse_u32(&v))
                    .unwrap_or(0);
                let pmode = std::env::var("TOUCHHLE_DS_POKE_MODE").unwrap_or_else(|_| "or".into());
                let pfrom: u64 = std::env::var("TOUCHHLE_DS_POKE_FROM")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                if pmask != 0 && fnow_now as u64 >= pfrom {
                    let objs: Vec<u32> = match poke_objs().lock() {
                        Ok(v) => v.clone(),
                        Err(_) => Vec::new(),
                    };
                    for o in objs {
                        let cur: u32 = env
                            .mem
                            .read(crate::mem::ConstPtr::<u32>::from_bits(o + poff));
                        let nv = if pmode == "set" { pmask } else { cur | pmask };
                        if nv != cur {
                            env.mem
                                .write(crate::mem::MutPtr::<u32>::from_bits(o + poff), nv);
                            echo!(
                                "[DSpoke2] f={} obj={:#010x} +{:#x} {:#010x} -> {:#010x}",
                                fnow_now,
                                o,
                                poff,
                                cur,
                                nv
                            );
                        }
                    }
                }
            }
            // frame to be written even if it is not on the dump schedule.
            // `TOUCHHLE_DS_LOG_FORWARDERS=1`: one-shot dump of every collected
            // action forwarder - its action code, its owner object and the
            // owner's vptr. Resolving those vptrs offline names the class, which
            // is how an action code (0x10 = interact, ...) is mapped to a
            // function.
            if std::env::var_os("TOUCHHLE_DS_LOG_FORWARDERS").is_some() {
                static DONE: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                // The list is filled by the E key's own sweep; do that sweep here
                // (every 30 frames) so the census does not need an E press.
                if fnow_now % 30 == 0 && ekey_fwds().lock().map(|v| v.is_empty()).unwrap_or(false) {
                    let objs = ds_sweep_vptr(env, 0x00a7d1e8);
                    if !objs.is_empty() {
                        echo!("[DSfwd] sweep found {} forwarder(s)", objs.len());
                        if let Ok(mut o) = ekey_fwds().lock() {
                            for a in objs {
                                if !o.contains(&a) {
                                    o.push(a);
                                }
                            }
                        }
                    }
                }
                let fwds: Vec<u32> = match ekey_fwds().lock() {
                    Ok(v) => v.clone(),
                    Err(_) => Vec::new(),
                };
                // Also allow a second, later census so a changed game state
                // (e.g. the forced aim hold) can be compared with the first.
                let at: u64 = std::env::var("TOUCHHLE_DS_LOG_FORWARDERS_AT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                static DONE2: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                let due_late = at != 0 && fnow_now as u64 >= at && !DONE2.swap(true, AOrd::Relaxed);
                if !fwds.is_empty() && (!DONE.swap(true, AOrd::Relaxed) || due_late) {
                    if due_late {
                        echo!("[DSfwd] late census at f={}", fnow_now);
                    }
                    echo!("[DSfwd] {} forwarder(s) collected", fwds.len());
                    for f in fwds {
                        let vp: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(f));
                        let code: u32 = env
                            .mem
                            .read(crate::mem::ConstPtr::<u32>::from_bits(f + 0x1c));
                        let owner: u32 = env
                            .mem
                            .read(crate::mem::ConstPtr::<u32>::from_bits(f + 0x8));
                        let o_vptr: u32 = if owner != 0 {
                            env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(owner))
                        } else {
                            0
                        };
                        let list: u32 = env
                            .mem
                            .read(crate::mem::ConstPtr::<u32>::from_bits(f + 0x10));
                        let on: u8 = if list != 0 {
                            env.mem
                                .read(crate::mem::ConstPtr::<u8>::from_bits(list + 8))
                        } else {
                            0
                        };
                        // Also read the region rectangle the forwarder would test
                        // (list+0xc -> array -> region -> +0xc..+0x18).
                        // The player's own five buttons carry no live rectangle
                        // outside aim mode; dump their words so any embedded name
                        // string can be resolved offline.
                        if o_vptr == 0x00a7a4f8 {
                            let mut w = String::new();
                            for i in 0..24 {
                                let a: u32 = env
                                    .mem
                                    .read(crate::mem::ConstPtr::<u32>::from_bits(f + i * 4));
                                w.push_str(&format!(" {:08x}", a));
                            }
                            echo!("[DSfwdW] fwd={:#010x} code={:#04x}:{}", f, code, w);
                        }
                        let mut rect = (0i32, 0i32, 0i32, 0i32);
                        if list != 0 {
                            let begin: u32 = env
                                .mem
                                .read(crate::mem::ConstPtr::<u32>::from_bits(list + 0xc));
                            if begin != 0 {
                                let r: u32 =
                                    env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(begin));
                                if r != 0 {
                                    rect = (
                                        env.mem
                                            .read(crate::mem::ConstPtr::<i32>::from_bits(r + 0xc)),
                                        env.mem
                                            .read(crate::mem::ConstPtr::<i32>::from_bits(r + 0x10)),
                                        env.mem
                                            .read(crate::mem::ConstPtr::<i32>::from_bits(r + 0x14)),
                                        env.mem
                                            .read(crate::mem::ConstPtr::<i32>::from_bits(r + 0x18)),
                                    );
                                }
                            }
                        }
                        echo!(
                            "[DSfwd] code={:#04x} live={} owner={:#010x} ovptr={:#010x} \
                             rect=({},{},{},{}) fwd={:#010x} vptr={:#010x} list={:#010x}",
                            code,
                            on,
                            owner,
                            o_vptr,
                            rect.0,
                            rect.1,
                            rect.2,
                            rect.3,
                            f,
                            vp,
                            list
                        );
                    }
                }
            }
            // `TOUCHHLE_DS_LOG_LIVE_REGIONS=1`: whenever the set of action
            // regions the engine currently offers changes, print it. Walking
            // past the workbench / store / kinesis module with this on maps
            // every action code to the thing the player can see.
            if std::env::var_os("TOUCHHLE_DS_LOG_LIVE_REGIONS").is_some() && fnow_now % 30 == 0 {
                let live = ds_live_regions(env);
                let mut sig: Vec<u32> = live
                    .iter()
                    .map(|(c, o, x, y, w, h)| {
                        (c & 0xff) << 24
                            | ((*o) & 0xf) << 20
                            | ((*x as u32) & 0x3ff) << 10
                            | ((*y as u32) & 0x3ff)
                            | ((*w as u32) & 1) << 9
                            | ((*h as u32) & 1) << 8
                    })
                    .collect();
                sig.sort_unstable();
                let changed = match LIVE_SIG.lock() {
                    Ok(mut prev) => {
                        if *prev == sig {
                            false
                        } else {
                            *prev = sig;
                            true
                        }
                    }
                    Err(_) => false,
                };
                if changed {
                    echo!("[DSlive] f={} {} region(s) live", fnow_now, live.len());
                    for (c, o, x, y, w, h) in &live {
                        echo!(
                            "[DSlive]   code={:#04x} obj={:#010x} rect=({},{},{},{}) centre=({},{})",
                            c,
                            o,
                            x,
                            y,
                            w,
                            h,
                            *x + *w / 2,
                            *y + *h / 2
                        );
                    }
                }
            }
            // `TOUCHHLE_DS_CHASE_AT_FRAME=<frame>`: run the `TOUCHHLE_DS_CHASE`
            // pointer-path dump again at that frame. The scan-time snapshot is
            // taken before the player is anywhere near a door, so its regions
            // read as disabled/off-screen (DECOMPILE.md D-026); this is what
            // gets the *active* rectangle while the player stands at the door.
            if let Ok(spec) = std::env::var("TOUCHHLE_DS_CHASE") {
                let at: u64 = std::env::var("TOUCHHLE_DS_CHASE_AT_FRAME")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                if at != 0 && fnow_now as u64 >= at && !CHASE_DONE.swap(true, AOrd::Relaxed) {
                    let mut it = spec.split(':');
                    let path: Vec<u32> = it
                        .nth(1)
                        .map(|s| s.split('/').map(|x| ds_parse_u32(x).unwrap_or(0)).collect())
                        .unwrap_or_default();
                    let words: usize = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(16);
                    let objs: Vec<u32> = match poke_objs().lock() {
                        Ok(v) => v.clone(),
                        Err(_) => Vec::new(),
                    };
                    echo!(
                        "[DSchaseT] f={} re-dumping {} object(s) at frame {}",
                        fnow_now,
                        objs.len(),
                        at
                    );
                    for base in objs {
                        let mut cur = base;
                        for (lvl, off) in std::iter::once(None)
                            .chain(path.iter().map(Some))
                            .enumerate()
                        {
                            if lvl > 0 {
                                cur = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(
                                    cur + off.unwrap_or(&0),
                                ));
                                if cur == 0 {
                                    break;
                                }
                            }
                            let mut w = String::new();
                            for i in 0..words.min(32) {
                                let a: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(
                                    cur + (i as u32) * 4,
                                ));
                                w.push_str(&format!(" {:08x}", a));
                            }
                            echo!(
                                "[DSchaseT{}] base={:#010x} addr={:#010x}:{}",
                                lvl,
                                base,
                                cur,
                                w
                            );
                        }
                    }
                }
            }
            if crate::frameworks::opengles::gles_guest::TRACK_OBJ.load(AOrd::Relaxed) != 0
                && CHANGE.swap(false, AOrd::Relaxed)
            {
                // Cap the number of forced dumps so a byte that changes every
                // frame cannot fill the disk.
                let cap: usize = std::env::var("TOUCHHLE_DS_TRACK_MAX_DUMPS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(300);
                if FORCED.fetch_add(1, AOrd::Relaxed) < cap {
                    crate::frameworks::opengles::gles_guest::FORCE_FRAME_DUMP
                        .store(true, AOrd::Relaxed);
                } else if FORCED.load(AOrd::Relaxed) == cap + 1 {
                    echo!(
                        "[DStrack] reached TOUCHHLE_DS_TRACK_MAX_DUMPS={}; \
                         no more forced frames",
                        cap
                    );
                }
            }
        }
    }
    if mask & gles11::COLOR_BUFFER_BIT != 0 {
        if let Ok(dir) = std::env::var("TOUCHHLE_DS_DUMP_FRAMES") {
            use std::sync::atomic::{AtomicUsize, Ordering as AOrd};
            static FRAMES: AtomicUsize = AtomicUsize::new(0);
            let n = FRAMES.fetch_add(1, AOrd::Relaxed);
            let every: usize = std::env::var("TOUCHHLE_DS_DUMP_FRAME_EVERY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            let from: usize = std::env::var("TOUCHHLE_DS_DUMP_FRAME_FROM")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let to: usize = std::env::var("TOUCHHLE_DS_DUMP_FRAME_TO")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(usize::MAX);
            let rotate: i32 = std::env::var("TOUCHHLE_DS_DUMP_FRAME_ROTATE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            // Filter on the presented-frame clock, i.e. the same clock the
            // files are named with and the terse trace tags its lines with.
            // (`n` only counts colour clears, which lag behind presents.)
            let fnow =
                crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed) as usize;
            let forced = FORCE_FRAME_DUMP.swap(false, std::sync::atomic::Ordering::Relaxed);
            // NOTE: `DS_MARK` (the M/F7 key) must be able to force a dump on its
            // own frame. It used to be read *inside* this `if`, so a mark was
            // silently ignored unless the periodic filter happened to match on
            // that exact frame - which made "mark the interesting frame" useless
            // whenever the dumper ran with a narrow window.
            let marked = std::env::var_os("TOUCHHLE_DS_DUMP_AT_PRESENT").is_none()
                && crate::window::DS_MARK.swap(false, std::sync::atomic::Ordering::Relaxed);
            if forced
                || (std::env::var_os("TOUCHHLE_DS_DUMP_AT_PRESENT").is_none()
                    && (marked
                        || (fnow >= from && fnow <= to && (every == 0 || fnow % every == 0))))
            {
                if forced {
                    log_once!("TOUCHHLE_DS_TRACK_VTABLE: dumping a forced frame");
                }
                with_ctx_and_mem(env, |gles, _mem| unsafe {
                    let mut vp = [0i32; 4];
                    gles.GetIntegerv(gles11::VIEWPORT, vp.as_mut_ptr());
                    let (w, h) = (vp[2], vp[3]);
                    if w > 0 && h > 0 && w <= 4096 && h <= 4096 {
                        let mut buf = vec![0u8; (w * h * 4) as usize];
                        gles.ReadPixels(
                            0,
                            0,
                            w,
                            h,
                            gles11::RGBA,
                            gles11::UNSIGNED_BYTE,
                            buf.as_mut_ptr() as *mut _,
                        );
                        // The colour buffer is stored bottom-up and the guest
                        // renders into a portrait target that the compositor
                        // rotates, so allow an explicit rotation for the dump.
                        let (ow, oh) = if rotate == 90 || rotate == 270 {
                            (h, w)
                        } else {
                            (w, h)
                        };
                        // `TOUCHHLE_DS_DUMP_FRAME_SCALE=<n>` writes every nth
                        // pixel, so a long "filmstrip" capture fits on disk
                        // (n=2 gives 240x160 for the 480x320 landscape frame).
                        // `M` (or F7) pressed since the last dump: save this
                        // frame at full resolution with its own numbering.
                        let mut scale: i32 = std::env::var("TOUCHHLE_DS_DUMP_FRAME_SCALE")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(1)
                            .clamp(1, 8);
                        if marked {
                            scale = 1;
                        }
                        let (sw, sh) = (ow / scale, oh / scale);
                        let mut out = Vec::with_capacity((sw * sh * 3) as usize + 32);
                        let header = format!("P6\n{} {}\n255\n", sw, sh);
                        out.extend_from_slice(header.as_bytes());
                        for oy in 0..sh {
                            for ox in 0..sw {
                                let (ox, oy) = (ox * scale, oy * scale);
                                let (sx, sy) = match rotate {
                                    90 => (oy, h - 1 - ox),
                                    180 => (w - 1 - ox, h - 1 - oy),
                                    270 => (w - 1 - oy, ox),
                                    _ => (ox, oy),
                                };
                                // source is bottom-up
                                let i = (((h - 1 - sy) * w + sx) * 4) as usize;
                                out.push(buf[i]);
                                out.push(buf[i + 1]);
                                out.push(buf[i + 2]);
                            }
                        }
                        let _ = std::fs::create_dir_all(&dir);
                        // Name by the presented-frame counter as well, so a
                        // terse draw trace (`[DSTEXt] ... f=<frame>`) and these
                        // files line up without guesswork.
                        let fcur = crate::window::DS_FRAME_COUNTER
                            .load(std::sync::atomic::Ordering::Relaxed);
                        // `TOUCHHLE_DS_DUMP_FRAME_SEQ=1` puts a plain 1,2,3...
                        // sequence in front of the file name, so a user can
                        // refer to "shot #37" without quoting a frame number.
                        static SEQ: std::sync::atomic::AtomicUsize =
                            std::sync::atomic::AtomicUsize::new(0);
                        static MARKSEQ: std::sync::atomic::AtomicUsize =
                            std::sync::atomic::AtomicUsize::new(0);
                        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        let mseq = MARKSEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        let seqmode = std::env::var_os("TOUCHHLE_DS_DUMP_FRAME_SEQ").is_some();
                        // Burn the number into the picture itself, so looking at
                        // the file is enough to know which shot it is.
                        let label = if marked {
                            format!("M{}", mseq)
                        } else {
                            format!("#{}", seq)
                        };
                        ds_burn_label(&mut out, header.len(), sw as usize, sh as usize, &label);
                        let name = if seqmode {
                            if marked {
                                format!("mark_{:05}_f{:06}.ppm", mseq, fcur)
                            } else {
                                format!("shot_{:05}_f{:06}.ppm", seq, fcur)
                            }
                        } else if marked {
                            format!("mark_#{}_f{:06}.ppm", mseq, fcur)
                        } else {
                            format!("frame_{:06}.ppm", fcur)
                        };
                        match std::fs::write(std::path::Path::new(&dir).join(&name), out) {
                            Ok(()) => {
                                if marked {
                                    echo!("[DSmark] M{} f={} {}x{} -> {}", mseq, fcur, sw, sh, name)
                                } else {
                                    echo!("[DSshot] #{} f={} {}x{} -> {}", seq, fcur, sw, sh, name)
                                }
                            }
                            Err(e) => echo!("[DSshot] #{} write failed: {}", seq, e),
                        }
                        log_once!("TOUCHHLE_DS_DUMP_FRAMES: writing frames to disk");
                    }
                });
            }
        }
    }
    // Gamepad semantic combat. Aim is applied before attacks so LT+RB/RT in
    // the same poll can use the native aiming flag.
    if crate::ds_frontend_controller::enabled() {
        // Menu widgets can update without a color-buffer clear on some PC
        // drivers; drain navigation requests at every GL call boundary.
        crate::ds_frontend_controller::tick_native(env);
    }
    if mask & gles11::COLOR_BUFFER_BIT != 0 {
        if crate::ds_frontend_controller::enabled() {
            let live = ds_current_hud_player(env);
            let action = crate::ds_frontend_controller::take_scene_action();
            if let Some((hud, _)) = live.filter(|_| action != 0) {
                let state: u32 = env.mem.read(ConstPtr::from_bits(hud + 0x234));
                // Hud::IsInConfirm (0x185530) checks precisely this range.
                if (8..=13).contains(&state) {
                    let (id, begin) = if action == 1 { ("BTN_OK", 0x008e_dff4u32) }
                        else { ("BTN_NO", 0x008e_ea18u32) };
                    let expected: Vec<u8> = id.chars().flat_map(|c| (c as u32).to_le_bytes()).collect();
                    if env.mem.get_bytes_fallible(crate::mem::ConstVoidPtr::from_bits(begin), expected.len() as u32) == Some(expected.as_slice()) {
                        let event = env.mem.alloc(24);
                        let end = begin + expected.len() as u32;
                        for (i, value) in [0, 0, begin, end, end, 0].into_iter().enumerate() {
                            env.mem.write(crate::mem::MutPtr::<u32>::from_bits(event.to_bits() + i as u32 * 4), value);
                        }
                        let _: u32 = GuestFunction::from_addr_and_thumb_flag(0x0019_4158, false)
                            .call_from_host(env, (hud, event.to_bits()));
                        env.mem.free(event);
                        echo!("[DSfrontendInput] native HUD {id} state={state}");
                    }
                } else if action == 1 {
                    ds_try_death_confirm(env);
                }
            }
        }
        // Pop outside the guest call: nested callbacks must never retain this lock.
        let command = crate::window::ds_take_hud_command();
        if let Some(command) = command {
            ds_native_hud_command(env, command);
            if crate::ds_frontend_controller::enabled() { let _=ds_current_hud_player(env); }
            crate::ds_frontend_controller::pause_command_finished();
        }
        match crate::window::DS_AIM_REQUEST.swap(-1, std::sync::atomic::Ordering::Relaxed) {
            request @ 0..=2 => ds_apply_aim_request(env, request),
            _ => {}
        }
        ds_update_aim_timeout(env);
        ds_apply_pc_inputs(env);
        ds_player_shortcuts::tick(env);
    }
    let death_cancel = mask & gles11::COLOR_BUFFER_BIT != 0
        && crate::window::DS_DEATH_CANCEL_REQUEST
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        && ds_try_death_cancel(env);
    if death_cancel {
        crate::window::DS_FKEY_KINESIS.store(false, std::sync::atomic::Ordering::Relaxed);
        crate::window::DS_CONTEXT_COMBAT_REQUEST
            .store(false, std::sync::atomic::Ordering::Relaxed);
        crate::window::ds_fire_control::enqueue(crate::window::ds_fire_control::Input::Cancel);
    }
    if !death_cancel
        && mask & gles11::COLOR_BUFFER_BIT != 0
        && crate::window::DS_CONTEXT_COMBAT_REQUEST
            .swap(false, std::sync::atomic::Ordering::Relaxed)
    {
        ds_gamepad_context_combat(env);
    }
    let mouse_kinesis_launch = mask & gles11::COLOR_BUFFER_BIT != 0
        && ds_try_kinesis_launch(
            env,
            &crate::window::DS_MOUSE_KINESIS_LAUNCH_REQUEST,
            "left click",
        );
    let rt_kinesis_launch = mask & gles11::COLOR_BUFFER_BIT != 0
        && ds_try_kinesis_launch(
            env,
            &crate::window::DS_RT_KINESIS_LAUNCH_REQUEST,
            "RT",
        );
    let kinesis_launch = mouse_kinesis_launch || rt_kinesis_launch;
    if mask & gles11::COLOR_BUFFER_BIT != 0 && !kinesis_launch {
        ds_tick_fire_control(env);
    }
    // Dead Space E key: locate the interaction ring and tap it. Handled here
    // (not in the SDL event loop) because this is where the colour buffer is
    // readable. See REPORT 13.147 / TODO-SPECIAL-CONTROLS.md.
    if mask & gles11::COLOR_BUFFER_BIT != 0
        && crate::window::DS_EKEY_INTERACT.swap(false, std::sync::atomic::Ordering::Relaxed)
        && !crate::ds_frontend_controller::capture_scene_dialog()
    {
        if env.window.is_none() {
            log!("[DSe] E pressed but there is no window; ignoring");
        } else if std::env::var_os("TOUCHHLE_DS_EKEY_TAP").is_none() {
            // Default path (2026-10-16): drive the *object's* own action button
            // through the engine instead of injecting a screen tap. A tap is a
            // plain screen event and the game may read it as look/fire (user:
            // "按 E 会开枪或者变成转身"); `SendAction` cannot be misread.
            // ★ Gating: with no live region for any code in the list, E does
            // *nothing* - that is the fix for "正常时候按 E 会开枪/转身".
            if let Some(message) = ds_try_current_prompt_gesture(env) {
                echo!("{}", message);
                return;
            }
            if ds_try_death_confirm(env) {
                return;
            }

            if let Some((label,actor))=ds_press_native_station(env) {
                echo!("[DSe] native {} interaction actor={:#x} (original CanInteract + OnInteract)",label,actor);
                return;
            }

            let codes = ekey_codes();
            // Discover new drops before ranking them against already cached
            // doors/benches. QTEs were handled above and never pay this scan.
            ds_refresh_action_forwarders(env, 30);
            let (mut hit, mut live) = ds_press_scene_action_any(env, &codes);
            if hit.is_none() && ds_refresh_action_forwarders(env, 30) {
                let (refreshed_hit, refreshed_live) = ds_press_scene_action_any(env, &codes);
                hit = refreshed_hit;
                live.extend(refreshed_live);
                live.sort_unstable();
                live.dedup();
                if hit.is_some() {
                    echo!("[DSe] refreshed action-forwarder cache after level/object change");
                }
            }
            let live_s = live
                .iter()
                .map(|c| format!("{:#x}", c))
                .collect::<Vec<_>>()
                .join(",");
            match hit {
                Some((code, f, obj, (x, y, w, h))) => echo!(
                    "[DSe] interact via engine: code={:#x} fwd={:#010x} obj={:#010x} \
                     rect=({},{},{},{}) (no screen tap) [live codes: {}]",
                    code,
                    f,
                    obj,
                    x,
                    y,
                    w,
                    h,
                    if live_s.is_empty() { "-" } else { &live_s }
                ),
                None => echo!(
                    "[DSe] no live interact region for {:?}; nothing pressed \
                     (live codes: {}; TOUCHHLE_DS_EKEY_TAP=1 restores the old tap \
                     path)",
                    codes,
                    if live_s.is_empty() { "-" } else { &live_s }
                ),
            }
        } else {
            let pt_w = env.window().size_unrotated_unscaled().0;
            // Data-coordinate prompt (TOUCHHLE_DS_PROMPT_UV): if the draw tracer
            // saw the prompt sprite within the last couple of frames, tap where it
            // actually was. This is the user-requested replacement for guessing at
            // pixels - see notes/PROMPT-UV-DESIGN.md. The colour scan stays as the
            // fallback so behaviour is unchanged when the switch is off.
            let mut from_prompt: Option<(f64, f64, u32, Option<(f32, f32)>, (f64, f64))> = None;
            if std::env::var_os("TOUCHHLE_DS_PROMPT_UV").is_some()
                || std::env::var_os("TOUCHHLE_DS_PROMPT_AT").is_some()
            {
                let f = crate::window::ds_prompt_frame();
                let now =
                    crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
                if let Some((px, py)) = crate::window::ds_prompt_xy() {
                    if f != 0 && now.saturating_sub(f) <= 2 {
                        log!(
                            "[DSe] prompt sprite (data coords) -> tap ({:.1}, {:.1})",
                            px,
                            py
                        );
                        from_prompt = Some((px as f64, py as f64, 0u32, None, (0.0f64, 0.0f64)));
                    }
                }
                if from_prompt.is_none() {
                    log!("[DSe] no prompt sprite in the last frames; falling back to the colour scan");
                }
            }
            // `TOUCHHLE_DS_EKEY_VIA_REGION=1`: tap the rectangle the game itself
            // would accept (the live `InputRegion` of the object whose +0x164 is
            // set) instead of any pixel-derived point. Needs TRACK_VTABLE=<door
            // vptr> + TRACK_ALL=1 + TOUCHHLE_DS_EKEY_FWD_VTABLE=0x00a7d1e8.
            let mut from_region: Option<(f64, f64, u32, Option<(f32, f32)>, (f64, f64))> = None;
            let mut region_no_fallback = false;
            let mut live_rect: Option<(i32, i32, i32, i32)> = None;
            // On by default now: the data-coordinate path is the one that
            // actually opens doors (D-028/D-029). `TOUCHHLE_DS_EKEY_LEGACY=1`
            // restores the old colour/prompt chain.
            if std::env::var_os("TOUCHHLE_DS_EKEY_LEGACY").is_none() {
                // The forwarder list may be empty (the vtable scan is optional,
                // and the objects are created after the level loads) - find them
                // ourselves, at most once every 30 frames.
                {
                    static LAST_SWEEP: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(0);
                    let now_f =
                        crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
                    let empty = ekey_fwds().lock().map(|v| v.is_empty()).unwrap_or(false);
                    if empty
                        && now_f
                            .saturating_sub(LAST_SWEEP.load(std::sync::atomic::Ordering::Relaxed))
                            >= 30
                    {
                        LAST_SWEEP.store(now_f, std::sync::atomic::Ordering::Relaxed);
                        let objs = ds_sweep_vptr(env, 0x00a7d1e8);
                        if !objs.is_empty() {
                            log!("[DSe] found {} action forwarder(s) by sweep", objs.len());
                            if let Ok(mut o) = ekey_fwds().lock() {
                                for a in objs {
                                    if !o.contains(&a) {
                                        o.push(a);
                                    }
                                }
                            }
                        }
                    }
                }
                // Self-sufficient path: no door list, no scan switches - just
                // "which action regions does the engine offer right now?".
                // One live region with the interact code means "you may interact
                // with that thing, and this is the rectangle to tap".
                {
                    let code: u32 = std::env::var("TOUCHHLE_DS_CODE_E")
                        .ok()
                        .and_then(|v| ds_parse_u32(&v))
                        .unwrap_or(0x10);
                    // Several regions can be live at once (door + the object in
                    // the room). Pick the one whose centre is nearest the screen
                    // centre - that is the thing in front of the player.
                    let mut best: Option<(u32, u32, i32, i32, i32, i32)> = None;
                    let mut best_d = i64::MAX;
                    for (c, o, x, y, w, h) in ds_live_regions(env)
                        .into_iter()
                        .filter(|(c, _, _, _, _, _)| *c == code)
                    {
                        let dx = (x + w / 2 - 240) as i64;
                        let dy = (y + h / 2 - 160) as i64;
                        let d = dx * dx + dy * dy;
                        if d < best_d {
                            best_d = d;
                            best = Some((c, o, x, y, w, h));
                        }
                    }
                    if let Some((c, o, x, y, w, h)) = best {
                        live_rect = Some((x, y, w, h));
                        let px = x as f64 + w as f64 / 2.0;
                        let py = y as f64 + h as f64 / 2.0;
                        log!(
                            "[DSe] live region code={:#x} obj={:#010x} \
                             rect=({},{},{},{}) -> tap ({:.0},{:.0})",
                            c,
                            o,
                            x,
                            y,
                            w,
                            h,
                            px,
                            py
                        );
                        from_region = Some((px, py, 0u32, None, (0.0f64, 0.0f64)));
                    }
                }
                // The requested data source: `TOUCHHLE_DS_PROMPT_AT` /
                // `TOUCHHLE_DS_PROMPT_UV` locate the prompt sprite and give its
                // screen centre. Use that point **when the engine's own live
                // region contains it**; otherwise fall back to the region centre
                // (measured 2026-10-11: the prompt quad's centre can sit outside
                // the tappable rect, e.g. (240,160) vs 167..216 x 124..173).
                if let (Some((ppx, ppy, ..)), Some((rx, ry, rw, rh))) = (from_prompt, live_rect) {
                    let inside = ppx as i32 >= rx - 2
                        && ppx as i32 <= rx + rw + 2
                        && ppy as i32 >= ry - 2
                        && ppy as i32 <= ry + rh + 2;
                    if inside {
                        log!(
                            "[DSe] PROMPT_AT point ({:.0},{:.0}) is inside the live \
                             region ({},{},{},{}) -> using it",
                            ppx,
                            ppy,
                            rx,
                            ry,
                            rw,
                            rh
                        );
                        from_region = Some((ppx, ppy, 0u32, None, (0.0f64, 0.0f64)));
                    } else {
                        log!(
                            "[DSe] PROMPT_AT point ({:.0},{:.0}) outside the live \
                             region ({},{},{},{}) -> using the region centre",
                            ppx,
                            ppy,
                            rx,
                            ry,
                            rw,
                            rh
                        );
                    }
                }
                let mut objs: Vec<u32> = Vec::new();
                let first = TRACK_OBJ.load(std::sync::atomic::Ordering::Relaxed);
                if first != 0 {
                    objs.push(first);
                }
                if let Ok(v) = track_all().lock() {
                    for a in v.iter() {
                        if !objs.contains(a) {
                            objs.push(*a);
                        }
                    }
                }
                let fwds: Vec<u32> = match ekey_fwds().lock() {
                    Ok(v) => v.clone(),
                    Err(_) => Vec::new(),
                };
                'outer: for o in &objs {
                    let live: u8 = env
                        .mem
                        .read(crate::mem::ConstPtr::<u8>::from_bits(*o + 0x164));
                    if live == 0 {
                        continue;
                    }
                    for f in &fwds {
                        // The forwarder's "+8" is the interactive object it
                        // belongs to (+0xc is zero) - measured 2026-10-11 in
                        // the take-48/49 dumps; using +0xc made the lookup never
                        // match, so E always fell back to the colour scan.
                        let owner: u32 = env
                            .mem
                            .read(crate::mem::ConstPtr::<u32>::from_bits(*f + 0x8));
                        if owner != *o {
                            continue;
                        }
                        let list: u32 = env
                            .mem
                            .read(crate::mem::ConstPtr::<u32>::from_bits(*f + 0x10));
                        if list == 0 {
                            continue;
                        }
                        let on: u8 = env
                            .mem
                            .read(crate::mem::ConstPtr::<u8>::from_bits(list + 8));
                        if on == 0 {
                            continue;
                        }
                        let begin: u32 = env
                            .mem
                            .read(crate::mem::ConstPtr::<u32>::from_bits(list + 0xc));
                        let end: u32 = env
                            .mem
                            .read(crate::mem::ConstPtr::<u32>::from_bits(list + 0x10));
                        if begin == 0 || end <= begin || end - begin > 4096 {
                            continue;
                        }
                        for i in 0..(((end - begin) / 8) as usize).min(8) {
                            let r: u32 = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(
                                begin + (i as u32) * 8,
                            ));
                            let Some((x, y, w, h)) = ds_live_region_bounds(env, r) else {
                                continue;
                            };
                            let px = x as f64 + w as f64 / 2.0;
                            let py = y as f64 + h as f64 / 2.0;
                            log!(
                                "[DSe] region data: obj={:#010x} fwd={:#010x} \
                                 rect=({},{},{},{}) -> tap ({:.0},{:.0})",
                                o,
                                f,
                                x,
                                y,
                                w,
                                h,
                                px,
                                py
                            );
                            from_region = Some((px, py, 0u32, None, (0.0f64, 0.0f64)));
                            break 'outer;
                        }
                    }
                }
                if from_region.is_none() {
                    // Data-coordinate mode must NOT fall back to the colour scan:
                    // that is what made E fire the *stasis* energy while the player
                    // was aiming (the colour point landed on a different, live
                    // action region). With the fallback off, E only acts when the
                    // engine itself offers an interact region. Set
                    // TOUCHHLE_DS_EKEY_ALLOW_FALLBACK=1 to restore the old chain.
                    if std::env::var_os("TOUCHHLE_DS_EKEY_ALLOW_FALLBACK").is_none() {
                        region_no_fallback = true;
                        log!(
                            "[DSe] no live interact InputRegion; not tapping \
                             (data-coordinate mode, no fallback)"
                        );
                    } else {
                        log!("[DSe] no live InputRegion found; falling back to prompt/colour");
                    }
                }
            }
            let hit = if region_no_fallback {
                None
            } else {
                from_region.or(from_prompt).or_else(|| {
                    with_ctx_and_mem(env, |gles, _mem| unsafe {
                        ds_find_interaction_ring(gles, pt_w, None)
                    })
                })
            };
            match hit {
                Some((x, y, area, swipe, extent)) => {
                    match swipe {
                        Some((dx, dy)) => {
                            // Arrow-shaped prompt: drag across the blob along the
                            // arrow axis (head = the wide end).
                            let (ex, ey) = extent;
                            let hx = x + dx as f64 * ex * 0.45;
                            let hy = y + dy as f64 * ey * 0.45;
                            let tx = x - dx as f64 * ex * 0.45;
                            let ty = y - dy as f64 * ey * 0.45;
                            echo!(
                                "[DSe] arrow prompt: area={} -> swipe ({:.1},{:.1})->({:.1},{:.1})",
                                area,
                                tx,
                                ty,
                                hx,
                                hy
                            );
                            env.window_mut()
                                .ds_queue_action(format!("swipe {} {} {} {}", tx, ty, hx, hy));
                        }
                        None => {
                            echo!(
                                "[DSe] interaction ring: area={} -> tap ({:.1}, {:.1})",
                                area,
                                x,
                                y
                            );
                            env.window_mut().ds_queue_action(format!("tap {} {}", x, y));
                        }
                    }
                }
                None => echo!("[DSe] no interaction ring found in this frame"),
            }
        }
    }
    // Dead Space F key: kinesis (magnet icon) prompt -> tap. The kinesis
    // marker is in the cyan family, so force family 2 (see TODO-SPECIAL-CONTROLS).
    if mask & gles11::COLOR_BUFFER_BIT != 0
        && crate::window::DS_FKEY_KINESIS.swap(false, std::sync::atomic::Ordering::Relaxed)
    {
        if env.window.is_none() {
            log!("[DSf] F pressed but there is no window; ignoring");
        } else if std::env::var_os("TOUCHHLE_DS_FKEY_TAP").is_none() {
            // Code 0x06 is the contextual aim/fire action. Its telekinesis
            // branch only runs when the player is ALREADY holding an object
            // (state 0x12); otherwise it just raises the weapon. Acquire a
            // visible telekinetic object's own action instead: OnInteract
            // checks IsTelekinesisable and calls StartTelekinesis with a target.
            let direct_target =
                if std::env::var("TOUCHHLE_DS_FKEY_DIRECT_TARGET").as_deref() == Ok("1") {
                    ds_start_nearest_kinesis_target(env)
                } else {
                    None
                };
            if direct_target.is_none() {
                if let Some((_hud, player)) = ds_current_hud_player(env) {
                    let state: u32 = env
                        .mem
                        .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x208));
                    if state == 0x12 {
                        let how = ds_press_action(env, 0x06);
                        echo!("[DSf] held-object action: {}", how);
                    } else {
                        let can_tk = GuestFunction::from_addr_and_thumb_flag(0x0003b058, false);
                        let allowed: u32 = can_tk.call_from_host(env, (player,));
                        if allowed == 0 {
                            echo!(
                                "[DSf] kinesis unavailable in current player state {}",
                                state
                            );
                        } else if let Some((fwd, obj, rect)) = ds_press_kinesis_action(env) {
                            let after: u32 = env
                                .mem
                                .read(crate::mem::ConstPtr::<u32>::from_bits(player + 0x208));
                            echo!(
                                "[DSf] visible kinesis target obj={:#010x} fwd={:#010x} rect={:?} state={}->{}",
                                obj, fwd, rect, state, after
                            );
                        } else {
                            echo!("[DSf] no enabled visible telekinetic target; nothing pressed");
                        }
                    }
                } else {
                    echo!("[DSf] no current player; nothing pressed");
                }
            }
        } else {
            let pt_w = env.window().size_unrotated_unscaled().0;
            // Data-coordinate path first: "念力互动不需要举枪" (user), so the
            // magnet prompt is simply another live `code=0x10` region on the
            // object - the same data the E key already uses. The colour finder
            // stays as the fallback. `TOUCHHLE_DS_CODE_F` overrides the code.
            let data_hit: Option<(f64, f64, u32, Option<(f32, f32)>, (f64, f64))> = {
                if ekey_fwds().lock().map(|v| v.is_empty()).unwrap_or(false) {
                    let objs = ds_sweep_vptr(env, 0x00a7d1e8);
                    if let Ok(mut o) = ekey_fwds().lock() {
                        for a in objs {
                            if !o.contains(&a) {
                                o.push(a);
                            }
                        }
                    }
                }
                let code: u32 = std::env::var("TOUCHHLE_DS_CODE_F")
                    .ok()
                    .and_then(|v| ds_parse_u32(&v))
                    .unwrap_or(0x10);
                let mut best: Option<(u32, u32, i32, i32, i32, i32)> = None;
                let mut best_d = i64::MAX;
                for (c, o, x, y, w, h) in ds_live_regions(env)
                    .into_iter()
                    .filter(|(c, _, _, _, _, _)| *c == code)
                {
                    let dx = (x + w / 2 - 240) as i64;
                    let dy = (y + h / 2 - 160) as i64;
                    let d = dx * dx + dy * dy;
                    if d < best_d {
                        best_d = d;
                        best = Some((c, o, x, y, w, h));
                    }
                }
                best.map(|(c, o, x, y, w, h)| {
                    let px = x as f64 + w as f64 / 2.0;
                    let py = y as f64 + h as f64 / 2.0;
                    echo!(
                        "[DSf] live region code={:#x} obj={:#010x} rect=({},{},{},{}) -> tap ({:.0},{:.0})",
                        c,
                        o,
                        x,
                        y,
                        w,
                        h,
                        px,
                        py
                    );
                    (px, py, 0u32, None, (0.0f64, 0.0f64))
                })
            };
            let hit = data_hit.or_else(|| {
                with_ctx_and_mem(env, |gles, _mem| unsafe {
                    ds_find_interaction_ring(gles, pt_w, Some(2))
                })
            });
            match hit {
                Some((x, y, area, _swipe, _extent)) => {
                    echo!(
                        "[DSf] kinesis prompt: area={} -> tap ({:.1}, {:.1})",
                        area,
                        x,
                        y
                    );
                    env.window_mut().ds_queue_action(format!("tap {} {}", x, y));
                }
                None => echo!("[DSf] no kinesis prompt found in this frame"),
            }
        }
    }
    // Scripted action codes (`TOUCHHLE_DS_SEND_ACTION`) - used to confirm the
    // code -> action table in one VM run (DECOMPILE.md D-034).
    if mask & gles11::COLOR_BUFFER_BIT != 0 {
        ds_scripted_actions(env);
        // Keep the window layer's "is the player aiming" flag up to date (used by
        // TOUCHHLE_DS_MOUSE_LOOK=aim).
        ds_publish_aim_flag(env);
        // N key: measure the player's own gauges right now and dump the frame,
        // so "what did that action do" is a number plus a picture.
        if crate::window::DS_PROBE.swap(false, std::sync::atomic::Ordering::Relaxed) {
            crate::ds_font_roles::report(env);
            // `TOUCHHLE_DS_EMPTY_CLIP=1`: empty the armed weapon's clip once
            // (`Weapon::SetAmmo(0)`, slot 111 / 0x000701d8) so the *next* R press
            // has something to reload - that turns "R works" into the numeric
            // criterion `clip 0/N -> N/N` in `[DSgauge]`.
            if std::env::var_os("TOUCHHLE_DS_EMPTY_CLIP").is_some() {
                static EMPTIED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !EMPTIED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    if let Some(p) = ds_find_player(env) {
                        let sel: u32 = env
                            .mem
                            .read(crate::mem::ConstPtr::<u32>::from_bits(p + 0x2ec));
                        if sel <= 8 {
                            let w: u32 = env
                                .mem
                                .read(crate::mem::ConstPtr::<u32>::from_bits(p + 0x2d4 + sel * 4));
                            if w != 0 {
                                let set_ammo =
                                    GuestFunction::from_addr_and_thumb_flag(0x000701d8, false);
                                let _: () = set_ammo.call_from_host(env, (w, 0u32));
                                echo!("[DSgauge] emptied clip of {:#010x} (SetAmmo 0)", w);
                            }
                        }
                    }
                }
            }
            match ds_find_player(env) {
                Some(p) => ds_log_player_gauges(env, p, "probe"),
                None => echo!("[DSgauge] probe: no player-owned action forwarder yet"),
            }
            ds_log_station_positions(env);
            crate::frameworks::opengles::gles_guest::FORCE_FRAME_DUMP
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    // Dead Space C key: stasis. Primary path = run the engine's own stasis
    // button by code through `SendAction` (see `ds_send_action`); the old
    // fixed-point tap stays available behind `TOUCHHLE_DS_ACTION_TAP=1`.
    if mask & gles11::COLOR_BUFFER_BIT != 0
        && crate::window::DS_CKEY_STASIS.swap(false, std::sync::atomic::Ordering::Relaxed)
    {
        if env.window.is_none() {
            log!("[DSc] C pressed but there is no window; ignoring");
        } else if !ds_stasis_acquired(env) {
            // Permanent correctness gate: a new game already has a charged
            // WeaponStasis object before acquiring it. C must not spend that
            // hidden charge or start empty-energy flashing. Ownership, not
            // chapter/energy, preserves NG+ chapter-one and native empty feedback.
            echo!("[DSc] stasis ignored: unavailable or gameplay UI owns input");
        } else {
            let code: u32 = std::env::var("TOUCHHLE_DS_CODE_C")
                .ok()
                .and_then(|v| ds_parse_u32(&v))
                // 0x0b is the stasis action: `player+0x30c` is the WeaponStasis
                // object and the code calls its vtable slot 92 (OnTryFire).
                // Measured: three presses spend three stasis charges while the
                // bullet count stays put (DECOMPILE.md D-034).
                .unwrap_or(0x0b);
            if std::env::var_os("TOUCHHLE_DS_ACTION_TAP").is_some() {
                echo!("[DSc] stasis -> tap (111, 149) [TOUCHHLE_DS_ACTION_TAP=1]");
                env.window_mut().ds_queue_action("tap 111 149".to_string());
            } else {
                // `TOUCHHLE_DS_C_REQUIRE_AIM=1`: only fire stasis while the
                // engine says the player is aiming (`+0x1e1`). The engine action
                // itself has no such check - the HUD normally gates the button -
                // so this restores the stricter reading of the original.
                let require_aim = std::env::var_os("TOUCHHLE_DS_C_REQUIRE_AIM").is_some();
                let aiming = ds_find_player(env)
                    .map(|p| {
                        env.mem
                            .read(crate::mem::ConstPtr::<u8>::from_bits(p + 0x1e1))
                            != 0
                    })
                    .unwrap_or(false);
                if require_aim && !aiming {
                    echo!(
                        "[DSc] stasis: not aiming (TOUCHHLE_DS_C_REQUIRE_AIM=1); nothing pressed"
                    );
                } else {
                    let how = ds_press_action(env, code);
                    echo!("[DSc] stasis: {how}");
                }
            }
        }
    }
    // Dead Space X key: rotate the plasma cutter (code 0x07).
    //
    // DECOMPILE.md D-036 pinned 0x07 to the engine's own sound
    // `deadspace/plasma_cutter/rotate`: it is the aiming-stance "turn the cutter
    // 90 degrees" button, not a fire action. The engine itself requires
    // aiming + idle + weapon available, so with none of that live nothing
    // happens (the log says so).
    if mask & gles11::COLOR_BUFFER_BIT != 0
        && crate::window::DS_XKEY_ROTATE.swap(false, std::sync::atomic::Ordering::Relaxed)
    {
        if env.window.is_none() {
            log!("[DSx] X pressed but there is no window; ignoring");
        } else {
            let code: u32 = std::env::var("TOUCHHLE_DS_CODE_X")
                .ok()
                .and_then(|v| ds_parse_u32(&v))
                .unwrap_or(0x07);
            let how = ds_press_action(env, code);
            echo!("[DSx] plasma cutter rotate: {how}");
        }
    }
    // Dead Space R key: reload - same engine-action path (code 0x0a is
    // `GameObjectPlayable::TryReload`, DECOMPILE.md D-034). The old colour scan
    // for the ammo box remains as the fallback when no forwarder answers.
    if mask & gles11::COLOR_BUFFER_BIT != 0
        && crate::window::DS_RKEY_RELOAD.swap(false, std::sync::atomic::Ordering::Relaxed)
    {
        if env.window.is_none() {
            log!("[DSr] R pressed but there is no window; ignoring");
        } else {
            let code: u32 = std::env::var("TOUCHHLE_DS_CODE_R")
                .ok()
                .and_then(|v| ds_parse_u32(&v))
                .unwrap_or(0x0a);
            let how = if std::env::var_os("TOUCHHLE_DS_ACTION_TAP").is_some() {
                None
            } else {
                Some(ds_press_action(env, code))
            };
            match how {
                Some(desc) => echo!("[DSr] reload: {desc}"),
                None => {
                    // `TOUCHHLE_DS_ACTION_TAP=1` (old behaviour): find the cyan
                    // ammo box in the colour buffer and tap it.
                    let pt_w = env.window().size_unrotated_unscaled().0;
                    let hit = with_ctx_and_mem(env, |gles, _mem| unsafe {
                        ds_find_weapon_box(gles, pt_w)
                    });
                    match hit {
                        Some((x, y, area)) => {
                            echo!(
                                "[DSr] TOUCHHLE_DS_ACTION_TAP: ammo box area={} -> tap ({:.1}, {:.1})",
                                area,
                                x,
                                y
                            );
                            env.window_mut().ds_queue_action(format!("tap {} {}", x, y));
                        }
                        None => echo!(
                            "[DSr] TOUCHHLE_DS_ACTION_TAP: no ammo box found; not tapping \
                             (aim mode would fire)"
                        ),
                    }
                }
            }
        }
    }

    // Debug aid: `TOUCHHLE_DS_DEPTH_PROBE=<file>` samples the depth buffer just
    // before a colour clear (i.e. at the end of the previous frame) and logs a
    // coarse 9x9 grid. A region that renders black but has depth == 1.0 had no
    // geometry written to it; depth < 1.0 means geometry is present.
    if mask & gles11::COLOR_BUFFER_BIT != 0 {
        if let Ok(trig) = std::env::var("TOUCHHLE_DS_DEPTH_PROBE") {
            if std::path::Path::new(&trig).exists() {
                use std::sync::atomic::{AtomicUsize, Ordering as AOrd};
                static N: AtomicUsize = AtomicUsize::new(0);
                let n = N.fetch_add(1, AOrd::Relaxed);
                if n < 5000 {
                    with_ctx_and_mem(env, |gles, _mem| unsafe {
                        let mut vp = [0i32; 4];
                        gles.GetIntegerv(gles11::VIEWPORT, vp.as_mut_ptr());
                        let (w, h) = (vp[2], vp[3]);
                        if w > 0 && h > 0 && w <= 2048 && h <= 2048 {
                            let mut buf = vec![0f32; (w * h) as usize];
                            gles.ReadPixels(
                                0,
                                0,
                                w,
                                h,
                                0x1902, /* GL_DEPTH_COMPONENT */
                                gles11::FLOAT,
                                buf.as_mut_ptr() as *mut _,
                            );
                            let mut line = String::new();
                            for gy in 0..9 {
                                for gx in 0..9 {
                                    let x = (w - 1) * gx / 8;
                                    let y = (h - 1) * gy / 8;
                                    let v = buf[(y * w + x) as usize];
                                    line.push_str(&format!("{:.3} ", v));
                                }
                                line.push('|');
                            }
                            log!("[DSdepth] #{} viewport={}x{} {}", n, w, h, line);
                        }
                    });
                }
            }
        }
    }
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Clear(mask) });
}
fn glClearColor(
    env: &mut Environment,
    red: GLclampf,
    green: GLclampf,
    blue: GLclampf,
    alpha: GLclampf,
) {
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glClearColor({}, {}, {}, {}) [this log will only be shown once]",
                red,
                green,
                blue,
                alpha
            );
        }
    }
    // Debug aid: force a bright magenta clear colour so a capture shows whether
    // a region is simply never drawn into (clear colour shows through) or is
    // covered by a drawn black layer.
    let (red, green, blue, alpha) = if std::env::var_os("TOUCHHLE_DS_CLEAR_MAGENTA").is_some() {
        (1.0f32, 0.0f32, 1.0f32, 1.0f32)
    } else {
        (red, green, blue, alpha)
    };
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.ClearColor(red, green, blue, alpha)
    });
}
fn glClearColorx(
    env: &mut Environment,
    red: GLclampx,
    green: GLclampx,
    blue: GLclampx,
    alpha: GLclampx,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.ClearColorx(red, green, blue, alpha)
    });
}
fn glClearDepthf(env: &mut Environment, depth: GLclampf) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.ClearDepthf(depth) });
}
fn glClearDepthx(env: &mut Environment, depth: GLclampx) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.ClearDepthx(depth) });
}
fn glClearStencil(env: &mut Environment, s: GLint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.ClearStencil(s) });
}

fn glMatrixMode(env: &mut Environment, mode: GLenum) {
    if trace_potatogold_render() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        if n < 80 {
            log!("[POTATO RENDER TRACE] glMatrixMode(0x{:x})", mode);
        }
    }

    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.MatrixMode(mode) });
}
fn glLoadIdentity(env: &mut Environment) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.LoadIdentity() });
}
fn glLoadMatrixf(env: &mut Environment, m: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let m = mem.ptr_at(m, 16);
        unsafe { gles.LoadMatrixf(m) };
    });
}
fn glLoadMatrixx(env: &mut Environment, m: ConstPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let m = mem.ptr_at(m, 16);
        unsafe { gles.LoadMatrixx(m) };
    });
}
fn glMultMatrixf(env: &mut Environment, m: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| {
        let m = mem.ptr_at(m, 16);
        unsafe { gles.MultMatrixf(m) };
    });
}
fn glMultMatrixx(env: &mut Environment, m: ConstPtr<GLfixed>) {
    with_ctx_and_mem(env, |gles, mem| {
        let m = mem.ptr_at(m, 16);
        unsafe { gles.MultMatrixx(m) };
    });
}
fn glPushMatrix(env: &mut Environment) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.PushMatrix() });
}
fn glPopMatrix(env: &mut Environment) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.PopMatrix() });
}
fn glOrthof(
    env: &mut Environment,
    left: GLfloat,
    right: GLfloat,
    bottom: GLfloat,
    top: GLfloat,
    near: GLfloat,
    far: GLfloat,
) {
    if trace_potatogold_render() {
        log!(
            "[POTATO RENDER TRACE] glOrthof(left={}, right={}, bottom={}, top={}, near={}, far={})",
            left,
            right,
            bottom,
            top,
            near,
            far
        );
    }

    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Orthof(left, right, bottom, top, near, far)
    });
}
fn glOrthox(
    env: &mut Environment,
    left: GLfixed,
    right: GLfixed,
    bottom: GLfixed,
    top: GLfixed,
    near: GLfixed,
    far: GLfixed,
) {
    if trace_potatogold_render() {
        log!(
            "[POTATO RENDER TRACE] glOrthox(left={}, right={}, bottom={}, top={}, near={}, far={})",
            left,
            right,
            bottom,
            top,
            near,
            far
        );
    }

    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Orthox(left, right, bottom, top, near, far)
    });
}
fn glFrustumf(
    env: &mut Environment,
    left: GLfloat,
    right: GLfloat,
    bottom: GLfloat,
    top: GLfloat,
    near: GLfloat,
    far: GLfloat,
) {
    if trace_potatogold_render() {
        log!(
            "[POTATO RENDER TRACE] glFrustumf(left={}, right={}, bottom={}, top={}, near={}, far={})",
            left,
            right,
            bottom,
            top,
            near,
            far
        );
    }

    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Frustumf(left, right, bottom, top, near, far)
    });
}
fn glFrustumx(
    env: &mut Environment,
    left: GLfixed,
    right: GLfixed,
    bottom: GLfixed,
    top: GLfixed,
    near: GLfixed,
    far: GLfixed,
) {
    if trace_potatogold_render() {
        log!(
            "[POTATO RENDER TRACE] glFrustumx(left={}, right={}, bottom={}, top={}, near={}, far={})",
            left,
            right,
            bottom,
            top,
            near,
            far
        );
    }

    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Frustumx(left, right, bottom, top, near, far)
    });
}
fn glRotatef(env: &mut Environment, angle: GLfloat, x: GLfloat, y: GLfloat, z: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Rotatef(angle, x, y, z) });
}
fn glRotatex(env: &mut Environment, angle: GLfixed, x: GLfixed, y: GLfixed, z: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Rotatex(angle, x, y, z) });
}
fn glScalef(env: &mut Environment, x: GLfloat, y: GLfloat, z: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Scalef(x, y, z) });
}
fn glScalex(env: &mut Environment, x: GLfixed, y: GLfixed, z: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Scalex(x, y, z) });
}
fn glTranslatef(env: &mut Environment, x: GLfloat, y: GLfloat, z: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Translatef(x, y, z) });
}
fn glTranslatex(env: &mut Environment, x: GLfixed, y: GLfixed, z: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Translatex(x, y, z) });
}

fn glPixelStorei(env: &mut Environment, pname: GLenum, param: GLint) {
    {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 40 {
            log!("[texdbg] glPixelStorei pname=0x{:x} param={}", pname, param);
        }
    }
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.PixelStorei(pname, param) })
}
fn glReadPixels(
    env: &mut Environment,
    x: GLint,
    y: GLint,
    width: GLsizei,
    height: GLsizei,
    format: GLenum,
    type_: GLenum,
    pixels: MutVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| {
        let pixels = {
            let pixel_count: GuestUSize = width.checked_mul(height).unwrap().try_into().unwrap();
            let size = image_size_estimate(pixel_count, format, type_);
            mem.ptr_at_mut(pixels.cast::<u8>(), size).cast::<GLvoid>()
        };
        unsafe { gles.ReadPixels(x, y, width, height, format, type_, pixels) }
    })
}
fn glGenTextures(env: &mut Environment, n: GLsizei, textures: MutPtr<GLuint>) {
    let shader_state=super::eagl::shader_compat_state(env);
    if env
        .framework_state
        .opengles
        .effective_ctx_missing(env.current_thread)
    {
        for i in 0..n {
            env.mem
                .write(textures + (i as GuestUSize), (i + 1) as GLuint);
        }
        return;
    }
    with_ctx_and_mem(env, |gles, mem| {
        let n_usize: GuestUSize = n.try_into().unwrap();
        let textures = mem.ptr_at_mut(textures, n_usize);
        unsafe { gles.GenTextures(n, textures); }
        if let Some(state)=&shader_state {
            let names=unsafe{std::slice::from_raw_parts(textures,n_usize as usize)};
            let mut state=state.borrow_mut();for &name in names {state.generated(name);}
        }
    })
}
fn glDeleteTextures(env: &mut Environment, n: GLsizei, textures: ConstPtr<GLuint>) {
    let shader_state=super::eagl::shader_compat_state(env);
    with_ctx_and_mem(env, |gles, mem| {
        let n_usize: GuestUSize = n.try_into().unwrap();
        let textures = mem.ptr_at(textures, n_usize);
        let texture_names = unsafe { std::slice::from_raw_parts(textures, n_usize as usize) };
        for &texture in texture_names {
            // Texture names are recycled by the game. Drop the host-side HD
            // replacement marker with the GL object so churn cannot grow the
            // global residency bookkeeping indefinitely.
            ds_clear_hd_texture_replaced(texture);
        }
        if let Some(state)=&shader_state {
            let names=unsafe{std::slice::from_raw_parts(textures,n_usize as usize)};
            let mut state=state.borrow_mut();for &name in names {state.delete(name);}
        }
        unsafe { gles.DeleteTextures(n, textures) }
    })
}
fn glActiveTexture(env: &mut Environment, texture: GLenum) {
    let shader_state=super::eagl::shader_compat_state(env);
    let actual=with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.ActiveTexture(texture);
        if texture>=gles11::TEXTURE0 && texture<gles11::TEXTURE0+8 {texture}
        else {let mut actual=0;gles.GetIntegerv(gles11::ACTIVE_TEXTURE,&mut actual);actual as u32}
    });
    if let Some(state)=shader_state {state.borrow_mut().active(actual);}

}
fn glIsTexture(env: &mut Environment, texture: GLuint) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.IsTexture(texture) })
}
fn glBindTexture(env: &mut Environment, target: GLenum, texture: GLuint) {
    let shader_state=super::eagl::shader_compat_state(env);
    if trace_potatogold_render() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        if n < 80 {
            log!(
                "[POTATO RENDER TRACE] glBindTexture(target=0x{:x}, texture={})",
                target,
                texture
            );
        }
    }

    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.BindTexture(target, texture)
    });
    if let Some(state)=shader_state {state.borrow_mut().bind(target,texture);}
}
/// If `pname` is `GL_TEXTURE_MIN_FILTER` and `param` is a mipmap min-filter
/// (e.g. `GL_NEAREST_MIPMAP_LINEAR`), return the closest non-mipmap value
/// (`GL_NEAREST` or `GL_LINEAR`). Otherwise return `param` unchanged.
///
/// On strict ES 1.1 drivers (notably ARM Mali r32p1) a texture that only had
/// level 0 uploaded becomes "incomplete" the moment the guest sets a mipmap
/// min-filter, and sampling such a texture returns black — which made the
/// LEGO Ninjago title menu render as a uniform-black quad on Mali-G57 MC2,
/// even though the LEGO splash logo (whose textures kept the touchHLE-forced
/// GL_LINEAR override from glTexImage2D) rendered fine.
fn demipmap_filter_value(pname: GLenum, param: GLint) -> GLint {
    if pname != gles11::TEXTURE_MIN_FILTER {
        return param;
    }
    let p = param as GLenum;
    let demipmapped = match p {
        gles11::NEAREST_MIPMAP_NEAREST | gles11::NEAREST_MIPMAP_LINEAR => gles11::NEAREST,
        gles11::LINEAR_MIPMAP_NEAREST | gles11::LINEAR_MIPMAP_LINEAR => gles11::LINEAR,
        _ => return param,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    static SEEN: AtomicBool = AtomicBool::new(false);
    if !SEEN.swap(true, Ordering::Relaxed) {
        log!(
            "First fix_texture_min_filter override: substituting guest's \
             glTexParameter(GL_TEXTURE_MIN_FILTER, 0x{:x}) with 0x{:x} \
             (mipmap modes leave the texture incomplete on strict ES 1.1 \
             drivers like Mali r32p1, which then samples them as black) \
             [this log will only be shown once]",
            p,
            demipmapped
        );
    }
    demipmapped as GLint
}

fn maybe_demipmap_min_filter(env: &Environment, pname: GLenum, param: GLint) -> GLint {
    if !env.options.fix_texture_min_filter {
        return param;
    }
    demipmap_filter_value(pname, param)
}

fn glTexParameteri(env: &mut Environment, target: GLenum, pname: GLenum, param: GLint) {
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glTexParameteri(target=0x{:x}, pname=0x{:x}, param=0x{:x}) [this log will only be shown once]",
                target, pname, param as u32
            );
        }
    }
    if pname == gles11::TEXTURE_CROP_RECT_OES {
        return;
    }
    let param = maybe_demipmap_min_filter(env, pname, param);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.TexParameteri(target, pname, param)
    })
}
fn glTexParameterf(env: &mut Environment, target: GLenum, pname: GLenum, param: GLfloat) {
    if pname == gles11::TEXTURE_CROP_RECT_OES {
        return;
    }
    // Floats can also be used to pass enum-valued min-filter params (an
    // OpenGL quirk), so route them through the same substitution.
    let param = maybe_demipmap_min_filter(env, pname, param as GLint) as GLfloat;
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.TexParameterf(target, pname, param)
    })
}
fn glTexParameterx(env: &mut Environment, target: GLenum, pname: GLenum, param: GLfixed) {
    if pname == gles11::TEXTURE_CROP_RECT_OES {
        return;
    }
    // Fixed-point can also encode enum values; route through substitution.
    let param = maybe_demipmap_min_filter(env, pname, param) as GLfixed;
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.TexParameterx(target, pname, param)
    })
}
fn glTexParameteriv(env: &mut Environment, target: GLenum, pname: GLenum, params: ConstPtr<GLint>) {
    if pname == gles11::TEXTURE_CROP_RECT_OES {
        with_ctx_and_mem(env, |gles, mem| unsafe {
            let params_ptr = mem.ptr_at(params, 4);
            {
                use std::sync::atomic::{AtomicBool, Ordering};
                static SEEN: AtomicBool = AtomicBool::new(false);
                if !SEEN.swap(true, Ordering::Relaxed) {
                    let crop = from_raw_parts(params_ptr, 4);
                    log!(
                        "GL_OES_draw_texture: first glTexParameteriv(GL_TEXTURE_CROP_RECT_OES) = [{}, {}, {}, {}]",
                        crop[0],
                        crop[1],
                        crop[2],
                        crop[3]
                    );
                }
            }
            gles.TexParameteriv(target, pname, params_ptr)
        });
        return;
    }
    let fix_min_filter = env.options.fix_texture_min_filter && pname == gles11::TEXTURE_MIN_FILTER;
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let params_ptr = mem.ptr_at(params, 1);
        if fix_min_filter {
            let original: GLint = *params_ptr;
            let substituted = demipmap_filter_value(pname, original);
            if substituted != original {
                let v = [substituted];
                gles.TexParameteriv(target, pname, v.as_ptr());
                return;
            }
        }
        gles.TexParameteriv(target, pname, params_ptr)
    })
}
fn glTexParameterfv(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: ConstPtr<GLfloat>,
) {
    if pname == gles11::TEXTURE_CROP_RECT_OES {
        with_ctx_and_mem(env, |gles, mem| unsafe {
            let params_ptr = mem.ptr_at(params, 4);
            {
                use std::sync::atomic::{AtomicBool, Ordering};
                static SEEN: AtomicBool = AtomicBool::new(false);
                if !SEEN.swap(true, Ordering::Relaxed) {
                    let crop = from_raw_parts(params_ptr, 4);
                    log!(
                        "GL_OES_draw_texture: first glTexParameterfv(GL_TEXTURE_CROP_RECT_OES) = [{}, {}, {}, {}]",
                        crop[0],
                        crop[1],
                        crop[2],
                        crop[3]
                    );
                }
            }
            gles.TexParameterfv(target, pname, params_ptr)
        });
        return;
    }
    let fix_min_filter = env.options.fix_texture_min_filter && pname == gles11::TEXTURE_MIN_FILTER;
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let params_ptr = mem.ptr_at(params, 1);
        if fix_min_filter {
            let original: GLfloat = *params_ptr;
            let substituted = demipmap_filter_value(pname, original as GLint) as GLfloat;
            if substituted != original {
                let v = [substituted];
                gles.TexParameterfv(target, pname, v.as_ptr());
                return;
            }
        }
        gles.TexParameterfv(target, pname, params_ptr)
    })
}
fn glTexParameterxv(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: ConstPtr<GLfixed>,
) {
    if pname == gles11::TEXTURE_CROP_RECT_OES {
        with_ctx_and_mem(env, |gles, mem| unsafe {
            let params_ptr = mem.ptr_at(params, 4);
            {
                use std::sync::atomic::{AtomicBool, Ordering};
                static SEEN: AtomicBool = AtomicBool::new(false);
                if !SEEN.swap(true, Ordering::Relaxed) {
                    let crop = from_raw_parts(params_ptr, 4);
                    log!(
                        "GL_OES_draw_texture: first glTexParameterxv(GL_TEXTURE_CROP_RECT_OES) = [{}, {}, {}, {}]",
                        crop[0],
                        crop[1],
                        crop[2],
                        crop[3]
                    );
                }
            }
            gles.TexParameterxv(target, pname, params_ptr)
        });
        return;
    }
    let fix_min_filter = env.options.fix_texture_min_filter && pname == gles11::TEXTURE_MIN_FILTER;
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let params_ptr = mem.ptr_at(params, 1);
        if fix_min_filter {
            let original: GLfixed = *params_ptr;
            let substituted = demipmap_filter_value(pname, original) as GLfixed;
            if substituted != original {
                let v = [substituted];
                gles.TexParameterxv(target, pname, v.as_ptr());
                return;
            }
        }
        gles.TexParameterxv(target, pname, params_ptr)
    })
}
fn image_size_estimate(pixel_count: GuestUSize, format: GLenum, type_: GLenum) -> GuestUSize {
    // This is only an upper-bound estimate used for memory tracking, not
    // anything OpenGL actually needs. Unknown combos should yield a
    // conservative "don't try to copy guest pixels" sentinel (0) and log,
    // not crash the host. The driver will catch real format issues.
    let bytes_per_pixel: Option<GuestUSize> = match type_ {
        gles11::UNSIGNED_BYTE => match format {
            gles11::ALPHA | gles11::LUMINANCE => Some(1),
            gles11::LUMINANCE_ALPHA => Some(2),
            gles11::RGB => Some(3),
            gles11::RGBA | gles11::BGRA_EXT => Some(4),
            _ => None,
        },
        gles11::UNSIGNED_SHORT_5_6_5
        | gles11::UNSIGNED_SHORT_4_4_4_4
        | gles11::UNSIGNED_SHORT_5_5_5_1 => Some(2),
        _ => None,
    };
    let Some(bpp) = bytes_per_pixel else {
        log!(
            "Warning: image_size_estimate(): unsupported format/type combination (format={:#x}, type={:#x}); treating as 0 bytes.",
            format,
            type_
        );
        return 0;
    };
    pixel_count.checked_mul(bpp).unwrap_or_else(|| {
        log!(
            "Warning: image_size_estimate(): pixel_count {} * bpp {} overflowed GuestUSize; returning u32::MAX.",
            pixel_count,
            bpp
        );
        GuestUSize::MAX
    })
}

/// `TOUCHHLE_DS_TEX_MIP=1`: after a level-0 texture was replaced by an HD pack
/// file, build a **mip chain from our picture** and keep a mipmapped min filter.
///
/// Why this is needed: the pack path otherwise drops mips and forces
/// `GL_LINEAR` (correct while the replacement is as blurry as the original, and
/// required so the app's PVRTC mips - which are mips of the *original* low-res
/// picture - can't win at distance). But the moment a pack carries more detail
/// than the source, a non-mipmapped minification filter makes distant surfaces
/// alias/shimmer badly, which is what makes cheap "HD packs" look worse than the
/// original at range. Generating our own chain fixes both ends: magnified
/// surfaces use the bigger level 0, minified surfaces filter through our mips.
///
/// The texture also stays *complete* either way, so the guest's own
/// `glTexParameteri(GL_TEXTURE_MIN_FILTER, …)` can no longer sample it as black.
unsafe fn ds_hd_make_mips(gles: &mut dyn GLES) {
    if std::env::var_os("TOUCHHLE_DS_TEX_MIP").is_none() {
        return;
    }
    gles.GenerateMipmapOES(gles11::TEXTURE_2D);
    gles.TexParameteri(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_MIN_FILTER,
        gles11::LINEAR_MIPMAP_LINEAR as GLint,
    );
    log_once!(
        "[DSTEX mip] built a mip chain for a replaced texture (TOUCHHLE_DS_TEX_MIP=1) \
         [this log will only be shown once]"
    );
}

unsafe fn ds_upload_cpu_alpha_mips(
    gles: &mut dyn GLES, target: GLenum, width: u32, height: u32, base: &[u8],
) -> bool {
    let mut source = std::borrow::Cow::Borrowed(base);
    let (mut sw, mut sh) = (width, height);
    let mut level = 1;
    while sw > 1 || sh > 1 {
        let Some((nw, nh, next)) = crate::ds_texture_mips::next_rgba(&source, sw, sh) else { return false; };
        gles.TexImage2D(target, level, gles11::RGBA as GLint, nw as GLsizei, nh as GLsizei,
            0, gles11::RGBA, gles11::UNSIGNED_BYTE, next.as_ptr() as *const GLvoid);
        source = std::borrow::Cow::Owned(next); sw = nw; sh = nh; level += 1;
    }
    true
}

// Replacement atlases need the same transparent-edge repair as original PVRTC.
unsafe fn ds_hd_make_mips_for_key(
    gles: &mut dyn GLES, target: GLenum, key: u32, width: u32, height: u32, pixels: &[u8],
) {
    if std::env::var_os("TOUCHHLE_DS_TEX_MIP").is_none() { return; }
    let alpha = std::env::var("TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH").ok()
        .map(|spec| crate::ds_texid::parse_keys(&spec).contains(&key)).unwrap_or(false);
    if alpha && ds_upload_cpu_alpha_mips(gles, target, width, height, pixels) {
        gles.TexParameteri(target, gles11::TEXTURE_MIN_FILTER, gles11::LINEAR_MIPMAP_LINEAR as GLint);
        log!("[DSTEX mip] alpha-aware replacement key={:08x} {}x{}", key, width, height);
    } else { ds_hd_make_mips(gles); }
}

/// The opening ship hull uses three PVRTC atlases whose app-supplied mip
/// chains produce broad horizontal banding on steeply minified surfaces.
/// Decoding level 0 and rebuilding the chain produces a clean, complete
/// texture while retaining mip filtering (unlike forcing GL_LINEAR).
unsafe fn ds_try_auto_opening_mips(
    gles: &mut dyn GLES,
    target: GLenum,
    texture: u32,
    key: u32,
    width: GLsizei,
    height: GLsizei,
    internalformat: GLenum,
    data: &[u8],
) -> bool {
    if std::env::var("TOUCHHLE_DS_AUTO_OPENING_MIPS").as_deref() == Ok("0") {
        return false;
    }
    // The bundled runtime uses the game's iPad2 resource tree, which changes
    // the content fingerprints even though these are the same opening
    // wall/poster, hull and lower-building atlases. Keep both resource sets so
    // the repair does not silently disappear when the IPA variant changes.
    let opening_key = matches!(
        key,
        0x7027_023c
            | 0x7bcf_a36c
            | 0x780d_00eb
            | 0x56c6_2e8a
            | 0x18c9_49fb
            | 0xd1fe_b687
            | 0x2131_afd5
    ) || std::env::var("TOUCHHLE_DS_AUTO_MIPS_HASH")
        .ok()
        .map(|spec| crate::ds_texid::parse_keys(&spec).contains(&key))
        .unwrap_or(false);
    let all_pvrtc = std::env::var("TOUCHHLE_DS_AUTO_MIPS_ALL").as_deref() == Ok("1");
    if target != gles11::TEXTURE_2D || (!opening_key && !all_pvrtc) || width <= 0 || height <= 0 {
        return false;
    }
    let is_2bit = matches!(
        internalformat,
        gles11::COMPRESSED_RGB_PVRTC_2BPPV1_IMG | gles11::COMPRESSED_RGBA_PVRTC_2BPPV1_IMG
    );
    let is_4bit = matches!(
        internalformat,
        gles11::COMPRESSED_RGB_PVRTC_4BPPV1_IMG | gles11::COMPRESSED_RGBA_PVRTC_4BPPV1_IMG
    );
    if !is_2bit && !is_4bit {
        return false;
    }
    let width = width as u32;
    let height = height as u32;
    let expected = if is_2bit {
        (width.max(16) as usize * height.max(8) as usize * 2).div_ceil(8)
    } else {
        (width.max(8) as usize * height.max(8) as usize * 4).div_ceil(8)
    };
    if data.len() != expected {
        return false;
    }
    let mut pixels = crate::image::decode_pvrtc(data, is_2bit, width, height);
    if let Ok(spec) = std::env::var("TOUCHHLE_DS_TEXFLAT_RGBA_HASH") {
        for item in spec.split(';') {
            let mut parts = item.splitn(2, ':');
            let wanted_key = parts
                .next()
                .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
            let rgba: Vec<u8> = parts
                .next()
                .unwrap_or("")
                .split(',')
                .filter_map(|value| value.trim().parse::<u8>().ok())
                .collect();
            if wanted_key == Some(key) && rgba.len() == 4 {
                let value = u32::from_le_bytes([rgba[0], rgba[1], rgba[2], rgba[3]]);
                pixels.fill(value);
                log!(
                    "[DSTEX flat] key={:08x} tex={} {}x{} rgba={:?}",
                    key,
                    texture,
                    width,
                    height,
                    rgba
                );
                break;
            }
        }
    }
    let use_cpu_alpha_mips = std::env::var("TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH")
        .ok()
        .map(|spec| crate::ds_texid::parse_keys(&spec).contains(&key))
        .unwrap_or(false);
    // Permanent content-scoped correctness repair (PC R4 accepted 2026-10-30).
    // Keep these alpha-aware chains in production defaults on both backends.
    // Performance/quality presets must not drop their key tables or substitute
    // ordinary per-channel GPU mips; that restores hull bands and dark edges.
    gles.TexImage2D(
        target,
        0,
        gles11::RGBA as GLint,
        width as GLsizei,
        height as GLsizei,
        0,
        gles11::RGBA,
        gles11::UNSIGNED_BYTE,
        pixels.as_ptr() as *const GLvoid,
    );
    let used_cpu_alpha_mips = use_cpu_alpha_mips && ds_upload_cpu_alpha_mips(
        gles, target, width, height,
        std::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), pixels.len() * 4),
    );
    if !used_cpu_alpha_mips { gles.GenerateMipmapOES(target); }
    gles.TexParameteri(
        target,
        gles11::TEXTURE_MIN_FILTER,
        gles11::LINEAR_MIPMAP_LINEAR as GLint,
    );
    ds_mark_hd_texture_replaced(texture);
    log!(
        "[DSTEX opening-mip] rebuilt mip chain key={:08x} tex={} {}x{} alpha_aware_cpu={}",
        key,
        texture,
        width,
        height,
        used_cpu_alpha_mips
    );
    true
}

/// A replacement marker shared by the uncompressed and PVRTC upload paths.
/// Once level 0 has been replaced, every later app-supplied mip level must be
/// ignored or it will overwrite the clean chain generated from level 0.
static DS_HD_REPLACED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<u32>>> =
    std::sync::OnceLock::new();

fn ds_hd_texture_was_replaced(texture: u32) -> bool {
    DS_HD_REPLACED
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
        .lock()
        .map(|set| set.contains(&texture))
        .unwrap_or(false)
}

fn ds_mark_hd_texture_replaced(texture: u32) {
    if let Ok(mut set) = DS_HD_REPLACED
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
        .lock()
    {
        set.insert(texture);
    }
}

fn ds_clear_hd_texture_replaced(texture: u32) {
    if let Ok(mut set) = DS_HD_REPLACED
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
        .lock()
    {
        set.remove(&texture);
    }
}

/// Look up (and cache) an HD replacement picture for a texture content key:
/// `<dir>/<key:08x>.png` -> `(width, height, RGBA bytes, path)`. The cache is
/// keyed by path so a repeated upload of the same artwork decodes once.
type HdCacheValue = Option<(u32, u32, Arc<Vec<u8>>, String)>;

struct HdCacheState {
    entries: HashMap<String, (HdCacheValue, u64)>,
    bytes: usize,
    serial: u64,
    limit_bytes: usize,
}

impl HdCacheState {
    fn new(limit_bytes: usize) -> Self {
        Self { entries: HashMap::new(), bytes: 0, serial: 0, limit_bytes }
    }

    fn value_bytes(value: &HdCacheValue) -> usize {
        value.as_ref().map_or(0, |(_, _, pixels, _)| pixels.len())
    }

    fn lookup(&mut self, path: &str) -> Option<HdCacheValue> {
        self.serial = self.serial.wrapping_add(1);
        let value = self.entries.get_mut(path)?;
        value.1 = self.serial;
        Some(value.0.clone())
    }

    fn insert(&mut self, path: String, value: HdCacheValue) {
        self.serial = self.serial.wrapping_add(1);
        let bytes = Self::value_bytes(&value);
        if let Some((old, _)) = self.entries.insert(path, (value, self.serial)) {
            self.bytes = self.bytes.saturating_sub(Self::value_bytes(&old));
        }
        self.bytes = self.bytes.saturating_add(bytes);
        while self.limit_bytes != 0 && self.bytes > self.limit_bytes {
            let Some(victim) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, stamp))| *stamp)
                .map(|(path, _)| path.clone())
            else { break; };
            let Some((old, _)) = self.entries.remove(&victim) else { break; };
            self.bytes = self.bytes.saturating_sub(Self::value_bytes(&old));
        }
    }
}

fn ds_hd_texture(
    dir: &str,
    key: u32,
) -> Option<(u32, u32, Arc<Vec<u8>>, String)> {
    static LIMIT_BYTES: OnceLock<usize> = OnceLock::new();
    let limit_bytes = *LIMIT_BYTES.get_or_init(|| {
        let megabytes = std::env::var("TOUCHHLE_DS_HD_CACHE_MB")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let bytes = megabytes.saturating_mul(1024 * 1024);
        if bytes != 0 {
            echo!("[DSTEX cache] HD decoded cache limit={} MiB", megabytes);
        }
        bytes
    });
    static CACHE: OnceLock<Mutex<HdCacheState>> = OnceLock::new();
    let path = format!("{}/{:08x}.png", dir.trim_end_matches(['/', '\\']), key);
    let cache = CACHE.get_or_init(|| Mutex::new(HdCacheState::new(limit_bytes)));
    if let Ok(mut m) = cache.lock() {
        if let Some(v) = m.lookup(&path) {
            let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::HdCacheHit,v.as_ref().map_or(0,|entry|entry.2.len() as u64),key as u64);
            return v;
        }
    }
    let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::HdDecode,1,key as u64);
    let decoded = std::fs::read(&path)
        .ok()
        .and_then(|bytes| crate::image::Image::from_bytes(&bytes).ok())
        .map(|img| {
            let (w, h) = img.dimensions();
            (w, h, Arc::new(img.pixels().to_vec()), path.clone())
        });
    if let Ok(mut m) = cache.lock() {
        m.insert(path.clone(), decoded.clone());
    }
    decoded
}

/// `TOUCHHLE_DS_ANISO=4|8|16`: request hardware anisotropic filtering for
/// every app texture. The value is clamped to the driver-reported maximum.
fn ds_apply_anisotropy(gles: &mut dyn GLES) {
    const TEXTURE_MAX_ANISOTROPY_EXT: GLenum = 0x84fe;
    const MAX_TEXTURE_MAX_ANISOTROPY_EXT: GLenum = 0x84ff;
    // The driver limit is immutable for the lifetime of a loaded GL device,
    // yet the old path queried it for every level-0 texture upload. At high
    // internal resolution this turns a cheap texture parameter into a large
    // stream of guest→HLE→driver queries. Keep the old live query behind an
    // explicit rollback switch for unusual multi-driver hosts.
    static CACHE_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let cache_enabled = *CACHE_ENABLED.get_or_init(|| {
        std::env::var("TOUCHHLE_DS_ANISO_CACHE").as_deref() != Ok("0")
    });
    let requested: GLfloat = if cache_enabled {
        static REQUESTED: std::sync::OnceLock<GLfloat> = std::sync::OnceLock::new();
        *REQUESTED.get_or_init(|| {
            std::env::var("TOUCHHLE_DS_ANISO")
                .ok()
                .and_then(|v| v.trim().parse::<f32>().ok())
                .filter(|v| v.is_finite() && *v >= 1.0)
                .unwrap_or(1.0)
        })
    } else {
        std::env::var("TOUCHHLE_DS_ANISO")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v >= 1.0)
            .unwrap_or(1.0)
    };
    if requested <= 1.0 {
        static DEFAULT_REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !DEFAULT_REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            crate::ds_startup_report::anisotropy(1.0, 1.0, true);
        }
        return;
    }
    static DRIVER_MAX: std::sync::OnceLock<GLfloat> = std::sync::OnceLock::new();
    let mut max: GLfloat = 1.0;
    unsafe {
        if cache_enabled {
            max = *DRIVER_MAX.get_or_init(|| {
                let mut queried = 1.0;
                gles.GetFloatv(MAX_TEXTURE_MAX_ANISOTROPY_EXT, &mut queried);
                queried.max(1.0)
            });
        } else {
            gles.GetFloatv(MAX_TEXTURE_MAX_ANISOTROPY_EXT, &mut max);
            max = max.max(1.0);
        }
        let value = requested.min(max.max(1.0));
        gles.TexParameterf(gles11::TEXTURE_2D, TEXTURE_MAX_ANISOTROPY_EXT, value);
        use std::sync::atomic::{AtomicBool, Ordering};
        static LOGGED: AtomicBool = AtomicBool::new(false);
        if !LOGGED.swap(true, Ordering::Relaxed) {
            let mut actual = 0.0;
            if crate::ds_startup_report::enabled() {
                gles.GetTexParameterfv(gles11::TEXTURE_2D, TEXTURE_MAX_ANISOTROPY_EXT, &mut actual);
                crate::ds_startup_report::anisotropy(requested, actual, actual >= 1.0);
            }
            log!(
                "[DSTEX aniso] requested={} driver_max={} actual={}",
                requested,
                max,
                value
            );
        }
    }
}

fn ds_synchronize_texture_upload(gles: &mut dyn GLES, payload: bool) {
    use crate::ds_perf_control::TextureSyncMode;
    let mode = crate::ds_perf_control::texture_upload_sync();
    // Count before Off's early return. A NULL/zero-area submission is not
    // evidence of texture payload work in a loading-state paired experiment.
    crate::ds_perf::texture_submission(mode, payload);
    static LAST_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(u8::MAX);
    let code = match mode {
        TextureSyncMode::Off => 0,
        TextureSyncMode::Flush => 1,
        TextureSyncMode::Finish => 2,
    };
    if LAST_MODE.swap(code, std::sync::atomic::Ordering::Relaxed) != code {
        echo!("[DSTEX sync] mode={mode:?}");
    }
    if !should_synchronize_texture_upload(mode, payload) {
        return;
    }
    let _profile = crate::ds_perf::Timer::new(crate::ds_perf::Stage::TextureSync);
    unsafe {
        match mode {
            TextureSyncMode::Off => {}
            TextureSyncMode::Flush => gles.Flush(),
            TextureSyncMode::Finish => gles.Finish(),
        }
    }
}

/// Empty/zero-area texture submissions have no pixel payload whose completion
/// needs to be waited on. The GL call itself remains ordered; avoid turning a
/// storage allocation or rejected upload into a full driver finish.
fn should_synchronize_texture_upload(
    mode: crate::ds_perf_control::TextureSyncMode,
    payload: bool,
) -> bool {
    payload && mode != crate::ds_perf_control::TextureSyncMode::Off
}

fn glTexImage2D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    internalformat: GLint,
    width: GLsizei,
    height: GLsizei,
    border: GLint,
    format: GLenum,
    type_: GLenum,
    pixels: ConstVoidPtr,
) {
    let shader_texture_state=super::eagl::shader_compat_state(env);
    if let Some(state)=&shader_texture_state {state.borrow().invalidate_upload(target,level);}

    let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::TexImage,if pixels.is_null(){0}else{width.max(0) as u64*height.max(0) as u64},format as u64);
    {
        use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glTexImage2D({}x{}, internalformat=0x{:x}, format=0x{:x}, type=0x{:x}) (app uploading texture data) [this log will only be shown once]",
                width, height, internalformat as u32, format, type_
            );
        }

        {
            static COUNT: AtomicU32 = AtomicU32::new(0);
            let n = COUNT.fetch_add(1, Ordering::Relaxed);
            if n < 80 {
                log!(
                    "[texdbg] glTexImage2D #{} size={}x{} level={} internal=0x{:x} format=0x{:x} type=0x{:x} pixels_null={}",
                    n,
                    width,
                    height,
                    level,
                    internalformat as u32,
                    format,
                    type_,
                    pixels.is_null()
                );
            }
        }
        if trace_potatogold_render() {
            static COUNT: AtomicU32 = AtomicU32::new(0);
            let n = COUNT.fetch_add(1, Ordering::Relaxed);
            if n < 80 {
                log!(
                    "[POTATO RENDER TRACE] glTexImage2D #{} target=0x{:x} level={} size={}x{} internal=0x{:x} format=0x{:x} type=0x{:x} pixels_null={}",
                    n + 1,
                    target,
                    level,
                    width,
                    height,
                    internalformat as u32,
                    format,
                    type_,
                    pixels.is_null()
                );
            }
        }
    }
    let fix_filter = env.options.fix_texture_min_filter && level == 0;
    // Textures whose level 0 was replaced from an HD PNG (TOUCHHLE_DS_TEX_OVERRIDE /
    // _TEXDIR): their remaining PVRTC mip levels must NOT be uploaded, otherwise
    // the sampler keeps reading the original low-res picture through the mip
    // chain and the replacement is invisible (measured 2026-10-12: 4 overrides
    // applied, screen unchanged). The min filter is forced away from mipmaps in
    // the override branch so the texture stays complete.
    let mut cur: GLint = 0;
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut cur);
    });
    if level == 0 {
        ds_clear_hd_texture_replaced(cur as u32);
    } else if ds_hd_texture_was_replaced(cur as u32) {
        log!(
            "[DSTEX override] dropping level {} upload for tex {}",
            level,
            cur
        );
        return;
    }
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let pixels = if pixels.is_null() {
            std::ptr::null()
        } else {
            let pixel_count: GuestUSize = width.checked_mul(height).unwrap().try_into().unwrap();
            let size = image_size_estimate(pixel_count, format, type_);
            mem.ptr_at(pixels.cast::<u8>(), size).cast::<GLvoid>()
        };
        // `TOUCHHLE_DS_TEXDIR=<folder>`: HD texture *injection*. If a level-0
        // upload's content fingerprint matches `<folder>/<key:08x>.png`, that
        // PNG is uploaded instead of the app's picture (any size - the game's
        // UVs are normalised, so a bigger file simply carries more detail).
        // The extraction half already exists (`tools/texdump-index.py` names its
        // PNGs by the same keys). Has to happen HERE, after `mem.ptr_at`: the
        // pointer above is only a *host* pointer at this point (outside the
        // closure the same variable is still a guest address, so an override
        // applied there would be translated a second time and lost).
        let mut hd_hold: Option<std::sync::Arc<Vec<u8>>> = None;
        let (width, height, format, type_, pixels) = {
            let (mut w, mut h, mut f, mut t, mut p) = (width, height, format, type_, pixels);
            if level == 0 && target == gles11::TEXTURE_2D && !p.is_null() && w > 0 && h > 0 {
                if let Ok(dir) = std::env::var("TOUCHHLE_DS_TEXDIR") {
                    let channels = match (f, t) {
                        (gles11::ALPHA, gles11::UNSIGNED_BYTE) => 1usize,
                        (gles11::LUMINANCE, gles11::UNSIGNED_BYTE) => 1,
                        (gles11::LUMINANCE_ALPHA, gles11::UNSIGNED_BYTE) => 2,
                        (gles11::RGB, gles11::UNSIGNED_BYTE) => 3,
                        (gles11::RGBA, gles11::UNSIGNED_BYTE) => 4,
                        (gles11::BGRA_EXT, gles11::UNSIGNED_BYTE) => 4,
                        _ => 0,
                    };
                    if channels != 0 {
                        let len = (w as usize) * (h as usize) * channels;
                        let bytes = std::slice::from_raw_parts(p as *const u8, len);
                        let key = crate::ds_texid::content_key(w as u32, h as u32, level, f, bytes);
                        if let Some((fw, fh, data, path)) = ds_hd_texture(&dir, key) {
                            log!(
                                "[DStex] key {:08x} ({}x{}) replaced by {} ({}x{})",
                                key,
                                w,
                                h,
                                path,
                                fw,
                                fh
                            );
                            w = fw as GLsizei;
                            h = fh as GLsizei;
                            f = gles11::RGBA;
                            t = gles11::UNSIGNED_BYTE;
                            p = data.as_ptr() as *const GLvoid;
                            hd_hold = Some(std::sync::Arc::clone(&data));
                            let mut texture: GLint = 0;
                            gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut texture);
                            ds_mark_hd_texture_replaced(texture as u32);
                            crate::ds_monitor::texture(key,fw,fh);
                        }
                    }
                }
            }
            (w, h, f, t, p)
        };
        let _hd_alive = &hd_hold;
        if std::env::var_os("TOUCHHLE_DS_TEXTURE_TRACE").is_some() {
            let mut texture: GLint = 0;
            gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut texture);
            log!(
                "[DSTEX upload] frame={} tex={} target=0x{:x} level={} size={}x{} internal=0x{:x} format=0x{:x} type=0x{:x} pixels_null={} pixels={:?}",
                crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),
                texture,
                target,
                level,
                width,
                height,
                internalformat as u32,
                format,
                type_,
                pixels.is_null(),
                pixels
            );
        }
        // Dead Space compatibility: optionally dump uploaded textures so the
        // atlas content can be compared against the sub-rect the app samples.
        //
        // The content key is computed *before* the dump block so it is recorded
        // even when dumping is off: GL texture names are recycled (one name can
        // hold unrelated pictures at different moments of a run), so the
        // `TOUCHHLE_DS_*_HASH` debug switches address textures by this key
        // rather than by name. See `crate::ds_texid`.
        let tex_key = {
            let mut texture: GLint = 0;
            gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut texture);
            let key = if !pixels.is_null() && level == 0 {
                let channels = match (format, type_) {
                    (gles11::ALPHA, gles11::UNSIGNED_BYTE) => 1usize,
                    (gles11::LUMINANCE, gles11::UNSIGNED_BYTE) => 1,
                    (gles11::LUMINANCE_ALPHA, gles11::UNSIGNED_BYTE) => 2,
                    (gles11::RGB, gles11::UNSIGNED_BYTE) => 3,
                    (gles11::RGBA, gles11::UNSIGNED_BYTE) => 4,
                    (gles11::BGRA_EXT, gles11::UNSIGNED_BYTE) => 4,
                    _ => 0,
                };
                if channels == 0 || width <= 0 || height <= 0 {
                    0
                } else {
                    let len = (width as usize) * (height as usize) * channels;
                    let bytes = std::slice::from_raw_parts(pixels.cast::<u8>(), len);
                    crate::ds_texid::content_key(width as u32, height as u32, level, format, bytes)
                }
            } else {
                0
            };
            // Only remember uploads that actually carry the base-level picture:
            // a later mip level or a storage-only (`pixels == NULL`) call must
            // not blank out the key of the level-0 upload it belongs to.
            if key != 0 {
                if let Some(state)=&shader_texture_state {state.borrow_mut().uploaded(target,texture as u32,key);}
                crate::ds_texid::record(
                    texture as u32,
                    crate::ds_texid::TexIdent {
                        key,
                        stem: String::new(),
                    },
                );
            }
            if key != 0 && std::env::var_os("TOUCHHLE_DS_TEXHASH_LOG").is_some() {
                log!(
                    "[DSTEX key] tex={} key={:08x} {}x{} level={} format=0x{:x}",
                    texture,
                    key,
                    width,
                    height,
                    level,
                    format
                );
            }
            key
        };
        if let Ok(dir) = std::env::var("TOUCHHLE_DS_DUMP_TEX") {
            let is_rgba = format == gles11::RGBA;
            let is_rgb = format == gles11::RGB;
            if (is_rgba || is_rgb) && type_ == gles11::UNSIGNED_BYTE && level == 0 {
                use std::sync::atomic::{AtomicU32, Ordering};
                static N: AtomicU32 = AtomicU32::new(0);
                let n = N.fetch_add(1, Ordering::Relaxed);
                if n < 80 {
                    let channels = if is_rgba { 4usize } else { 3usize };
                    let count = (width as usize) * (height as usize) * channels;
                    // `pixels` is already a host pointer into guest memory
                    // (validated with ptr_at above), so read it directly.
                    if pixels.is_null() {
                        return;
                    }
                    let raw = std::slice::from_raw_parts(pixels as *const u8, count);
                    // Same `TOUCHHLE_DS_DUMP_TEX_ALPHA=1` option as the compressed
                    // path: keep the alpha channel when the upload has one.
                    let want_alpha = std::env::var_os("TOUCHHLE_DS_DUMP_TEX_ALPHA").is_some();
                    let out_channels = if want_alpha && channels == 4 { 4 } else { 3 };
                    let mut out = Vec::with_capacity(
                        (width as usize) * (height as usize) * out_channels + 64,
                    );
                    if out_channels == 4 {
                        out.extend_from_slice(
                            format!(
                                "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\n\
                                 TUPLTYPE RGB_ALPHA\nENDHDR\n",
                                width, height
                            )
                            .as_bytes(),
                        );
                    } else {
                        out.extend_from_slice(
                            format!("P6\n{} {}\n255\n", width, height).as_bytes(),
                        );
                    }
                    for px in raw.chunks_exact(channels) {
                        out.push(px[0]);
                        out.push(px[1]);
                        out.push(px[2]);
                        if out_channels == 4 {
                            out.push(px[3]);
                        }
                    }
                    let _ = std::fs::create_dir_all(&dir);
                    let trimmed = dir.trim_end_matches(|c| c == '/' || c == '\\');
                    let mut texture: GLint = 0;
                    gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut texture);
                    let file_name = format!(
                        "tex_{}_n{}__{}x{}__fmt{:x}.{}",
                        texture,
                        n,
                        width,
                        height,
                        format,
                        if out_channels == 4 { "pam" } else { "ppm" }
                    );
                    let out_path = format!("{}/{}", trimmed, file_name);
                    let _ = std::fs::write(&out_path, out);
                    let manifest = format!("{}/manifest.csv", trimmed);
                    let line = format!(
                        "{},{},{},{},{:x},{},0x{:08x}\n",
                        texture, n, width, height, format, file_name, tex_key
                    );
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&manifest)
                    {
                        use std::io::Write;
                        let _ = f.write_all(line.as_bytes());
                    }
                    log!(
                        "[DSTEX dump] tex={} fmt=0x{:x} wrote {}x{} (n={}, key={:08x}) to {}",
                        texture,
                        format,
                        width,
                        height,
                        n,
                        tex_key,
                        out_path
                    );
                }
            }
        }
        gles.TexImage2D(
            target,
            level,
            internalformat,
            width,
            height,
            border,
            format,
            type_,
            pixels,
        );
        ds_synchronize_texture_upload(gles, !pixels.is_null() && width > 0 && height > 0);
        if level == 0 {
            ds_apply_anisotropy(gles);
        }
        // Uncompressed path (`TOUCHHLE_DS_TEXDIR`) replacement: same mip
        // treatment as the compressed-override branch above.
        if hd_hold.is_some() && level == 0 {
            ds_hd_make_mips(gles);
        }
        if fix_filter {
            // Set GL_TEXTURE_MIN_FILTER to GL_LINEAR for the bound
            // texture so it isn't sampled as opaque black on strict
            // ES 1.1 drivers (notably Qualcomm Adreno) just because
            // the guest never bothered to override the default
            // GL_NEAREST_MIPMAP_LINEAR. The guest's own
            // glTexParameteri(GL_TEXTURE_MIN_FILTER, …) will override
            // this on subsequent calls — see Options::fix_texture_min_filter.
            static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !SEEN.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log!(
                    "First fix_texture_min_filter override: forcing GL_TEXTURE_MIN_FILTER=GL_LINEAR after glTexImage2D(level=0) [this log will only be shown once]"
                );
            }
            gles.TexParameteri(target, gles11::TEXTURE_MIN_FILTER, gles11::LINEAR as GLint);
        }
    })
}
fn glTexSubImage2D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    xoffset: GLint,
    yoffset: GLint,
    width: GLsizei,
    height: GLsizei,
    format: GLenum,
    type_: GLenum,
    pixels: ConstVoidPtr,
) {
    if let Some(state)=super::eagl::shader_compat_state(env) {state.borrow().invalidate_upload(target,level);}

    let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::TexSubImage,if pixels.is_null(){0}else{width.max(0) as u64*height.max(0) as u64},format as u64);
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let pixel_count: GuestUSize = width.checked_mul(height).unwrap().try_into().unwrap();
        let size = image_size_estimate(pixel_count, format, type_);
        // Some apps pass a NULL `pixels` pointer here (e.g. to leave the
        // sub-rectangle undefined / allocate only). Translating guest address 0
        // through the memory map would fault, so forward NULL verbatim.
        // (Ported from upstream touchHLE commit 2adcb16d.)
        let pixels = if pixels.is_null() {
            std::ptr::null()
        } else {
            mem.ptr_at(pixels.cast::<u8>(), size).cast::<GLvoid>()
        };
        if std::env::var_os("TOUCHHLE_DS_TEXTURE_TRACE").is_some() {
            let mut texture: GLint = 0;
            gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut texture);
            log!(
                "[DSTEX subimage] tex={} level={} x={} y={} size={}x{} format=0x{:x} type=0x{:x}",
                texture,
                level,
                xoffset,
                yoffset,
                width,
                height,
                format,
                type_
            );
        }
        gles.TexSubImage2D(
            target, level, xoffset, yoffset, width, height, format, type_, pixels,
        );
        ds_synchronize_texture_upload(gles, !pixels.is_null() && width > 0 && height > 0);
    })
}
fn glCompressedTexImage2D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
    border: GLint,
    image_size: GLsizei,
    data: ConstVoidPtr,
) {
    let shader_texture_state=super::eagl::shader_compat_state(env);
    if let Some(state)=&shader_texture_state {state.borrow().invalidate_upload(target,level);}

    let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::TexCompressed,if data.is_null(){0}else{image_size.max(0) as u64},internalformat as u64);
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First glCompressedTexImage2D({}x{}, internalformat=0x{:x}, image_size={}) (app uploading PVRTC/compressed texture) [this log will only be shown once]",
                width, height, internalformat, image_size
            );
        }
    }
    let fix_filter = env.options.fix_texture_min_filter && level == 0;
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let mut texture: GLint = 0;
        gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut texture);
        if level == 0 {
            ds_clear_hd_texture_replaced(texture as u32);
        } else {
            if ds_hd_texture_was_replaced(texture as u32) {
                log!(
                    "[DSTEX override] dropping compressed level {} upload for tex {}",
                    level,
                    texture
                );
                return;
            }
        }
        // Pre-flight: drain any sticky GL error left by the previous call
        // (e.g. an oversized RGBA8 glTexImage2D on a strict Mali driver),
        // so when we check post-upload below we can attribute a fresh error
        // to *this* compressed upload only. Without this, every PVRTC /
        // paletted upload that follows a failed glTexImage2D would report
        // a misleading "compressed upload failed" on the very first call —
        // that's exactly what was happening on Mali-G57 with Temple Run
        // (HyperHLE log: 1024x1024 RGBA8 upload immediately followed by
        // 256x256 PVRTC upload, both blamed on the PVRTC path).
        let mut drained = 0u32;
        loop {
            let e = gles.GetError();
            if e == 0 {
                break;
            }
            drained += 1;
            if drained > 16 {
                // Pathological driver that won't clear; bail out instead of
                // looping forever.
                break;
            }
        }

        let data = mem
            .ptr_at(data.cast::<u8>(), image_size.try_into().unwrap())
            .cast();
        if std::env::var_os("TOUCHHLE_DS_TEXTURE_TRACE").is_some() {
            let mut texture: GLint = 0;
            gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut texture);
            log!(
                "[DSTEX upload-compressed] frame={} tex={} target=0x{:x} level={} size={}x{} internal=0x{:x} image_size={} data={:?}",
                crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),
                texture,
                target,
                level,
                width,
                height,
                internalformat,
                image_size,
                data
            );
        }
        // Dead Space compatibility: dump decoded PVRTC uploads as PPM too, so
        // every texture (not just the handful of uncompressed ones) can be
        // reviewed by GL object id on the host.
        // Every level-0 PVRTC upload gets a stable name, used both for dumping
        // and for matching an HD replacement PNG (TOUCHHLE_DS_TEX_OVERRIDE).
        {
            let is_2bit = matches!(
                internalformat,
                gles11::COMPRESSED_RGB_PVRTC_2BPPV1_IMG | gles11::COMPRESSED_RGBA_PVRTC_2BPPV1_IMG
            );
            let is_4bit = matches!(
                internalformat,
                gles11::COMPRESSED_RGB_PVRTC_4BPPV1_IMG | gles11::COMPRESSED_RGBA_PVRTC_4BPPV1_IMG
            );
            if level == 0 && (is_2bit || is_4bit) && width > 0 && height > 0 {
                use std::sync::atomic::{AtomicU32, Ordering};
                static N_PVRTC: AtomicU32 = AtomicU32::new(0);
                let n = N_PVRTC.fetch_add(1, Ordering::Relaxed);
                let mut texture: GLint = 0;
                gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut texture);
                let stem = format!(
                    "tex_{}_pvrtc_n{}__{}x{}__fmt{:x}",
                    texture, n, width, height, internalformat
                );
                let raw = std::slice::from_raw_parts(data as *const u8, image_size as usize);
                // GL names are recycled, so record the content key of what this
                // name holds *now*; the draw-time debug switches
                // (`TOUCHHLE_DS_*_HASH`) match on this instead of the name.
                // See `crate::ds_texid`.
                let tex_key = crate::ds_texid::content_key(
                    width as u32,
                    height as u32,
                    level,
                    internalformat,
                    raw,
                );
                if let Some(state)=&shader_texture_state {state.borrow_mut().uploaded(target,texture as u32,tex_key);}
                crate::ds_texid::record(
                    texture as u32,
                    crate::ds_texid::TexIdent {
                        key: tex_key,
                        stem: stem.clone(),
                    },
                );
                if std::env::var_os("TOUCHHLE_DS_TEXHASH_LOG").is_some() {
                    log!(
                        // `stem=` is what the dump writes (`TOUCHHLE_DS_DUMP_TEX`)
                        // and what `TOUCHHLE_DS_TEX_OVERRIDE` looks for, so one run
                        // of the log gives the complete key <-> stem table that an
                        // HD pack needs (tools/ds-texmap.py parses it).
                        "[DSTEX key] tex={} key={:08x} {}x{} level={} internal=0x{:x} (pvrtc) stem={}",
                        texture,
                        tex_key,
                        width,
                        height,
                        level,
                        internalformat,
                        stem
                    );
                }
                // HD texture override: if a PNG named after this texture exists in
                // TOUCHHLE_DS_TEX_OVERRIDE (either directly or in its png/ subdir),
                // upload that instead of the app's PVRTC data. Lets an upscaled
                // texture pack be dropped in without touching the IPA.
                if let Ok(ovr) = std::env::var("TOUCHHLE_DS_TEX_OVERRIDE") {
                    for cand in [
                        // Content-key naming first: `ds_texid::content_key` is a
                        // fingerprint of the *picture*, so an HD pack keyed this
                        // way survives changes in GL name recycling or in the
                        // order textures happen to be uploaded. Same scheme the
                        // uncompressed path (`TOUCHHLE_DS_TEXDIR`) uses.
                        std::path::Path::new(&ovr).join(format!("{:08x}.png", tex_key)),
                        std::path::Path::new(&ovr).join(format!("{}.png", stem)),
                        std::path::Path::new(&ovr)
                            .join("png")
                            .join(format!("{}.png", stem)),
                    ] {
                        if let Ok(bytes) = std::fs::read(&cand) {
                            match crate::image::Image::from_bytes(&bytes) {
                                Ok(img) => {
                                    let (iw, ih) = img.dimensions();
                                    let pixels = img.pixels();
                                    log!(
                                        "[DSTEX override] tex={} {}x{} -> {} ({}x{})",
                                        texture,
                                        width,
                                        height,
                                        cand.display(),
                                        iw,
                                        ih
                                    );
                                    gles.TexImage2D(
                                        target,
                                        level,
                                        gles11::RGBA as GLint,
                                        iw as GLsizei,
                                        ih as GLsizei,
                                        0,
                                        gles11::RGBA,
                                        gles11::UNSIGNED_BYTE,
                                        pixels.as_ptr() as *const _,
                                    );
                                    if level == 0 {
                                        ds_apply_anisotropy(gles);
                                    }
                                    // No mips from the *app* for a replaced texture:
                                    // its own mip levels are PVRTC of the original
                                    // picture and would win at distance. With
                                    // `TOUCHHLE_DS_TEX_MIP=1` we instead build a mip
                                    // chain from our replacement; otherwise stay
                                    // non-mipmapped (and complete) as before.
                                    if std::env::var_os("TOUCHHLE_DS_TEX_MIP").is_some() {
                                        ds_hd_make_mips_for_key(gles, target, tex_key, iw, ih, pixels);
                                    } else {
                                        gles.TexParameteri(
                                            gles11::TEXTURE_2D,
                                            gles11::TEXTURE_MIN_FILTER,
                                            gles11::LINEAR as GLint,
                                        );
                                    }
                                    ds_mark_hd_texture_replaced(texture as u32);
                                    crate::ds_monitor::texture(tex_key,iw,ih);
                                    return;
                                }
                                Err(e) => {
                                    log!("[DSTEX override] {}: {}", cand.display(), e);
                                }
                            }
                        }
                    }
                }
                // Prefer a valid selected replacement; retain the original-atlas
                // repair whenever no replacement could be loaded.
                if ds_try_auto_opening_mips(
                    gles,
                    target,
                    texture as u32,
                    tex_key,
                    width,
                    height,
                    internalformat,
                    raw,
                ) {
                    return;
                }
                if let Ok(dir) = std::env::var("TOUCHHLE_DS_DUMP_TEX") {
                    if n < 400 {
                        let raw =
                            std::slice::from_raw_parts(data as *const u8, image_size as usize);
                        let px =
                            crate::image::decode_pvrtc(raw, is_2bit, width as u32, height as u32);
                        // `TOUCHHLE_DS_DUMP_TEX_ALPHA=1` writes PAM (P7, RGBA) instead of
                        // RGB PPM. PVRTC always carries four channels of data, and the
                        // game may sample `.a` even from an "RGB" (`0x8c00`) texture -
                        // measured 2026-10-13: a pack built from the RGB-only dump turned
                        // one such mask into an opaque plane and flooded the menu
                        // background at scale-hack 3.
                        let want_alpha = std::env::var_os("TOUCHHLE_DS_DUMP_TEX_ALPHA").is_some();
                        let channels = if want_alpha { 4 } else { 3 };
                        let mut out = Vec::with_capacity(px.len() * channels + 64);
                        if want_alpha {
                            out.extend_from_slice(
                                format!(
                                    "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\n\
                                 TUPLTYPE RGB_ALPHA\nENDHDR\n",
                                    width, height
                                )
                                .as_bytes(),
                            );
                        } else {
                            out.extend_from_slice(
                                format!("P6\n{} {}\n255\n", width, height).as_bytes(),
                            );
                        }
                        for p in &px {
                            out.push((p & 0xff) as u8);
                            out.push(((p >> 8) & 0xff) as u8);
                            out.push(((p >> 16) & 0xff) as u8);
                            if want_alpha {
                                out.push(((p >> 24) & 0xff) as u8);
                            }
                        }
                        let _ = std::fs::create_dir_all(&dir);
                        let file_name = if want_alpha {
                            format!("{}.pam", stem)
                        } else {
                            format!("{}.ppm", stem)
                        };
                        let base = std::path::PathBuf::from(&dir);
                        let out_path = base.join(&file_name);
                        let _ = std::fs::write(&out_path, out);
                        // Keep the raw compressed payload too, so we can tell
                        // "the app uploaded black" apart from "we decoded it wrong".
                        if std::env::var_os("TOUCHHLE_DS_DUMP_TEX_RAW").is_some() {
                            let raw_dir = base.join("raw");
                            let _ = std::fs::create_dir_all(&raw_dir);
                            let raw_name = format!(
                                "raw_tex_{}_pvrtc_n{}__{}x{}__fmt{:x}.bin",
                                texture, n, width, height, internalformat
                            );
                            let _ = std::fs::write(raw_dir.join(raw_name), raw);
                        }
                        let line = format!(
                            "{},{},{},{},{:x},{} (pvrtc),0x{:08x}\n",
                            texture, n, width, height, internalformat, file_name, tex_key
                        );
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(base.join("manifest.csv"))
                        {
                            use std::io::Write;
                            let _ = f.write_all(line.as_bytes());
                        }
                        log!(
                            "[DSTEX dump-pvrtc] tex={} {}x{} fmt=0x{:x} key={:08x} -> {:?}",
                            texture,
                            width,
                            height,
                            internalformat,
                            tex_key,
                            out_path
                        );
                    }
                }
            }
        }
        gles.CompressedTexImage2D(
            target,
            level,
            internalformat,
            width,
            height,
            border,
            image_size,
            data,
        );
        ds_synchronize_texture_upload(gles, !data.is_null() && image_size > 0 && width > 0 && height > 0);
        if level == 0 {
            ds_apply_anisotropy(gles);
        }

        // Post-flight: if the upload itself produced a fresh error (and the
        // backend didn't already software-decode the payload to RGBA8 via
        // glTexImage2D, which is the success path), report it once with
        // enough context for the user to file a bug. This is `log!`
        // (visible at default level) but rate-limited per (format, error)
        // pair so a frame-by-frame texture stream can't flood the console.
        let post_err = gles.GetError();
        if post_err != 0 {
            use std::sync::atomic::{AtomicU64, Ordering};
            // Pack (internalformat << 32) | post_err into a single 64-bit
            // slot per "ever seen" entry; cap at 8 distinct entries.
            const MAX_REPORTED: usize = 8;
            static REPORTED: [AtomicU64; MAX_REPORTED] = [
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
                AtomicU64::new(0),
            ];
            let key: u64 = ((internalformat as u64) << 32) | (post_err as u64);
            let mut already_seen = false;
            for slot in &REPORTED {
                let cur = slot.load(Ordering::Relaxed);
                if cur == key {
                    already_seen = true;
                    break;
                }
                if cur == 0
                    && slot
                        .compare_exchange(0, key, Ordering::Relaxed, Ordering::Relaxed)
                        .is_ok()
                {
                    break;
                }
            }
            if !already_seen {
                log!(
                    "Warning: glCompressedTexImage2D: host driver returned \
                     {post_err:#x} for {width}x{height} level {level} \
                     internalformat {internalformat:#x} (image_size={image_size}). \
                     This is the SOURCE of the GL error, not a sticky one — \
                     {drained} pre-existing error(s) were drained beforehand. \
                     [this format/error pair will be reported once]"
                );
            }
        }

        if fix_filter {
            gles.TexParameteri(target, gles11::TEXTURE_MIN_FILTER, gles11::LINEAR as GLint);
        }
    })
}
/// Content-key tags for texture uploads that come from the *framebuffer*
/// rather than from client memory. `content_key` uses the GL `format` enum as
/// its tag for real uploads (always < 0x10000), so these two synthetic values
/// cannot collide with an ordinary upload.
const TAG_COPYTEX_IMAGE: u32 = 0xc071_0002;
const TAG_COPYTEX_SUB_IMAGE: u32 = 0xc071_0003;

/// Largest framebuffer rectangle we are willing to read back per copy (RGBA8).
/// The HUD tiles this exists for are a few hundred pixels across; a full
/// 1024x1024 readback would stall the frame for no extra information.
const COPYTEX_HASH_MAX_PIXELS: usize = 512 * 512;

/// Fingerprint a texture whose content was copied out of the framebuffer.
///
/// Why this exists (2026-10-10): `glCopyTexImage2D` / `glCopyTexSubImage2D`
/// take their pixels from the framebuffer, so the `glTexImage2D` recorder in
/// `record`/`lookup` never saw them and the affected texture names had *no*
/// content key at all. Measured consequence: the 256x256 tile that covers the
/// chapter-12 door prompt (`tex=35`, 17x18 px of it on screen at the probe
/// point) was invisible to every content-keyed debug switch, so
/// `TOUCHHLE_DS_REDTEX_HASH` could not paint it and the "is the prompt sprite
/// actually drawn?" question stayed unanswerable.
///
/// We read the same rectangle back and hash it, which gives the name a key
/// that is stable across runs. Readback forces a pipeline sync, so this is
/// opt-in and separate from the upload-time keying:
/// `TOUCHHLE_DS_COPYTEX_HASH=1` (plus `TOUCHHLE_DS_TEXHASH_LOG=1` to log it).
unsafe fn record_copytex_key(
    gles: &mut dyn GLES,
    target: GLenum,
    x: GLint,
    y: GLint,
    level: GLint,
    width: GLsizei,
    height: GLsizei,
    tag: u32,
) -> u32 {
    // Only 2D level 0 defines "the picture"; cubemap faces and mip levels
    // would need their own bookkeeping and are not what we are hunting.
    if target != gles11::TEXTURE_2D || level != 0 {
        return 0;
    }
    if std::env::var_os("TOUCHHLE_DS_COPYTEX_HASH").is_none() {
        return 0;
    }
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || w * h > COPYTEX_HASH_MAX_PIXELS {
        return 0;
    }
    let mut pixels = vec![0u8; w * h * 4];
    // RGBA/UNSIGNED_BYTE rows are always 4-byte aligned, so the default
    // PACK_ALIGNMENT of 4 is correct and we do not have to touch pixel store.
    gles.ReadPixels(
        x,
        y,
        width,
        height,
        gles11::RGBA,
        gles11::UNSIGNED_BYTE,
        pixels.as_mut_ptr().cast(),
    );
    let key = crate::ds_texid::content_key(width as u32, height as u32, level, tag, &pixels);
    let mut texture: GLint = 0;
    gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut texture);
    crate::ds_texid::record(
        texture as u32,
        crate::ds_texid::TexIdent {
            key,
            stem: String::new(),
        },
    );
    if std::env::var_os("TOUCHHLE_DS_TEXHASH_LOG").is_some() {
        log!(
            "[DSTEX key] tex={} key={:08x} {}x{} level={} format=0x{:x} \
             (fb-copy from {},{} of {}x{})",
            texture,
            key,
            width,
            height,
            level,
            tag,
            x,
            y,
            w,
            h
        );
    }
    key
}

fn glCopyTexImage2D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    internalformat: GLenum,
    x: GLint,
    y: GLint,
    width: GLsizei,
    height: GLsizei,
    border: GLint,
) {
    if let Some(state)=super::eagl::shader_compat_state(env) {state.borrow().invalidate_upload(target,level);}

    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.CopyTexImage2D(target, level, internalformat, x, y, width, height, border);
        record_copytex_key(gles, target, x, y, level, width, height, TAG_COPYTEX_IMAGE);
    })
}
fn glCopyTexSubImage2D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    xoffset: GLint,
    yoffset: GLint,
    x: GLint,
    y: GLint,
    width: GLsizei,
    height: GLsizei,
) {
    if let Some(state)=super::eagl::shader_compat_state(env) {state.borrow().invalidate_upload(target,level);}

    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.CopyTexSubImage2D(target, level, xoffset, yoffset, x, y, width, height);
        record_copytex_key(
            gles,
            target,
            x,
            y,
            level,
            width,
            height,
            TAG_COPYTEX_SUB_IMAGE,
        );
    })
}
fn glTexEnvf(env: &mut Environment, target: GLenum, pname: GLenum, param: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.TexEnvf(target, pname, param)
    })
}
fn glTexEnvx(env: &mut Environment, target: GLenum, pname: GLenum, param: GLfixed) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.TexEnvx(target, pname, param)
    })
}
fn glTexEnvi(env: &mut Environment, target: GLenum, pname: GLenum, param: GLint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.TexEnvi(target, pname, param)
    })
}
fn glTexEnvfv(env: &mut Environment, target: GLenum, pname: GLenum, params: ConstPtr<GLfloat>) {
    assert!(target == gles11::TEXTURE_ENV || target == gles11::TEXTURE_FILTER_CONTROL_EXT);
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.TexEnvfv(target, pname, params) }
    })
}
fn glTexEnvxv(env: &mut Environment, target: GLenum, pname: GLenum, params: ConstPtr<GLfixed>) {
    assert!(target == gles11::TEXTURE_ENV);
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.TexEnvxv(target, pname, params) }
    })
}
fn glTexEnviv(env: &mut Environment, target: GLenum, pname: GLenum, params: ConstPtr<GLint>) {
    assert!(target == gles11::TEXTURE_ENV);
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at(params, 4);
        unsafe { gles.TexEnviv(target, pname, params) }
    })
}
fn glMultiTexCoord4f(
    env: &mut Environment,
    target: GLenum,
    s: GLfloat,
    t: GLfloat,
    r: GLfloat,
    q: GLfloat,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.MultiTexCoord4f(target, s, t, r, q)
    })
}
fn glMultiTexCoord4x(
    env: &mut Environment,
    target: GLenum,
    s: GLfixed,
    t: GLfixed,
    r: GLfixed,
    q: GLfixed,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.MultiTexCoord4x(target, s, t, r, q)
    })
}

fn glGenFramebuffersOES(env: &mut Environment, n: GLsizei, framebuffers: MutPtr<GLuint>) {
    if env
        .framework_state
        .opengles
        .effective_ctx_missing(env.current_thread)
    {
        for i in 0..n {
            env.mem
                .write(framebuffers + (i as GuestUSize), (i + 1) as GLuint);
        }
        return;
    }
    with_ctx_and_mem(env, |gles, mem| {
        let n_usize: GuestUSize = n.try_into().unwrap();
        let framebuffers = mem.ptr_at_mut(framebuffers, n_usize);
        unsafe { gles.GenFramebuffersOES(n, framebuffers) }
    })
}
fn glGenRenderbuffersOES(env: &mut Environment, n: GLsizei, renderbuffers: MutPtr<GLuint>) {
    if env
        .framework_state
        .opengles
        .effective_ctx_missing(env.current_thread)
    {
        for i in 0..n {
            env.mem
                .write(renderbuffers + (i as GuestUSize), (i + 1) as GLuint);
        }
        return;
    }
    with_ctx_and_mem(env, |gles, mem| {
        let n_usize: GuestUSize = n.try_into().unwrap();
        let renderbuffers = mem.ptr_at_mut(renderbuffers, n_usize);
        unsafe { gles.GenRenderbuffersOES(n, renderbuffers) }
    })
}
fn glIsFramebufferOES(env: &mut Environment, framebuffer: GLuint) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.IsFramebufferOES(framebuffer)
    })
}
fn glIsRenderbufferOES(env: &mut Environment, renderbuffer: GLuint) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.IsRenderbufferOES(renderbuffer)
    })
}
/// The drawable framebuffer for the calling thread's current EAGL context, if
/// one has been set up by `-[EAGLContext renderbufferStorage:fromDrawable:]`.
///
/// Uses the same main-thread fallback as [super::State::effective_ctx_missing]
/// so worker-thread rendering (Dead Space) finds the context too.
pub(crate) fn drawable_fbo_of_current_ctx(env: &mut Environment) -> Option<GLuint> {
    let tid = env.current_thread;
    let ctx = {
        let state = &mut env.framework_state.opengles;
        (*state.current_ctx_for_thread(tid)).or(*state.current_ctx_for_thread(0))
    }?;
    let host = env.objc.borrow::<EAGLContextHostObject>(ctx);
    host.msaa_fbo.or(host.drawable_fbo)
}

/// scale-hack applies only to the EAGL drawable, never to guest-created
/// offscreen framebuffers. Offscreen render targets are allocated at their
/// requested pixel size; multiplying their viewport/scissor would clip layout
/// output and render-to-texture effects.
fn scale_hack_for_current_framebuffer(env: &mut Environment) -> GLsizei {
    let scale = env.options.scale_hack.get() as GLsizei;
    if scale <= 1
        || env
            .framework_state
            .opengles
            .effective_ctx_missing(env.current_thread)
    {
        return scale;
    }

    let drawable_fbo = drawable_fbo_of_current_ctx(env);
    with_ctx_and_mem(env, |gles, _mem| {
        let mut framebuffer: GLint = 0;
        unsafe {
            gles.GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut framebuffer);
        }
        let framebuffer = framebuffer as GLuint;
        if framebuffer == 0 || drawable_fbo == Some(framebuffer) {
            scale
        } else {
            1
        }
    })
}

fn glBindFramebufferOES(env: &mut Environment, target: GLenum, framebuffer: GLuint) {
    // On iOS guest framebuffer 0 is the EAGL drawable (the window-system
    // framebuffer). touchHLE otherwise maps it to the host SDL window, which
    // means an app that draws its final frame directly into framebuffer 0
    // never reaches the drawable renderbuffer that `presentRenderbuffer:`
    // reads back. Redirect it to the FBO holding the drawable renderbuffer.
    let requested = framebuffer;
    let framebuffer = if framebuffer == 0 {
        drawable_fbo_of_current_ctx(env).unwrap_or(0)
    } else {
        framebuffer
    };
    if std::env::var_os("TOUCHHLE_DS_MSAA_PROBE").is_some() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        if n < 32 {
            log!(
                "[DSmsaa bind] target={:#x} requested={} mapped={}",
                target,
                requested,
                framebuffer
            );
        }
    }
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        // Optional one-frame capture of every framebuffer that is retired by a
        // bind. This is diagnostic-only and helps separate scene rendering from
        // the app's offscreen compositing passes.
        if let Ok(dir) = std::env::var("TOUCHHLE_DS_DUMP_FBO_DIR") {
            let wanted_frame = std::env::var("TOUCHHLE_DS_DUMP_FBO_FRAME")
                .ok()
                .and_then(|value| value.parse::<u64>().ok());
            let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
            let mut old_fbo: GLint = 0;
            gles.GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut old_fbo);
            if wanted_frame == Some(frame) && old_fbo != 0 && old_fbo as GLuint != framebuffer {
                let mut viewport = [0i32; 4];
                gles.GetIntegerv(gles11::VIEWPORT, viewport.as_mut_ptr());
                let (w, h) = (viewport[2], viewport[3]);
                if w > 0 && h > 0 && w <= 4096 && h <= 4096 {
                    let mut rgba = vec![0u8; (w * h * 4) as usize];
                    gles.ReadPixels(
                        0,
                        0,
                        w,
                        h,
                        gles11::RGBA,
                        gles11::UNSIGNED_BYTE,
                        rgba.as_mut_ptr().cast(),
                    );
                    let mut ppm = Vec::with_capacity((w * h * 3) as usize + 32);
                    ppm.extend_from_slice(format!("P6\n{} {}\n255\n", w, h).as_bytes());
                    for y in 0..h {
                        for x in 0..w {
                            let i = (((h - 1 - y) * w + x) * 4) as usize;
                            ppm.extend_from_slice(&rgba[i..i + 3]);
                        }
                    }
                    use std::sync::atomic::{AtomicUsize, Ordering as AOrd};
                    static SEQ: AtomicUsize = AtomicUsize::new(0);
                    let seq = SEQ.fetch_add(1, AOrd::Relaxed) + 1;
                    let _ = std::fs::create_dir_all(&dir);
                    let path = std::path::Path::new(&dir)
                        .join(format!("fbo{}_{:03}_f{:06}.ppm", old_fbo, seq, frame));
                    if std::fs::write(&path, ppm).is_ok() {
                        echo!("[DSfbo] dumped {} -> {}", old_fbo, path.display());
                    }
                }
            }
        }
        gles.BindFramebufferOES(target, framebuffer)
    })
}
fn glBindRenderbufferOES(env: &mut Environment, target: GLenum, renderbuffer: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.BindRenderbufferOES(target, renderbuffer)
    })
}
fn glRenderbufferStorageOES(
    env: &mut Environment,
    target: GLenum,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
) {
    // The EAGL drawable renderbuffer is allocated by
    // EAGLContext.renderbufferStorage:fromDrawable:, which applies scale-hack
    // itself. Calls reaching this function are guest-owned framebuffers and
    // must keep the exact dimensions requested by the app.
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.RenderbufferStorageOES(target, internalformat, width, height)
    })
}
fn glFramebufferRenderbufferOES(
    env: &mut Environment,
    target: GLenum,
    attachment: GLenum,
    renderbuffertarget: GLenum,
    renderbuffer: GLuint,
) {
    let active_drawable = drawable_fbo_of_current_ctx(env);
    let drawable_only = std::env::var_os("TOUCHHLE_DS_MSAA_DRAWABLE_ONLY").is_some();
    let per_fbo_depth = std::env::var_os("TOUCHHLE_DS_MSAA_FBO_DEPTH").is_some();
    let (ctx, msaa_color, msaa_depth, msaa_samples, is_drawable_renderbuffer, drawable_fbo) = {
        let tid = env.current_thread;
        let ctx = {
            let state = &mut env.framework_state.opengles;
            (*state.current_ctx_for_thread(tid)).or(*state.current_ctx_for_thread(0))
        };
        ctx.map(|ctx| {
            let host = env.objc.borrow::<EAGLContextHostObject>(ctx);
            let is_drawable = host
                .renderbuffer_drawable_bindings
                .borrow()
                .contains_key(&renderbuffer);
            (
                ctx,
                host.msaa_color_renderbuffer,
                host.msaa_depth_renderbuffer,
                host.msaa_samples,
                is_drawable,
                host.drawable_fbo,
            )
        })
        .unwrap_or((nil, None, None, 0, false, None))
    };
    let current_fbo = with_ctx_and_mem(env, |gles, _mem| unsafe {
        let mut framebuffer: GLint = 0;
        gles.GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut framebuffer);
        framebuffer as GLuint
    });
    let cached_fbo_depth = if per_fbo_depth && ctx != nil {
        env.objc
            .borrow::<EAGLContextHostObject>(ctx)
            .msaa_depth_by_fbo
            .get(&current_fbo)
            .copied()
    } else {
        None
    };
    let mut allocated_fbo_depth = None;
    let mut failed_fbo_depth = None;
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        let msaa_allowed = !drawable_only || drawable_fbo == Some(current_fbo);
        let mut using_unique_fbo_depth = false;
        // The canonical EAGL setup binds a guest-owned viewFramebuffer and
        // attaches the drawable renderbuffer to it. Redirecting only guest
        // framebuffer 0 therefore leaves all real drawing on the single-sample
        // renderbuffer and makes the later MSAA resolve overwrite it with an
        // empty image. Substitute the drawable renderbuffer wherever the guest
        // attaches it while MSAA is active.
        let original_renderbuffer = renderbuffer;
        let color_uses_msaa = attachment == gles11::COLOR_ATTACHMENT0_OES
            && renderbuffertarget == gles11::RENDERBUFFER_OES
            && is_drawable_renderbuffer
            && msaa_color.is_some()
            && msaa_allowed;
        let renderbuffer = if color_uses_msaa {
            msaa_color.unwrap_or(renderbuffer)
        } else if attachment == gles11::DEPTH_ATTACHMENT_OES
            && renderbuffertarget == gles11::RENDERBUFFER_OES
            && msaa_allowed
            && (active_drawable == Some(current_fbo as GLuint)
                || msaa_color.is_some_and(|color| {
                    let mut attached: GLint = 0;
                    gles.GetFramebufferAttachmentParameterivOES(
                        target,
                        gles11::COLOR_ATTACHMENT0_OES,
                        gles11::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME_OES,
                        &mut attached,
                    );
                    attached as GLuint == color
                }))
        {
            if per_fbo_depth {
                let depth = cached_fbo_depth
                    .or_else(|| {
                        allocate_msaa_depth_for_fbo(
                            gles,
                            msaa_color.unwrap_or(renderbuffer),
                            msaa_samples,
                        )
                    })
                    .or(msaa_depth)
                    .unwrap_or(renderbuffer);
                if depth != renderbuffer && Some(depth) != msaa_depth {
                    allocated_fbo_depth = Some((current_fbo, depth));
                }
                using_unique_fbo_depth = depth != msaa_depth.unwrap_or(0);
                depth
            } else {
                msaa_depth.unwrap_or(renderbuffer)
            }
        } else {
            renderbuffer
        };
        if std::env::var_os("TOUCHHLE_DS_MSAA_PROBE").is_some() {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            if n < 32 {
                log!(
                    "[DSmsaa attach] fbo={} attachment={:#x} input_rb={} drawable_rb={} msaa_color={:?} msaa_depth={:?} chosen_rb={}",
                    current_fbo,
                    attachment,
                    original_renderbuffer,
                    is_drawable_renderbuffer,
                    msaa_color,
                    msaa_depth,
                    renderbuffer
                );
            }
        }
        gles.FramebufferRenderbufferOES(target, attachment, renderbuffertarget, renderbuffer);
        if color_uses_msaa {
            let depth = if per_fbo_depth {
                cached_fbo_depth.or(msaa_depth)
            } else {
                msaa_depth
            };
            if let Some(depth) = depth {
                using_unique_fbo_depth |=
                    per_fbo_depth && cached_fbo_depth == Some(depth) && Some(depth) != msaa_depth;
                // Guest apps frequently attach depth before the drawable
                // colour renderbuffer. Re-attach the matching multisample
                // depth here so the guest FBO is not left with mixed sample
                // counts (which makes every draw incomplete and black).
                gles.FramebufferRenderbufferOES(
                    target,
                    gles11::DEPTH_ATTACHMENT_OES,
                    gles11::RENDERBUFFER_OES,
                    depth,
                );
            }
        }
        // When an iOS app attaches its drawable color renderbuffer to an FBO,
        // it commonly forgets to also attach a depth renderbuffer. On real iOS
        // GPUs / desktop GL the missing depth attachment is treated leniently,
        // but a strict OpenGL ES 2.0 driver (e.g. Mesa or Mali) will then
        // discard every drawn fragment when the app enables GL_DEPTH_TEST.
        // To match the lenient behaviour we auto-create and attach a matching
        // depth renderbuffer the first time a color attachment is set up on a
        // user-managed framebuffer.
        if gles.is_es2()
            && attachment == gles11::COLOR_ATTACHMENT0_OES
            && renderbuffertarget == gles11::RENDERBUFFER_OES
            && renderbuffer != 0
        {
            // Don't clobber an explicitly-attached depth attachment.
            let mut depth_attached: GLint = 0;
            gles.GetFramebufferAttachmentParameterivOES(
                target,
                gles11::DEPTH_ATTACHMENT_OES,
                gles11::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME_OES,
                &mut depth_attached,
            );
            // Drain any error from the lookup (e.g. on freshly-attached FBO).
            while gles.GetError() != 0 {}
            if depth_attached == 0 {
                // Query renderbuffer size from the just-attached color RB.
                let mut prev_rb: GLint = 0;
                gles.GetIntegerv(gles11::RENDERBUFFER_BINDING_OES, &mut prev_rb);
                gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, renderbuffer);
                let mut w: GLint = 0;
                let mut h: GLint = 0;
                gles.GetRenderbufferParameterivOES(
                    gles11::RENDERBUFFER_OES,
                    gles11::RENDERBUFFER_WIDTH_OES,
                    &mut w,
                );
                gles.GetRenderbufferParameterivOES(
                    gles11::RENDERBUFFER_OES,
                    gles11::RENDERBUFFER_HEIGHT_OES,
                    &mut h,
                );
                if w > 0 && h > 0 {
                    let mut depth_rb: GLuint = 0;
                    gles.GenRenderbuffersOES(1, &mut depth_rb);
                    gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, depth_rb);
                    gles.RenderbufferStorageOES(
                        gles11::RENDERBUFFER_OES,
                        // GL_DEPTH_COMPONENT24_OES (0x81A6): the device this
                        // fork targets advertises GL_OES_depth24 and Dead
                        // Space's coplanar decals / layered sky need the extra
                        // precision - a 16-bit attachment makes them z-fight.
                        // The gles11 bindings only carry the 16-bit name, hence
                        // the literal.
                        0x81A6,
                        w,
                        h,
                    );
                    gles.FramebufferRenderbufferOES(
                        target,
                        gles11::DEPTH_ATTACHMENT_OES,
                        gles11::RENDERBUFFER_OES,
                        depth_rb,
                    );
                    log_dbg!(
                        "Auto-attached depth renderbuffer ({}x{}) to FBO target={:#x} attach={:#x}",
                        w,
                        h,
                        target,
                        attachment
                    );
                }
                gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, prev_rb as GLuint);
            }
            while gles.GetError() != 0 {}
        }
        if using_unique_fbo_depth
            && gles.CheckFramebufferStatusOES(target) != gles11::FRAMEBUFFER_COMPLETE_OES
        {
            // Never leave the guest on an incomplete FBO. Some drivers reject
            // a multisample depth attachment created after colour storage was
            // already established; the shared EAGL depth is the safe fallback.
            if let Some(shared_depth) = msaa_depth {
                gles.FramebufferRenderbufferOES(
                    target,
                    gles11::DEPTH_ATTACHMENT_OES,
                    gles11::RENDERBUFFER_OES,
                    shared_depth,
                );
            }
            failed_fbo_depth = Some((current_fbo, renderbuffer));
            use std::sync::atomic::{AtomicU32, Ordering};
            static FAILED_LOGS: AtomicU32 = AtomicU32::new(0);
            if FAILED_LOGS.fetch_add(1, Ordering::Relaxed) < 8 {
                log!(
                    "[DSmsaa fbo-depth] incomplete attachment for fbo={} depth={} samples={}; reverted to shared depth",
                    current_fbo,
                    renderbuffer,
                    msaa_samples
                );
            }
        }
    });
    if let (Some((fbo, depth)), true) = (failed_fbo_depth, ctx != nil) {
        env.objc
            .borrow_mut::<EAGLContextHostObject>(ctx)
            .msaa_depth_by_fbo
            .remove(&fbo);
        allocated_fbo_depth = None;
        with_ctx_and_mem(env, |gles, _mem| unsafe {
            gles.DeleteRenderbuffersOES(1, &depth);
        });
    }
    if let (Some((fbo, depth)), true) = (allocated_fbo_depth, ctx != nil) {
        env.objc
            .borrow_mut::<EAGLContextHostObject>(ctx)
            .msaa_depth_by_fbo
            .insert(fbo, depth);
    }
}

/// Allocate a multisample depth renderbuffer matching the just-attached colour
/// renderbuffer. This intentionally lives at guest-FBO granularity: Dead Space
/// alternates several FBO objects while writing the same drawable colour, and
/// sharing one depth buffer makes later composition passes inherit stale scene
/// depth.
unsafe fn allocate_msaa_depth_for_fbo(
    gles: &mut dyn GLES,
    color_renderbuffer: GLuint,
    samples: GLsizei,
) -> Option<GLuint> {
    if samples <= 1 || color_renderbuffer == 0 {
        return None;
    }
    let mut old_rb: GLint = 0;
    gles.GetIntegerv(gles11::RENDERBUFFER_BINDING_OES, &mut old_rb);
    gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, color_renderbuffer);
    let mut width: GLint = 0;
    let mut height: GLint = 0;
    gles.GetRenderbufferParameterivOES(
        gles11::RENDERBUFFER_OES,
        gles11::RENDERBUFFER_WIDTH_OES,
        &mut width,
    );
    gles.GetRenderbufferParameterivOES(
        gles11::RENDERBUFFER_OES,
        gles11::RENDERBUFFER_HEIGHT_OES,
        &mut height,
    );
    if width <= 0 || height <= 0 {
        gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, old_rb as GLuint);
        return None;
    }
    let mut depth: GLuint = 0;
    gles.GenRenderbuffersOES(1, &mut depth);
    gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, depth);
    gles.RenderbufferStorageMultisampleAPPLE(
        gles11::RENDERBUFFER_OES,
        samples,
        0x81A6, // GL_DEPTH_COMPONENT24_OES
        width,
        height,
    );
    if gles.GetError() != gles11::NO_ERROR {
        if depth != 0 {
            gles.DeleteRenderbuffersOES(1, &depth);
        }
        depth = 0;
    }
    gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, old_rb as GLuint);
    (depth != 0).then_some(depth)
}
fn glFramebufferTexture2DOES(
    env: &mut Environment,
    target: GLenum,
    attachment: GLenum,
    textarget: GLenum,
    texture: GLuint,
    level: i32,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.FramebufferTexture2DOES(target, attachment, textarget, texture, level)
    })
}
fn glGetFramebufferAttachmentParameterivOES(
    env: &mut Environment,
    target: GLenum,
    attachment: GLenum,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 1);
        unsafe { gles.GetFramebufferAttachmentParameterivOES(target, attachment, pname, params) }
    })
}
fn glGetRenderbufferParameterivOES(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    with_ctx_and_mem(env, |gles, mem| {
        let params = mem.ptr_at_mut(params, 1);
        unsafe { gles.GetRenderbufferParameterivOES(target, pname, params) };
    })
}
fn glCheckFramebufferStatusOES(env: &mut Environment, target: GLenum) -> GLenum {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.CheckFramebufferStatusOES(target)
    })
}
fn glDeleteFramebuffersOES(env: &mut Environment, n: GLsizei, framebuffers: ConstPtr<GLuint>) {
    let tid = env.current_thread;
    let ctx = {
        let state = &mut env.framework_state.opengles;
        (*state.current_ctx_for_thread(tid)).or(*state.current_ctx_for_thread(0))
    };
    let deleted_depths = ctx
        .map(|ctx| {
            let n_usize: GuestUSize = n.try_into().unwrap();
            let mut host = env.objc.borrow_mut::<EAGLContextHostObject>(ctx);
            (0..n_usize)
                .filter_map(|i| {
                    let fbo = env.mem.read(framebuffers + i);
                    host.msaa_depth_by_fbo.remove(&fbo)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    with_ctx_and_mem(env, |gles, mem| {
        let n_usize: GuestUSize = n.try_into().unwrap();
        let framebuffers = mem.ptr_at(framebuffers, n_usize);
        unsafe {
            gles.DeleteFramebuffersOES(n, framebuffers);
            for depth in deleted_depths {
                gles.DeleteRenderbuffersOES(1, &depth);
            }
        }
    });
}
fn glDeleteRenderbuffersOES(env: &mut Environment, n: GLsizei, renderbuffers: ConstPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| {
        let n_usize: GuestUSize = n.try_into().unwrap();
        let renderbuffers = mem.ptr_at(renderbuffers, n_usize);
        unsafe { gles.DeleteRenderbuffersOES(n, renderbuffers) }
    })
}
fn glGenerateMipmapOES(env: &mut Environment, target: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.GenerateMipmapOES(target) })
}

fn glGenFramebuffers(env: &mut Environment, n: GLsizei, framebuffers: MutPtr<GLuint>) {
    glGenFramebuffersOES(env, n, framebuffers)
}
fn glGenRenderbuffers(env: &mut Environment, n: GLsizei, renderbuffers: MutPtr<GLuint>) {
    glGenRenderbuffersOES(env, n, renderbuffers)
}
fn glIsFramebuffer(env: &mut Environment, framebuffer: GLuint) -> GLboolean {
    glIsFramebufferOES(env, framebuffer)
}
fn glIsRenderbuffer(env: &mut Environment, renderbuffer: GLuint) -> GLboolean {
    glIsRenderbufferOES(env, renderbuffer)
}
fn glBindFramebuffer(env: &mut Environment, target: GLenum, framebuffer: GLuint) {
    glBindFramebufferOES(env, target, framebuffer)
}
fn glBindRenderbuffer(env: &mut Environment, target: GLenum, renderbuffer: GLuint) {
    glBindRenderbufferOES(env, target, renderbuffer)
}
fn glRenderbufferStorage(
    env: &mut Environment,
    target: GLenum,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
) {
    glRenderbufferStorageOES(env, target, internalformat, width, height)
}
fn glFramebufferRenderbuffer(
    env: &mut Environment,
    target: GLenum,
    attachment: GLenum,
    renderbuffertarget: GLenum,
    renderbuffer: GLuint,
) {
    glFramebufferRenderbufferOES(env, target, attachment, renderbuffertarget, renderbuffer)
}
fn glFramebufferTexture2D(
    env: &mut Environment,
    target: GLenum,
    attachment: GLenum,
    textarget: GLenum,
    texture: GLuint,
    level: i32,
) {
    glFramebufferTexture2DOES(env, target, attachment, textarget, texture, level)
}
fn glGetFramebufferAttachmentParameteriv(
    env: &mut Environment,
    target: GLenum,
    attachment: GLenum,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    glGetFramebufferAttachmentParameterivOES(env, target, attachment, pname, params)
}
fn glGetRenderbufferParameteriv(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    glGetRenderbufferParameterivOES(env, target, pname, params)
}
fn glCheckFramebufferStatus(env: &mut Environment, target: GLenum) -> GLenum {
    glCheckFramebufferStatusOES(env, target)
}
fn glDeleteFramebuffers(env: &mut Environment, n: GLsizei, framebuffers: ConstPtr<GLuint>) {
    glDeleteFramebuffersOES(env, n, framebuffers)
}
fn glDeleteRenderbuffers(env: &mut Environment, n: GLsizei, renderbuffers: ConstPtr<GLuint>) {
    glDeleteRenderbuffersOES(env, n, renderbuffers)
}
fn glGenerateMipmap(env: &mut Environment, target: GLenum) {
    glGenerateMipmapOES(env, target)
}

fn _get_currently_bound_buffer_object_name(env: &mut Environment, target: GLenum) -> GLuint {
    let binding = match target {
        ARRAY_BUFFER => gles11::ARRAY_BUFFER_BINDING,
        ELEMENT_ARRAY_BUFFER => ELEMENT_ARRAY_BUFFER_BINDING,
        other => {
            // Anything else is a malformed call from the guest. Real GL
            // would set GL_INVALID_ENUM; we have nothing sensible to bind
            // against so just report "no object bound" and continue.
            log!(
                "Warning: _get_currently_bound_buffer_object_name(): unsupported buffer target {:#x}; returning 0.",
                other
            );
            return 0;
        }
    };
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        let mut name: GLint = 0;
        gles.GetIntegerv(binding, &mut name);
        name as GLuint
    })
}

fn _get_buffer_size(env: &mut Environment, target: GLenum) -> GLint {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        let mut size: GLint = 0;
        gles.GetBufferParameteriv(target, gles11::BUFFER_SIZE, &mut size);
        size
    })
}

fn glGetBufferParameteriv(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    let params = env.mem.ptr_at_mut(params, 1);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetBufferParameteriv(target, pname, params)
    })
}
fn glMapBufferOES(env: &mut Environment, target: GLenum, access: GLenum) -> MutPtr<GLvoid> {
    assert!(matches!(target, ARRAY_BUFFER | ELEMENT_ARRAY_BUFFER));
    assert!(access == WRITE_ONLY_OES);
    let buffer_object_name = _get_currently_bound_buffer_object_name(env, target);
    let host_buffer = with_ctx_and_mem_no_skip(env, |gles, _mem| unsafe {
        gles.MapBufferOES(target, access)
    });
    if host_buffer.is_null() {
        nil.cast()
    } else {
        let buffer_size = _get_buffer_size(env, target) as u32;
        let guest_buffer: MutVoidPtr = env.mem.alloc(buffer_size).cast();
        unsafe {
            env.mem
                .bytes_at_mut(guest_buffer.cast(), buffer_size)
                .copy_from_slice(from_raw_parts(host_buffer as *mut u8, buffer_size as usize));
        }
        let Some(current_ctx) = *env
            .framework_state
            .opengles
            .current_ctx_for_thread(env.current_thread)
        else {
            // glMapBufferOES requires a current context (Apple: "An
            // EAGLContext must be current on the thread before any GL
            // calls"). Without one, free the temporary guest buffer we
            // just allocated and return NULL, matching the GL spec which
            // says glMapBufferOES returns NULL on failure.
            env.mem.free(guest_buffer);
            return nil.cast();
        };
        let current_ctx_host_object = env.objc.borrow_mut::<EAGLContextHostObject>(current_ctx);
        // Per the GL_OES_mapbuffer spec, calling glMapBufferOES while a
        // buffer is already mapped is a GL_INVALID_OPERATION and returns
        // NULL. Some apps (e.g. ZAS) accidentally re-map without unmapping
        // first; the previous host mapping is still owned by the GL
        // driver, so we cannot just drop it without unmapping. To stay
        // close to Apple's lenient behaviour, we unmap the stale mapping
        // (freeing the previous guest mirror) and install the new one so
        // the app can continue uploading data instead of crashing.
        if let Some((stale_guest_buffer, _stale_host_buffer)) = current_ctx_host_object
            .mapped_buffers
            .remove(&buffer_object_name)
        {
            log!(
                "Warning: glMapBufferOES called on buffer {} that was already mapped; \
                 discarding the previous mapping (the previous pointer is no longer valid).",
                buffer_object_name
            );
            env.mem.free(stale_guest_buffer);
            // Issue a real glUnmapBufferOES so the driver releases its
            // own mapping of the buffer that we just dropped.
            with_ctx_and_mem(env, |gles, _mem| unsafe {
                gles.UnmapBufferOES(target);
            });
            // Re-borrow because with_ctx_and_mem dropped our reference.
            let current_ctx_host_object = env.objc.borrow_mut::<EAGLContextHostObject>(current_ctx);
            current_ctx_host_object
                .mapped_buffers
                .insert(buffer_object_name, (guest_buffer, host_buffer));
        } else {
            current_ctx_host_object
                .mapped_buffers
                .insert(buffer_object_name, (guest_buffer, host_buffer));
        }
        guest_buffer
    }
}
fn glUnmapBufferOES(env: &mut Environment, target: GLenum) -> GLboolean {
    let buffer_object_name = _get_currently_bound_buffer_object_name(env, target);
    let current_ctx = env
        .framework_state
        .opengles
        .current_ctx_for_thread(env.current_thread);
    let current_ctx_host_object = env
        .objc
        .borrow_mut::<EAGLContextHostObject>(current_ctx.unwrap());
    if let Some((guest_buffer, host_buffer)) = current_ctx_host_object
        .mapped_buffers
        .remove(&buffer_object_name)
    {
        let buffer_size = _get_buffer_size(env, target) as u32;
        unsafe {
            host_buffer.copy_from(
                env.mem.bytes_at(guest_buffer.cast(), buffer_size).as_ptr() as *mut GLvoid,
                buffer_size as usize,
            );
        }
        env.mem.free(guest_buffer);
    }
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.UnmapBufferOES(target) })
}

// =====================================================================
// OpenGL ES 2.0 entry points.
//
// These wrap the [crate::gles::GLES] trait's ES 2.0 methods, which are
// implemented by [crate::gles::gles1_on_gl2::GLES1OnGL2] using OpenGL 2.1
// compatibility profile. EAGL routes ES 2.0 contexts to that backend, so by
// the time these are called the trait methods are real implementations.
//
// Strings (shader sources, attribute/uniform names) need to be copied out of
// the guest's address space before being passed to host GL.
// =====================================================================

fn read_guest_cstring(mem: &Mem, ptr: ConstPtr<GLubyte>) -> std::ffi::CString {
    if ptr.is_null() {
        return std::ffi::CString::default();
    }
    let bytes = mem.cstr_at(ptr);
    std::ffi::CString::new(bytes).unwrap_or_default()
}

fn glCreateProgram(env: &mut Environment) -> GLuint {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.CreateProgram() })
}
fn glCreateShader(env: &mut Environment, type_: GLenum) -> GLuint {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.CreateShader(type_) })
}
fn glBindAttribLocation(
    env: &mut Environment,
    program: GLuint,
    index: GLuint,
    name: ConstPtr<GLubyte>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let cstr = read_guest_cstring(mem, name);
        gles.BindAttribLocation(program, index, cstr.as_ptr());
    });
}
fn glGetAttribLocation(env: &mut Environment, program: GLuint, name: ConstPtr<GLubyte>) -> GLint {
    with_ctx_and_mem_no_skip(env, |gles, mem| unsafe {
        let cstr = read_guest_cstring(mem, name);
        gles.GetAttribLocation(program, cstr.as_ptr())
    })
}
fn glGetUniformLocation(env: &mut Environment, program: GLuint, name: ConstPtr<GLubyte>) -> GLint {
    with_ctx_and_mem_no_skip(env, |gles, mem| unsafe {
        let cstr = read_guest_cstring(mem, name);
        gles.GetUniformLocation(program, cstr.as_ptr())
    })
}
fn glUniformMatrix2fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let n = (count as usize) * 4;
        let ptr = mem.ptr_at(value, n.try_into().unwrap_or(0));
        gles.UniformMatrix2fv(location, count, transpose, ptr);
    });
}
fn glUniformMatrix3fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let n = (count as usize) * 9;
        let ptr = mem.ptr_at(value, n.try_into().unwrap_or(0));
        gles.UniformMatrix3fv(location, count, transpose, ptr);
    });
}
fn glUniformMatrix4fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let n = (count as usize) * 16;
        let ptr = mem.ptr_at(value, n.try_into().unwrap_or(0));
        gles.UniformMatrix4fv(location, count, transpose, ptr);
    });
}
fn glUseProgram(env: &mut Environment, program: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.UseProgram(program) });
}
fn glDeleteProgram(env: &mut Environment, program: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.DeleteProgram(program) });
}
fn glDeleteShader(env: &mut Environment, shader: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.DeleteShader(shader) });
}
fn glCompileShader(env: &mut Environment, shader: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.CompileShader(shader);
        let mut ok: GLint = 0;
        gles.GetShaderiv(shader, 0x8B81 /* GL_COMPILE_STATUS */, &mut ok);
        if ok == 0 {
            let mut buf = [0u8; 1024];
            let mut len: GLsizei = 0;
            gles.GetShaderInfoLog(shader, 1024, &mut len, buf.as_mut_ptr() as *mut _);
            let s = std::str::from_utf8(std::slice::from_raw_parts(
                buf.as_ptr() as *const u8,
                len as usize,
            ))
            .unwrap_or("?");
            log!("Shader {} compile failed: {}", shader, s);
        }
    });
}
/// OpenGL ES 2.0 `glGetShaderPrecisionFormat`. Defined here so that guest
/// apps that probe shader compiler precision (e.g. Minecraft PE 0.10.x) get
/// real numbers from the driver instead of a return-0 stub installed by dyld
/// for an unimplemented symbol.
/// <https://registry.khronos.org/OpenGL-Refpages/es2.0/xhtml/glGetShaderPrecisionFormat.xml>
fn glGetShaderPrecisionFormat(
    env: &mut Environment,
    shadertype: GLenum,
    precisiontype: GLenum,
    range: MutPtr<GLint>,
    precision: MutPtr<GLint>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let mut host_range: [GLint; 2] = [0, 0];
        let mut host_precision: GLint = 0;
        gles.GetShaderPrecisionFormat(
            shadertype,
            precisiontype,
            host_range.as_mut_ptr(),
            &mut host_precision,
        );
        if !range.is_null() {
            mem.write(range + 0, host_range[0]);
            mem.write(range + 1, host_range[1]);
        }
        if !precision.is_null() {
            mem.write(precision, host_precision);
        }
    });
}
fn glAttachShader(env: &mut Environment, program: GLuint, shader: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.AttachShader(program, shader)
    });
}
fn glDetachShader(env: &mut Environment, program: GLuint, shader: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.DetachShader(program, shader)
    });
}
fn glLinkProgram(env: &mut Environment, program: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.LinkProgram(program);
        let mut ok: GLint = 0;
        gles.GetProgramiv(program, 0x8B82 /* GL_LINK_STATUS */, &mut ok);
        if ok == 0 {
            let mut buf = [0u8; 1024];
            let mut len: GLsizei = 0;
            gles.GetProgramInfoLog(program, 1024, &mut len, buf.as_mut_ptr() as *mut _);
            let s = std::str::from_utf8(std::slice::from_raw_parts(
                buf.as_ptr() as *const u8,
                len as usize,
            ))
            .unwrap_or("?");
            log!("Program {} link failed: {}", program, s);
        }
    });
}
fn glValidateProgram(env: &mut Environment, program: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.ValidateProgram(program) });
}
fn glIsShader(env: &mut Environment, shader: GLuint) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.IsShader(shader) })
}
fn glIsProgram(env: &mut Environment, program: GLuint) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.IsProgram(program) })
}
fn glGetShaderiv(env: &mut Environment, shader: GLuint, pname: GLenum, params: MutPtr<GLint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let mut val: GLint = 0;
        gles.GetShaderiv(shader, pname, &mut val);
        if !params.is_null() {
            mem.write(params, val);
        }
    });
}
/// OpenGL ES 2.0 `glGetShaderSource`. Returns the source string previously
/// uploaded via `glShaderSource` (which may have been translated to desktop
/// GLSL on the host side, but apps that round-trip via this entry point are
/// rare — the gles2_native backend returns the exact text the driver stored).
/// <https://registry.khronos.org/OpenGL-Refpages/es2.0/xhtml/glGetShaderSource.xml>
fn glGetShaderSource(
    env: &mut Environment,
    shader: GLuint,
    bufSize: GLsizei,
    length: MutPtr<GLsizei>,
    source: MutPtr<GLubyte>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        if bufSize <= 0 {
            if !length.is_null() {
                mem.write(length, 0);
            }
            return;
        }
        let mut buf: Vec<u8> = vec![0u8; bufSize as usize];
        let mut written: GLsizei = 0;
        gles.GetShaderSource(shader, bufSize, &mut written, buf.as_mut_ptr().cast());
        if !length.is_null() {
            mem.write(length, written);
        }
        if !source.is_null() && written >= 0 {
            // The driver writes a NUL terminator at `written` and may write up
            // to `bufSize` bytes including the terminator. Copy what was
            // produced (plus the terminator when there's room) into guest
            // memory.
            let count = written as usize + 1; // include trailing NUL
            let count = count.min(buf.len()).min(bufSize as usize);
            let dst = mem.ptr_at_mut(source, count.try_into().unwrap_or(0));
            std::ptr::copy_nonoverlapping(buf.as_ptr(), dst, count);
        }
    });
}
fn glGetShaderInfoLog(
    env: &mut Environment,
    shader: GLuint,
    bufSize: GLsizei,
    length: MutPtr<GLsizei>,
    infoLog: MutPtr<GLubyte>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        if bufSize <= 0 {
            if !length.is_null() {
                mem.write(length, 0);
            }
            return;
        }
        let mut buf: Vec<u8> = vec![0u8; bufSize as usize];
        let mut written: GLsizei = 0;
        gles.GetShaderInfoLog(shader, bufSize, &mut written, buf.as_mut_ptr().cast());
        if !length.is_null() {
            mem.write(length, written);
        }
        if !infoLog.is_null() && written >= 0 {
            let count = written as usize + 1; // include trailing NUL
            let count = count.min(buf.len()).min(bufSize as usize);
            let dst = mem.ptr_at_mut(infoLog, count.try_into().unwrap_or(0));
            std::ptr::copy_nonoverlapping(buf.as_ptr(), dst, count);
        }
    });
}
fn glGetProgramiv(env: &mut Environment, program: GLuint, pname: GLenum, params: MutPtr<GLint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let mut val: GLint = 0;
        gles.GetProgramiv(program, pname, &mut val);
        if !params.is_null() {
            mem.write(params, val);
        }
    });
}
fn glGetProgramInfoLog(
    env: &mut Environment,
    program: GLuint,
    bufSize: GLsizei,
    length: MutPtr<GLsizei>,
    infoLog: MutPtr<GLubyte>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        if bufSize <= 0 {
            if !length.is_null() {
                mem.write(length, 0);
            }
            return;
        }
        let mut buf: Vec<u8> = vec![0u8; bufSize as usize];
        let mut written: GLsizei = 0;
        gles.GetProgramInfoLog(program, bufSize, &mut written, buf.as_mut_ptr().cast());
        if !length.is_null() {
            mem.write(length, written);
        }
        if !infoLog.is_null() && written >= 0 {
            let count = written as usize + 1;
            let count = count.min(buf.len()).min(bufSize as usize);
            let dst = mem.ptr_at_mut(infoLog, count.try_into().unwrap_or(0));
            std::ptr::copy_nonoverlapping(buf.as_ptr(), dst, count);
        }
    });
}
fn glGetActiveUniform(
    env: &mut Environment,
    program: GLuint,
    index: GLuint,
    bufSize: GLsizei,
    length: MutPtr<GLsizei>,
    size: MutPtr<GLint>,
    type_: MutPtr<GLenum>,
    name: MutPtr<GLubyte>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        if bufSize <= 0 {
            if !length.is_null() {
                mem.write(length, 0);
            }
            return;
        }
        let mut host_length: GLsizei = 0;
        let mut host_size: GLint = 0;
        let mut host_type: GLenum = 0;
        let mut buf: Vec<u8> = vec![0u8; bufSize as usize];
        gles.GetActiveUniform(
            program,
            index,
            bufSize,
            &mut host_length,
            &mut host_size,
            &mut host_type,
            buf.as_mut_ptr().cast(),
        );
        if !length.is_null() {
            mem.write(length, host_length);
        }
        if !size.is_null() {
            mem.write(size, host_size);
        }
        if !type_.is_null() {
            mem.write(type_, host_type);
        }
        if !name.is_null() && host_length >= 0 {
            let count = host_length as usize + 1;
            let count = count.min(buf.len()).min(bufSize as usize);
            let dst = mem.ptr_at_mut(name, count.try_into().unwrap_or(0));
            std::ptr::copy_nonoverlapping(buf.as_ptr(), dst, count);
        }
    });
}
fn glGetActiveAttrib(
    env: &mut Environment,
    program: GLuint,
    index: GLuint,
    bufSize: GLsizei,
    length: MutPtr<GLsizei>,
    size: MutPtr<GLint>,
    type_: MutPtr<GLenum>,
    name: MutPtr<GLubyte>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        if bufSize <= 0 {
            if !length.is_null() {
                mem.write(length, 0);
            }
            return;
        }
        let mut host_length: GLsizei = 0;
        let mut host_size: GLint = 0;
        let mut host_type: GLenum = 0;
        let mut buf: Vec<u8> = vec![0u8; bufSize as usize];
        gles.GetActiveAttrib(
            program,
            index,
            bufSize,
            &mut host_length,
            &mut host_size,
            &mut host_type,
            buf.as_mut_ptr().cast(),
        );
        if !length.is_null() {
            mem.write(length, host_length);
        }
        if !size.is_null() {
            mem.write(size, host_size);
        }
        if !type_.is_null() {
            mem.write(type_, host_type);
        }
        if !name.is_null() && host_length >= 0 {
            let count = host_length as usize + 1;
            let count = count.min(buf.len()).min(bufSize as usize);
            let dst = mem.ptr_at_mut(name, count.try_into().unwrap_or(0));
            std::ptr::copy_nonoverlapping(buf.as_ptr(), dst, count);
        }
    });
}

fn strip_captain_tomato_shader_precision(source: &str) -> String {
    fn replace_precision_tokens(line: &str) -> String {
        let mut out = String::with_capacity(line.len());
        let mut token = String::new();

        let flush = |token: &mut String, out: &mut String| {
            if token == "lowp" || token == "mediump" || token == "highp" {
                out.push_str("highp");
            } else {
                out.push_str(token);
            }
            token.clear();
        };

        for ch in line.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                token.push(ch);
            } else {
                if !token.is_empty() {
                    flush(&mut token, &mut out);
                }
                out.push(ch);
            }
        }

        if !token.is_empty() {
            flush(&mut token, &mut out);
        }

        out
    }

    let mut lines: Vec<String> = Vec::new();
    let mut has_float_precision = false;
    let mut has_int_precision = false;

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("precision ") && trimmed.ends_with(" float;") {
            has_float_precision = true;
            lines.push("precision highp float;".to_string());
            continue;
        }

        if trimmed.starts_with("precision ") && trimmed.ends_with(" int;") {
            has_int_precision = true;
            lines.push("precision highp int;".to_string());
            continue;
        }

        lines.push(replace_precision_tokens(line));
    }

    let mut insert_at = 0usize;
    while insert_at < lines.len()
        && (lines[insert_at].trim_start().starts_with("#version")
            || lines[insert_at].trim_start().starts_with("#extension"))
    {
        insert_at += 1;
    }

    if !has_int_precision {
        lines.insert(insert_at, "precision highp int;".to_string());
    }
    if !has_float_precision {
        lines.insert(insert_at, "precision highp float;".to_string());
    }

    lines.join("\n")
}

fn normalize_shader_preprocessor_whitespace(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + 32);
    for (i, line) in source.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let line = line.trim_end_matches('\r');
        let trimmed_start = line.trim_start();
        let is_directive = trimmed_start.starts_with('#');
        if is_directive {
            let comment_at = match (line.find("//"), line.find("/*")) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            if let Some(comment_at) = comment_at {
                let before = &line[..comment_at];
                let comment = &line[comment_at..];
                if !before.ends_with(' ') && !before.ends_with('\t') && !before.is_empty() {
                    out.push_str(before.trim_end());
                    out.push(' ');
                    out.push_str(comment);
                    continue;
                }
            }
        }
        out.push_str(line);
    }
    out
}

fn glShaderSource(
    env: &mut Environment,
    shader: GLuint,
    count: GLsizei,
    string: ConstPtr<ConstPtr<GLubyte>>,
    length: ConstPtr<GLint>,
) {
    if count <= 0 {
        return;
    }
    // Copy each source string out of the guest's memory into a host-side
    // buffer so we can pass real host pointers to the GLES implementation.
    // Concatenate first so preprocessor normalization can see directive
    // boundaries that guest code split across multiple source strings.
    let mut raw_source = Vec::<u8>::new();
    for i in 0..count {
        let str_ptr_ptr: ConstPtr<ConstPtr<GLubyte>> = string + (i as GuestUSize);
        let str_ptr: ConstPtr<GLubyte> = env.mem.read(str_ptr_ptr);
        if str_ptr.is_null() {
            continue;
        }
        // Check if explicit lengths were provided.
        let len_opt: Option<i32> = if length.is_null() {
            None
        } else {
            let len_ptr: ConstPtr<GLint> = length + (i as GuestUSize);
            let l: GLint = env.mem.read(len_ptr);
            if l < 0 {
                None
            } else {
                Some(l)
            }
        };
        let bytes_vec: Vec<u8> = if let Some(len) = len_opt {
            let slice = env
                .mem
                .bytes_at(str_ptr.cast(), len.try_into().unwrap_or(0));
            slice.to_vec()
        } else {
            // GLSL shader sources can legitimately exceed the default 64KB
            // `cstr_at` safety cap (e.g. Unreal Engine's generated shaders in
            // UDKGame). Using the default cap silently truncates the source,
            // which then fails to compile with errors like
            // "Unterminated #if/#ifdef/#ifndef". Allow up to 16 MB here.
            const MAX_SHADER_SRC_LEN: u32 = 16 * 1024 * 1024;
            env.mem
                .cstr_at_with_max_len(str_ptr, MAX_SHADER_SRC_LEN)
                .to_vec()
        };
        raw_source.extend_from_slice(&bytes_vec);
    }
    // Normalize shader text before sending it to the driver:
    // 1. Fix up preprocessor-directive whitespace (see doc comment on
    //    `normalize_shader_preprocessor_whitespace`) — real-world guest
    //    shaders (e.g. Gameloft's "9mm") glue same-line comments
    //    directly onto `#endif`/`#if`/`#elif` or use CRLF endings,
    //    which real PowerVR SGX drivers tolerated but modern GLSL
    //    compilers reject with "unexpected tokens following #endif".
    // 2. Normalize GLES precision qualifiers for all Cocos2D shaders.
    //    Fixes Mesa link failures like:
    //    uniform `CC_PMatrix` declared as type `f16mat4` and type `mat4`.
    let src = String::from_utf8_lossy(&raw_source);
    let src = normalize_shader_preprocessor_whitespace(&src);
    let bytes_vec = strip_captain_tomato_shader_precision(&src).into_bytes();

    let cs = std::ffi::CString::new(bytes_vec).unwrap_or_default();
    let ptr = cs.as_ptr();
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.ShaderSource(shader, 1, &ptr, std::ptr::null());
    });
}
fn glEnableVertexAttribArray(env: &mut Environment, index: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.EnableVertexAttribArray(index)
    });
}
fn glDisableVertexAttribArray(env: &mut Environment, index: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.DisableVertexAttribArray(index)
    });
}
fn glVertexAttribPointer(
    env: &mut Environment,
    index: GLuint,
    size: GLint,
    type_: GLenum,
    normalized: GLboolean,
    stride: GLsizei,
    pointer: ConstVoidPtr,
) {
    // If a buffer is bound, `pointer` is treated as an offset within that
    // buffer (not as a pointer into client memory) and we can pass it through
    // as-is. Otherwise we need to translate the guest pointer to a host
    // pointer.
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let mut bound: GLint = 0;
        gles.GetIntegerv(gles11::ARRAY_BUFFER_BINDING, &mut bound);
        let host_ptr = if bound == 0 {
            if pointer.is_null() {
                std::ptr::null()
            } else {
                mem.ptr_at(pointer.cast::<u8>(), 1) as *const _
            }
        } else {
            pointer.to_bits() as usize as *const _
        };
        gles.VertexAttribPointer(index, size, type_, normalized, stride, host_ptr);
    });
}
fn glVertexAttrib1f(env: &mut Environment, index: GLuint, x: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.VertexAttrib1f(index, x) });
}
fn glVertexAttrib1fv(env: &mut Environment, index: GLuint, values: ConstPtr<GLfloat>) {
    let value = env.mem.read(values);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.VertexAttrib1fv(index, &value)
    });
}
fn glVertexAttrib2f(env: &mut Environment, index: GLuint, x: GLfloat, y: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.VertexAttrib2f(index, x, y)
    });
}
fn glVertexAttrib3f(env: &mut Environment, index: GLuint, x: GLfloat, y: GLfloat, z: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.VertexAttrib3f(index, x, y, z)
    });
}
fn glVertexAttrib4f(
    env: &mut Environment,
    index: GLuint,
    x: GLfloat,
    y: GLfloat,
    z: GLfloat,
    w: GLfloat,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.VertexAttrib4f(index, x, y, z, w)
    });
}
fn glUniform1i(env: &mut Environment, location: GLint, v0: GLint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Uniform1i(location, v0) });
}
fn glUniform2i(env: &mut Environment, location: GLint, v0: GLint, v1: GLint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Uniform2i(location, v0, v1)
    });
}
fn glUniform3i(env: &mut Environment, location: GLint, v0: GLint, v1: GLint, v2: GLint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Uniform3i(location, v0, v1, v2)
    });
}
fn glUniform4i(env: &mut Environment, location: GLint, v0: GLint, v1: GLint, v2: GLint, v3: GLint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Uniform4i(location, v0, v1, v2, v3)
    });
}
fn glUniform1f(env: &mut Environment, location: GLint, v0: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Uniform1f(location, v0) });
}
fn glUniform2f(env: &mut Environment, location: GLint, v0: GLfloat, v1: GLfloat) {
    let (v0,v1)=crate::ds_shader_opening_fog::parameters(env,location,v0,v1);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Uniform2f(location, v0, v1)
    });
}
fn glUniform3f(env: &mut Environment, location: GLint, v0: GLfloat, v1: GLfloat, v2: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Uniform3f(location, v0, v1, v2)
    });
}
fn glUniform4f(
    env: &mut Environment,
    location: GLint,
    v0: GLfloat,
    v1: GLfloat,
    v2: GLfloat,
    v3: GLfloat,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Uniform4f(location, v0, v1, v2, v3)
    });
}
fn glUniform1iv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let n = count as usize;
        let ptr = mem.ptr_at(value, n.try_into().unwrap_or(0));
        gles.Uniform1iv(location, count, ptr);
    });
}
fn glUniform2iv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let n = (count as usize) * 2;
        let ptr = mem.ptr_at(value, n.try_into().unwrap_or(0));
        gles.Uniform2iv(location, count, ptr);
    });
}
fn glUniform3iv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let n = (count as usize) * 3;
        let ptr = mem.ptr_at(value, n.try_into().unwrap_or(0));
        gles.Uniform3iv(location, count, ptr);
    });
}
fn glUniform4iv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let n = (count as usize) * 4;
        let ptr = mem.ptr_at(value, n.try_into().unwrap_or(0));
        gles.Uniform4iv(location, count, ptr);
    });
}
fn glUniform1fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let n = count as usize;
        let ptr = mem.ptr_at(value, n.try_into().unwrap_or(0));
        gles.Uniform1fv(location, count, ptr);
    });
}
fn glUniform2fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let n = (count as usize) * 2;
        let ptr = mem.ptr_at(value, n.try_into().unwrap_or(0));
        gles.Uniform2fv(location, count, ptr);
    });
}
fn glUniform3fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let n = (count as usize) * 3;
        let ptr = mem.ptr_at(value, n.try_into().unwrap_or(0));
        gles.Uniform3fv(location, count, ptr);
    });
}
fn glUniform4fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let n = (count as usize) * 4;
        let ptr = mem.ptr_at(value, n.try_into().unwrap_or(0));
        gles.Uniform4fv(location, count, ptr);
    });
}
/// `void glShaderBinary(GLsizei count, const GLuint *shaders,
///                      GLenum binaryformat, const void *binary,
///                      GLsizei length)` — OpenGL ES 2.0+.
/// Loads a pre-compiled shader binary. We forward count, format, and
/// raw bytes to the host so backends that support
/// `GL_EXT_shader_binary` can load them; backends that don't accept
/// any binary format will simply set GL_INVALID_ENUM, which is the
/// spec-mandated behaviour.
fn glShaderBinary(
    env: &mut Environment,
    count: GLsizei,
    shaders: ConstPtr<GLuint>,
    binary_format: GLenum,
    binary: ConstVoidPtr,
    length: GLsizei,
) {
    if count <= 0 || shaders.is_null() {
        return;
    }
    let n = count as usize;
    let mut shaders_vec: Vec<GLuint> = Vec::with_capacity(n);
    for i in 0..n {
        shaders_vec.push(env.mem.read(shaders + (i as GuestUSize)));
    }
    let bin_len = length.max(0) as GuestUSize;
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let bin_host = if binary.is_null() || bin_len == 0 {
            std::ptr::null()
        } else {
            mem.bytes_at(binary.cast(), bin_len).as_ptr().cast()
        };
        gles.ShaderBinary(count, shaders_vec.as_ptr(), binary_format, bin_host, length);
    });
}

/// `void glGetActiveUniformsiv(GLuint program, GLsizei uniformCount,
///                             const GLuint *uniformIndices, GLenum pname,
///                             GLint *params)` — OpenGL ES 3.0 §2.12.6.
fn glGetActiveUniformsiv(
    env: &mut Environment,
    program: GLuint,
    uniform_count: GLsizei,
    uniform_indices: ConstPtr<GLuint>,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    if uniform_count <= 0 {
        return;
    }
    let n = uniform_count as usize;
    let mut indices: Vec<GLuint> = Vec::with_capacity(n);
    for i in 0..n {
        indices.push(env.mem.read(uniform_indices + (i as GuestUSize)));
    }
    let mut results: Vec<GLint> = vec![0; n];
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetActiveUniformsiv(
            program,
            uniform_count,
            indices.as_ptr(),
            pname,
            results.as_mut_ptr(),
        );
    });
    for (i, v) in results.iter().enumerate() {
        env.mem.write(params + (i as GuestUSize), *v);
    }
}

/// `void glGetActiveUniformBlockiv(GLuint program,
///                                 GLuint uniformBlockIndex, GLenum pname,
///                                 GLint *params)` — OpenGL ES 3.0 §2.12.6.
fn glGetActiveUniformBlockiv(
    env: &mut Environment,
    program: GLuint,
    uniform_block_index: GLuint,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    // The number of values written depends on `pname`; the largest is
    // GL_UNIFORM_BLOCK_ACTIVE_UNIFORM_INDICES which returns up to
    // `GL_MAX_UNIFORM_BLOCK_BINDINGS` values. We allocate a generous
    // 64-element scratch buffer (more than enough for any GLES 3.0
    // hardware) and only write back what the backend reports.
    let mut scratch: [GLint; 64] = [0; 64];
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetActiveUniformBlockiv(program, uniform_block_index, pname, scratch.as_mut_ptr());
    });
    // Conservative: write back one int — sufficient for the scalar
    // queries (BINDING, DATA_SIZE, NAME_LENGTH, *_ACTIVE_UNIFORMS,
    // REFERENCED_BY_*_SHADER). For the array query the guest passes a
    // buffer it sized using GL_UNIFORM_BLOCK_ACTIVE_UNIFORMS, which it
    // queried right before; the backend writes into the host scratch
    // and we copy the count of ints the guest had asked for. Without
    // a way to know the buffer size we copy 1 int (covers all scalar
    // pnames) — the GL_UNIFORM_BLOCK_ACTIVE_UNIFORM_INDICES path
    // is exercised by very few apps and falls back to GL_INVALID_VALUE
    // on backends that don't implement uniform blocks.
    if !params.is_null() {
        env.mem.write(params, scratch[0]);
    }
}

/// `void glGetActiveUniformBlockName(GLuint program,
///                                   GLuint uniformBlockIndex,
///                                   GLsizei bufSize, GLsizei *length,
///                                   GLchar *uniformBlockName)`.
fn glGetActiveUniformBlockName(
    env: &mut Environment,
    program: GLuint,
    uniform_block_index: GLuint,
    buf_size: GLsizei,
    length: MutPtr<GLsizei>,
    uniform_block_name: MutPtr<GLubyte>,
) {
    if buf_size <= 0 || uniform_block_name.is_null() {
        if !length.is_null() {
            env.mem.write(length, 0);
        }
        return;
    }
    let cap = buf_size as usize;
    let mut name_buf: Vec<u8> = vec![0u8; cap];
    let mut host_length: GLsizei = 0;
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetActiveUniformBlockName(
            program,
            uniform_block_index,
            buf_size,
            &mut host_length,
            name_buf.as_mut_ptr() as *mut std::os::raw::c_char,
        );
    });
    let n = (host_length.max(0) as usize).min(cap);
    let dst = env
        .mem
        .bytes_at_mut(uniform_block_name.cast(), n as GuestUSize);
    dst.copy_from_slice(&name_buf[..n]);
    if !length.is_null() {
        env.mem.write(length, host_length);
    }
}

/// `void glProgramBinary(GLuint program, GLenum binaryFormat,
///                       const void *binary, GLsizei length)`.
fn glProgramBinary(
    env: &mut Environment,
    program: GLuint,
    binary_format: GLenum,
    binary: ConstVoidPtr,
    length: GLsizei,
) {
    let len = length.max(0) as GuestUSize;
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let host_ptr: *const GLvoid = if binary.is_null() || len == 0 {
            std::ptr::null()
        } else {
            mem.bytes_at(binary.cast(), len).as_ptr().cast()
        };
        gles.ProgramBinary(program, binary_format, host_ptr, length);
    });
}

/// `void glGetProgramBinary(GLuint program, GLsizei bufSize,
///                          GLsizei *length, GLenum *binaryFormat,
///                          void *binary)`.
fn glGetProgramBinary(
    env: &mut Environment,
    program: GLuint,
    buf_size: GLsizei,
    length: MutPtr<GLsizei>,
    binary_format: MutPtr<GLenum>,
    binary: MutVoidPtr,
) {
    if buf_size <= 0 || binary.is_null() {
        if !length.is_null() {
            env.mem.write(length, 0);
        }
        return;
    }
    let cap = buf_size as usize;
    let mut host_buf: Vec<u8> = vec![0u8; cap];
    let mut host_length: GLsizei = 0;
    let mut host_format: GLenum = 0;
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetProgramBinary(
            program,
            buf_size,
            &mut host_length,
            &mut host_format,
            host_buf.as_mut_ptr() as *mut GLvoid,
        );
    });
    let n = (host_length.max(0) as usize).min(cap);
    let dst = env.mem.bytes_at_mut(binary.cast(), n as GuestUSize);
    dst.copy_from_slice(&host_buf[..n]);
    if !length.is_null() {
        env.mem.write(length, host_length);
    }
    if !binary_format.is_null() {
        env.mem.write(binary_format, host_format);
    }
}

fn glReleaseShaderCompiler(env: &mut Environment) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.ReleaseShaderCompiler() });
}
fn glBlendColor(env: &mut Environment, r: GLclampf, g: GLclampf, b: GLclampf, a: GLclampf) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.BlendColor(r, g, b, a) });
}
fn glBlendEquation(env: &mut Environment, mode: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.BlendEquation(mode) });
}
fn glBlendEquationSeparate(env: &mut Environment, modeRGB: GLenum, modeAlpha: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.BlendEquationSeparate(modeRGB, modeAlpha)
    });
}
fn glBlendFuncSeparate(
    env: &mut Environment,
    srcRGB: GLenum,
    dstRGB: GLenum,
    srcAlpha: GLenum,
    dstAlpha: GLenum,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.BlendFuncSeparate(srcRGB, dstRGB, srcAlpha, dstAlpha)
    });
}
fn glStencilFuncSeparate(
    env: &mut Environment,
    face: GLenum,
    func: GLenum,
    ref_: GLint,
    mask: GLuint,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.StencilFuncSeparate(face, func, ref_, mask)
    });
}
fn glStencilOpSeparate(
    env: &mut Environment,
    face: GLenum,
    sfail: GLenum,
    dpfail: GLenum,
    dppass: GLenum,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.StencilOpSeparate(face, sfail, dpfail, dppass)
    });
}
fn glStencilMaskSeparate(env: &mut Environment, face: GLenum, mask: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.StencilMaskSeparate(face, mask)
    });
}

// Pre-existing GLES1 helpers reused for ES 2.0 — VAOs are not part of ES 2.0
// but some apps still call these as no-ops. On an ES 3.0 context (where
// VAOs are core) we route to the real driver instead.

/// VAO entry point. On ES 1.1 / ES 2.0 the GLES trait has no VAO support and
/// the OES stub path returns sequential fake IDs (which most apps tolerate
/// as no-op handles). On ES 3.0 we delegate to the real driver, which is
/// mandatory because Core profile requires VAOs and the per-bind state is
/// observable via uniform locations etc.
fn glGenVertexArrays(env: &mut Environment, n: GLsizei, arrays: MutPtr<GLuint>) {
    let is_es3 = with_ctx_and_mem(env, |gles, _mem| gles.is_es3());
    if is_es3 {
        with_ctx_and_mem(env, |gles, mem| unsafe {
            let slice = mem.bytes_at_mut(arrays.cast(), (n as GuestUSize) * 4);
            gles.GenVertexArrays(n, slice.as_mut_ptr().cast());
        });
    } else {
        glGenVertexArraysOES(env, n, arrays)
    }
}
fn glBindVertexArray(env: &mut Environment, array: GLuint) {
    let is_es3 = with_ctx_and_mem(env, |gles, _mem| gles.is_es3());
    if is_es3 {
        with_ctx_and_mem(env, |gles, _mem| unsafe {
            gles.BindVertexArray(array);
        });
    } else {
        glBindVertexArrayOES(env, array)
    }
}
fn glDeleteVertexArrays(env: &mut Environment, n: GLsizei, arrays: ConstPtr<GLuint>) {
    let is_es3 = with_ctx_and_mem(env, |gles, _mem| gles.is_es3());
    if is_es3 {
        with_ctx_and_mem(env, |gles, mem| unsafe {
            let slice = mem.bytes_at(arrays.cast(), (n as GuestUSize) * 4);
            gles.DeleteVertexArrays(n, slice.as_ptr().cast());
        });
    } else {
        glDeleteVertexArraysOES(env, n, arrays)
    }
}
fn glIsVertexArray(env: &mut Environment, array: GLuint) -> GLboolean {
    let is_es3 = with_ctx_and_mem(env, |gles, _mem| gles.is_es3());
    if is_es3 {
        with_ctx_and_mem(env, |gles, _mem| unsafe { gles.IsVertexArray(array) })
    } else {
        glIsVertexArrayOES(env, array)
    }
}

/// ES 3.0 has the unsuffixed `glUnmapBuffer`. On the ES 2.0 / ES 1.1 paths
/// `glUnmapBufferOES` is the matching entry point — share the existing
/// implementation since the buffer-mapping bookkeeping is identical.
fn glUnmapBuffer(env: &mut Environment, target: GLenum) -> GLboolean {
    glUnmapBufferOES(env, target)
}

// ===== OpenGL ES 3.0 guest dispatchers =====
//
// Every function below is a thin wrapper that grabs the current GL context
// and forwards the call to the corresponding `GLES` trait method. Pointers
// are translated from guest memory to host memory via the `Mem` helper so
// the host driver sees host addresses.

// -- Buffer object operations --
fn glMapBufferRange(
    env: &mut Environment,
    target: GLenum,
    offset: GuestGLintptr,
    length: GuestGLsizeiptr,
    access: GLbitfield,
) -> MutVoidPtr {
    let host_ptr: *mut GLvoid = with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.MapBufferRange(
            target,
            offset as HostGLintptr,
            length as HostGLsizeiptr,
            access,
        )
    });
    // The host pointer cannot be observed by the guest directly. EAGL
    // tracks pending mappings in `EAGLContextHostObject::mapped_buffers`;
    // the matching `glUnmapBuffer` consults that table. Allocate a guest
    // buffer of `length` bytes, copy the contents from the host pointer,
    // and remember the pairing.
    if host_ptr.is_null() || length == 0 {
        return Ptr::null();
    }
    let guest_buf: MutPtr<GLvoid> = env.mem.alloc(length as GuestUSize).cast();
    unsafe {
        let host_slice = from_raw_parts(host_ptr as *const u8, length as usize);
        let guest_slice = env.mem.bytes_at_mut(guest_buf.cast(), length as GuestUSize);
        guest_slice.copy_from_slice(host_slice);
    }
    let current_ctx: Option<crate::objc::id> = *env
        .framework_state
        .opengles
        .current_ctx_for_thread(env.current_thread);
    if let Some(ctx) = current_ctx {
        let host_obj = env.objc.borrow_mut::<EAGLContextHostObject>(ctx);
        host_obj
            .mapped_buffers
            .insert(target, (guest_buf, host_ptr));
    }
    guest_buf.cast()
}

fn glFlushMappedBufferRange(
    env: &mut Environment,
    target: GLenum,
    offset: GuestGLintptr,
    length: GuestGLsizeiptr,
) {
    // Copy the relevant slice of the guest-visible buffer back to the
    // host-mapped buffer so the driver sees the writes.
    let current_ctx: Option<crate::objc::id> = *env
        .framework_state
        .opengles
        .current_ctx_for_thread(env.current_thread);
    if let Some(ctx) = current_ctx {
        let mapping = env
            .objc
            .borrow::<EAGLContextHostObject>(ctx)
            .mapped_buffers
            .get(&target)
            .copied();
        if let Some((guest_buf, host_ptr)) = mapping {
            let guest_slice = env
                .mem
                .bytes_at(guest_buf.cast(), (offset + length) as GuestUSize);
            unsafe {
                let host_slice = std::slice::from_raw_parts_mut(
                    (host_ptr as *mut u8).add(offset as usize),
                    length as usize,
                );
                host_slice.copy_from_slice(&guest_slice[offset as usize..]);
            }
        }
    }
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.FlushMappedBufferRange(target, offset as HostGLintptr, length as HostGLsizeiptr)
    });
}

fn glCopyBufferSubData(
    env: &mut Environment,
    read_target: GLenum,
    write_target: GLenum,
    read_offset: GuestGLintptr,
    write_offset: GuestGLintptr,
    size: GuestGLsizeiptr,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.CopyBufferSubData(
            read_target,
            write_target,
            read_offset as HostGLintptr,
            write_offset as HostGLintptr,
            size as HostGLsizeiptr,
        )
    });
}

fn glBindBufferBase(env: &mut Environment, target: GLenum, index: GLuint, buffer: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.BindBufferBase(target, index, buffer)
    });
}

fn glBindBufferRange(
    env: &mut Environment,
    target: GLenum,
    index: GLuint,
    buffer: GLuint,
    offset: GuestGLintptr,
    size: GuestGLsizeiptr,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.BindBufferRange(
            target,
            index,
            buffer,
            offset as HostGLintptr,
            size as HostGLsizeiptr,
        )
    });
}

// -- Drawing --
fn glDrawRangeElements(
    env: &mut Environment,
    mode: GLenum,
    start: GLuint,
    end: GLuint,
    count: GLsizei,
    type_: GLenum,
    indices: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        // ELEMENT_ARRAY_BUFFER bound: indices is a byte offset (small int)
        // → pass through. Otherwise it's a guest pointer → translate.
        let mut buf: GLint = 0;
        gles.GetIntegerv(ELEMENT_ARRAY_BUFFER_BINDING, &mut buf);
        let host_ptr: *const GLvoid = if buf != 0 {
            indices.cast::<u8>().to_bits() as usize as *const GLvoid
        } else {
            let bytes_per_index = match type_ {
                gles11::UNSIGNED_BYTE => 1,
                gles11::UNSIGNED_SHORT => 2,
                _ => 4,
            };
            let total_bytes = (count as GuestUSize) * bytes_per_index;
            mem.bytes_at(indices.cast(), total_bytes).as_ptr().cast()
        };
        gles.DrawRangeElements(mode, start, end, count, type_, host_ptr)
    });
}

fn glDrawArraysInstanced(
    env: &mut Environment,
    mode: GLenum,
    first: GLint,
    count: GLsizei,
    instance_count: GLsizei,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.DrawArraysInstanced(mode, first, count, instance_count)
    });
}

fn glDrawElementsInstanced(
    env: &mut Environment,
    mode: GLenum,
    count: GLsizei,
    type_: GLenum,
    indices: ConstVoidPtr,
    instance_count: GLsizei,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let mut buf: GLint = 0;
        gles.GetIntegerv(ELEMENT_ARRAY_BUFFER_BINDING, &mut buf);
        let host_ptr: *const GLvoid = if buf != 0 {
            indices.cast::<u8>().to_bits() as usize as *const GLvoid
        } else {
            let bytes_per_index = match type_ {
                gles11::UNSIGNED_BYTE => 1,
                gles11::UNSIGNED_SHORT => 2,
                _ => 4,
            };
            let total_bytes = (count as GuestUSize) * bytes_per_index;
            mem.bytes_at(indices.cast(), total_bytes).as_ptr().cast()
        };
        gles.DrawElementsInstanced(mode, count, type_, host_ptr, instance_count)
    });
}

fn glVertexAttribDivisor(env: &mut Environment, index: GLuint, divisor: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.VertexAttribDivisor(index, divisor)
    });
}

// -- Multiple render targets / draw buffers --
fn glReadBuffer(env: &mut Environment, src: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.ReadBuffer(src) });
}

fn glDrawBuffers(env: &mut Environment, n: GLsizei, bufs: ConstPtr<GLenum>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(bufs.cast(), (n as GuestUSize) * 4);
        gles.DrawBuffers(n, slice.as_ptr().cast())
    });
}

// -- glClearBuffer* --
fn glClearBufferiv(
    env: &mut Environment,
    buffer: GLenum,
    drawbuffer: GLint,
    value: ConstPtr<GLint>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), 16);
        gles.ClearBufferiv(buffer, drawbuffer, slice.as_ptr().cast())
    });
}

fn glClearBufferuiv(
    env: &mut Environment,
    buffer: GLenum,
    drawbuffer: GLint,
    value: ConstPtr<GLuint>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), 16);
        gles.ClearBufferuiv(buffer, drawbuffer, slice.as_ptr().cast())
    });
}

fn glClearBufferfv(
    env: &mut Environment,
    buffer: GLenum,
    drawbuffer: GLint,
    value: ConstPtr<GLfloat>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), 16);
        gles.ClearBufferfv(buffer, drawbuffer, slice.as_ptr().cast())
    });
}

fn glClearBufferfi(
    env: &mut Environment,
    buffer: GLenum,
    drawbuffer: GLint,
    depth: GLfloat,
    stencil: GLint,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.ClearBufferfi(buffer, drawbuffer, depth, stencil)
    });
}

// -- Framebuffer blits / multisample / layered attachments --
#[allow(clippy::too_many_arguments)]
fn glBlitFramebuffer(
    env: &mut Environment,
    src_x0: GLint,
    src_y0: GLint,
    src_x1: GLint,
    src_y1: GLint,
    dst_x0: GLint,
    dst_y0: GLint,
    dst_x1: GLint,
    dst_y1: GLint,
    mask: GLbitfield,
    filter: GLenum,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.BlitFramebuffer(
            src_x0, src_y0, src_x1, src_y1, dst_x0, dst_y0, dst_x1, dst_y1, mask, filter,
        )
    });
}

fn glRenderbufferStorageMultisample(
    env: &mut Environment,
    target: GLenum,
    samples: GLsizei,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.RenderbufferStorageMultisample(target, samples, internalformat, width, height)
    });
}

fn glFramebufferTextureLayer(
    env: &mut Environment,
    target: GLenum,
    attachment: GLenum,
    texture: GLuint,
    level: GLint,
    layer: GLint,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.FramebufferTextureLayer(target, attachment, texture, level, layer)
    });
}

fn glInvalidateFramebuffer(
    env: &mut Environment,
    target: GLenum,
    num_attachments: GLsizei,
    attachments: ConstPtr<GLenum>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(attachments.cast(), (num_attachments as GuestUSize) * 4);
        gles.InvalidateFramebuffer(target, num_attachments, slice.as_ptr().cast())
    });
}

#[allow(clippy::too_many_arguments)]
fn glInvalidateSubFramebuffer(
    env: &mut Environment,
    target: GLenum,
    num_attachments: GLsizei,
    attachments: ConstPtr<GLenum>,
    x: GLint,
    y: GLint,
    width: GLsizei,
    height: GLsizei,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(attachments.cast(), (num_attachments as GuestUSize) * 4);
        gles.InvalidateSubFramebuffer(
            target,
            num_attachments,
            slice.as_ptr().cast(),
            x,
            y,
            width,
            height,
        )
    });
}

// -- 3D textures and immutable storage --
#[allow(clippy::too_many_arguments)]
fn glTexImage3D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    internalformat: GLint,
    width: GLsizei,
    height: GLsizei,
    depth: GLsizei,
    border: GLint,
    format: GLenum,
    type_: GLenum,
    pixels: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        // PIXEL_UNPACK_BUFFER bound: pixels is a buffer offset. Otherwise
        // it's a guest pointer to a host-readable image.
        let mut buf: GLint = 0;
        // `GL_PIXEL_UNPACK_BUFFER_BINDING` (0x88EF) is an ES 3.0 / GL 2.1+
        // enum; not present in our gles11 binding. Use the literal value.
        gles.GetIntegerv(0x88EF, &mut buf);
        let host_pixels: *const GLvoid = if buf != 0 || pixels.is_null() {
            pixels.cast::<u8>().to_bits() as usize as *const GLvoid
        } else {
            let total = (width as GuestUSize) * (height as GuestUSize) * (depth as GuestUSize) * 4;
            mem.bytes_at(pixels.cast(), total).as_ptr().cast()
        };
        gles.TexImage3D(
            target,
            level,
            internalformat,
            width,
            height,
            depth,
            border,
            format,
            type_,
            host_pixels,
        )
    });
}

#[allow(clippy::too_many_arguments)]
fn glTexSubImage3D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    xoffset: GLint,
    yoffset: GLint,
    zoffset: GLint,
    width: GLsizei,
    height: GLsizei,
    depth: GLsizei,
    format: GLenum,
    type_: GLenum,
    pixels: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let mut buf: GLint = 0;
        // See `glTexImage3D` above for the rationale for the literal.
        gles.GetIntegerv(0x88EF, &mut buf);
        let host_pixels: *const GLvoid = if buf != 0 || pixels.is_null() {
            pixels.cast::<u8>().to_bits() as usize as *const GLvoid
        } else {
            let total = (width as GuestUSize) * (height as GuestUSize) * (depth as GuestUSize) * 4;
            mem.bytes_at(pixels.cast(), total).as_ptr().cast()
        };
        gles.TexSubImage3D(
            target,
            level,
            xoffset,
            yoffset,
            zoffset,
            width,
            height,
            depth,
            format,
            type_,
            host_pixels,
        )
    });
}

#[allow(clippy::too_many_arguments)]
fn glCopyTexSubImage3D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    xoffset: GLint,
    yoffset: GLint,
    zoffset: GLint,
    x: GLint,
    y: GLint,
    width: GLsizei,
    height: GLsizei,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.CopyTexSubImage3D(
            target, level, xoffset, yoffset, zoffset, x, y, width, height,
        )
    });
}

fn glTexStorage2D(
    env: &mut Environment,
    target: GLenum,
    levels: GLsizei,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.TexStorage2D(target, levels, internalformat, width, height)
    });
}

fn glTexStorage3D(
    env: &mut Environment,
    target: GLenum,
    levels: GLsizei,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
    depth: GLsizei,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.TexStorage3D(target, levels, internalformat, width, height, depth)
    });
}

// -- Query objects --
fn glGenQueries(env: &mut Environment, n: GLsizei, ids: MutPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at_mut(ids.cast(), (n as GuestUSize) * 4);
        gles.GenQueries(n, slice.as_mut_ptr().cast())
    });
}

fn glDeleteQueries(env: &mut Environment, n: GLsizei, ids: ConstPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(ids.cast(), (n as GuestUSize) * 4);
        gles.DeleteQueries(n, slice.as_ptr().cast())
    });
}

fn glIsQuery(env: &mut Environment, id: GLuint) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.IsQuery(id) })
}

fn glBeginQuery(env: &mut Environment, target: GLenum, id: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.BeginQuery(target, id) });
}

fn glEndQuery(env: &mut Environment, target: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.EndQuery(target) });
}

fn glGetQueryiv(env: &mut Environment, target: GLenum, pname: GLenum, params: MutPtr<GLint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at_mut(params.cast(), 4);
        gles.GetQueryiv(target, pname, slice.as_mut_ptr().cast())
    });
}

fn glGetQueryObjectuiv(env: &mut Environment, id: GLuint, pname: GLenum, params: MutPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at_mut(params.cast(), 4);
        gles.GetQueryObjectuiv(id, pname, slice.as_mut_ptr().cast())
    });
}

// -- Sampler objects --
fn glGenSamplers(env: &mut Environment, count: GLsizei, samplers: MutPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at_mut(samplers.cast(), (count as GuestUSize) * 4);
        gles.GenSamplers(count, slice.as_mut_ptr().cast())
    });
}

fn glDeleteSamplers(env: &mut Environment, count: GLsizei, samplers: ConstPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(samplers.cast(), (count as GuestUSize) * 4);
        gles.DeleteSamplers(count, slice.as_ptr().cast())
    });
}

fn glIsSampler(env: &mut Environment, sampler: GLuint) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.IsSampler(sampler) })
}

fn glBindSampler(env: &mut Environment, unit: GLuint, sampler: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.BindSampler(unit, sampler) });
}

fn glSamplerParameteri(env: &mut Environment, sampler: GLuint, pname: GLenum, param: GLint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.SamplerParameteri(sampler, pname, param)
    });
}

fn glSamplerParameterf(env: &mut Environment, sampler: GLuint, pname: GLenum, param: GLfloat) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.SamplerParameterf(sampler, pname, param)
    });
}

// -- Transform feedback --
fn glBeginTransformFeedback(env: &mut Environment, primitive_mode: GLenum) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.BeginTransformFeedback(primitive_mode)
    });
}

fn glEndTransformFeedback(env: &mut Environment) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.EndTransformFeedback() });
}

fn glBindTransformFeedback(env: &mut Environment, target: GLenum, id: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.BindTransformFeedback(target, id)
    });
}

fn glDeleteTransformFeedbacks(env: &mut Environment, n: GLsizei, ids: ConstPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(ids.cast(), (n as GuestUSize) * 4);
        gles.DeleteTransformFeedbacks(n, slice.as_ptr().cast())
    });
}

fn glGenTransformFeedbacks(env: &mut Environment, n: GLsizei, ids: MutPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at_mut(ids.cast(), (n as GuestUSize) * 4);
        gles.GenTransformFeedbacks(n, slice.as_mut_ptr().cast())
    });
}

fn glIsTransformFeedback(env: &mut Environment, id: GLuint) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.IsTransformFeedback(id) })
}

fn glPauseTransformFeedback(env: &mut Environment) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.PauseTransformFeedback() });
}

fn glResumeTransformFeedback(env: &mut Environment) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.ResumeTransformFeedback() });
}

// -- Integer vertex attributes --
fn glVertexAttribIPointer(
    env: &mut Environment,
    index: GLuint,
    size: GLint,
    type_: GLenum,
    stride: GLsizei,
    pointer: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        // ARRAY_BUFFER bound → `pointer` is interpreted as a byte offset
        // into the bound VBO; not as a guest pointer. In either case the
        // 32-bit "pointer" value is forwarded verbatim — when no VBO is
        // bound the host driver reads from guest memory at the actual
        // pointer, which is mapped into the host address space.
        let host_ptr = pointer.cast::<u8>().to_bits() as usize as *const GLvoid;
        gles.VertexAttribIPointer(index, size, type_, stride, host_ptr)
    });
}

fn glVertexAttribI4i(env: &mut Environment, index: GLuint, x: GLint, y: GLint, z: GLint, w: GLint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.VertexAttribI4i(index, x, y, z, w)
    });
}

fn glVertexAttribI4ui(
    env: &mut Environment,
    index: GLuint,
    x: GLuint,
    y: GLuint,
    z: GLuint,
    w: GLuint,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.VertexAttribI4ui(index, x, y, z, w)
    });
}

// -- 3D / compressed-3D textures (OpenGL ES 3.0 §3.8.6) --
fn glCompressedTexImage3D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
    depth: GLsizei,
    border: GLint,
    image_size: GLsizei,
    data: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let data = if data.is_null() {
            std::ptr::null()
        } else {
            mem.ptr_at(data.cast::<u8>(), image_size.max(0) as GuestUSize)
                .cast()
        };
        gles.CompressedTexImage3D(
            target,
            level,
            internalformat,
            width,
            height,
            depth,
            border,
            image_size,
            data,
        )
    });
}

fn glCompressedTexSubImage3D(
    env: &mut Environment,
    target: GLenum,
    level: GLint,
    xoffset: GLint,
    yoffset: GLint,
    zoffset: GLint,
    width: GLsizei,
    height: GLsizei,
    depth: GLsizei,
    format: GLenum,
    image_size: GLsizei,
    data: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let data = if data.is_null() {
            std::ptr::null()
        } else {
            mem.ptr_at(data.cast::<u8>(), image_size.max(0) as GuestUSize)
                .cast()
        };
        gles.CompressedTexSubImage3D(
            target, level, xoffset, yoffset, zoffset, width, height, depth, format, image_size,
            data,
        )
    });
}

// -- Sampler vector parameters (OpenGL ES 3.0 §3.8.10) --
fn glSamplerParameteriv(
    env: &mut Environment,
    sampler: GLuint,
    pname: GLenum,
    params: ConstPtr<GLint>,
) {
    let params = env.mem.ptr_at(params, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.SamplerParameteriv(sampler, pname, params)
    });
}

fn glSamplerParameterfv(
    env: &mut Environment,
    sampler: GLuint,
    pname: GLenum,
    params: ConstPtr<GLfloat>,
) {
    let params = env.mem.ptr_at(params, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.SamplerParameterfv(sampler, pname, params)
    });
}

fn glGetSamplerParameteriv(
    env: &mut Environment,
    sampler: GLuint,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    let params = env.mem.ptr_at_mut(params, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetSamplerParameteriv(sampler, pname, params)
    });
}

fn glGetSamplerParameterfv(
    env: &mut Environment,
    sampler: GLuint,
    pname: GLenum,
    params: MutPtr<GLfloat>,
) {
    let params = env.mem.ptr_at_mut(params, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetSamplerParameterfv(sampler, pname, params)
    });
}

// -- Indexed / 64-bit state queries (OpenGL ES 3.0 §6.1.1) --
fn glGetInteger64v(env: &mut Environment, pname: GLenum, data: MutPtr<i64>) {
    let data = env.mem.ptr_at_mut(data, 16);
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.GetInteger64v(pname, data) });
}

fn glGetIntegeri_v(env: &mut Environment, target: GLenum, index: GLuint, data: MutPtr<GLint>) {
    let data = env.mem.ptr_at_mut(data, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetIntegeri_v(target, index, data)
    });
}

fn glGetInteger64i_v(env: &mut Environment, target: GLenum, index: GLuint, data: MutPtr<i64>) {
    let data = env.mem.ptr_at_mut(data, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetInteger64i_v(target, index, data)
    });
}

fn glGetBufferParameteri64v(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: MutPtr<i64>,
) {
    let params = env.mem.ptr_at_mut(params, 1);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetBufferParameteri64v(target, pname, params)
    });
}

fn glGetInternalformativ(
    env: &mut Environment,
    target: GLenum,
    internalformat: GLenum,
    pname: GLenum,
    buf_size: GLsizei,
    params: MutPtr<GLint>,
) {
    if buf_size <= 0 {
        return;
    }
    let params = env.mem.ptr_at_mut(params, buf_size as GuestUSize);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetInternalformativ(target, internalformat, pname, buf_size, params)
    });
}

// -- Integer vertex attributes (OpenGL ES 3.0 §2.7 / §6.1.10) --
fn glVertexAttribI4iv(env: &mut Environment, index: GLuint, v: ConstPtr<GLint>) {
    let v = env.mem.ptr_at(v, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.VertexAttribI4iv(index, v) });
}

fn glVertexAttribI4uiv(env: &mut Environment, index: GLuint, v: ConstPtr<GLuint>) {
    let v = env.mem.ptr_at(v, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.VertexAttribI4uiv(index, v)
    });
}

// -- Vertex attribute queries (OpenGL ES 2.0 §6.1.8) --
// The largest query (GL_CURRENT_VERTEX_ATTRIB) returns four values, so the
// out-buffer is always mapped as 4 elements, matching Apple's headers.

fn glGetVertexAttribfv(
    env: &mut Environment,
    index: GLuint,
    pname: GLenum,
    params: MutPtr<GLfloat>,
) {
    let params = env.mem.ptr_at_mut(params, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetVertexAttribfv(index, pname, params)
    });
}

fn glGetVertexAttribiv(env: &mut Environment, index: GLuint, pname: GLenum, params: MutPtr<GLint>) {
    let params = env.mem.ptr_at_mut(params, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetVertexAttribiv(index, pname, params)
    });
}

/// `void glGetVertexAttribPointerv(GLuint index, GLenum pname, void **pointer)`
/// — OpenGL ES 2.0 §6.1.8. The result is either a buffer offset (when a
/// buffer object is bound to the attribute) or a client-side array pointer,
/// which must be translated from a host address back to a guest address.
fn glGetVertexAttribPointerv(
    env: &mut Environment,
    index: GLuint,
    pname: GLenum,
    pointer: MutPtr<ConstVoidPtr>,
) {
    const VERTEX_ATTRIB_ARRAY_BUFFER_BINDING: GLenum = 0x889F;
    with_ctx_and_mem(env, |gles, mem| {
        let mut buffer_binding: GLint = 0;
        let mut host_pointer_or_offset: *mut GLvoid = std::ptr::null_mut();
        let guest_pointer_or_offset = unsafe {
            gles.GetVertexAttribiv(
                index,
                VERTEX_ATTRIB_ARRAY_BUFFER_BINDING,
                &mut buffer_binding,
            );
            gles.GetVertexAttribPointerv(index, pname, &mut host_pointer_or_offset);
            if buffer_binding != 0 {
                // A buffer object is bound: the "pointer" is really an offset
                // and must be passed through unchanged.
                Ptr::from_bits(u32::try_from(host_pointer_or_offset as usize).unwrap_or(0))
            } else if host_pointer_or_offset.is_null() {
                Ptr::null()
            } else {
                mem.host_ptr_to_guest_ptr(host_pointer_or_offset)
            }
        };
        mem.write(pointer, guest_pointer_or_offset);
    });
}

fn glGetVertexAttribIiv(
    env: &mut Environment,
    index: GLuint,
    pname: GLenum,
    params: MutPtr<GLint>,
) {
    let params = env.mem.ptr_at_mut(params, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetVertexAttribIiv(index, pname, params)
    });
}

fn glGetVertexAttribIuiv(
    env: &mut Environment,
    index: GLuint,
    pname: GLenum,
    params: MutPtr<GLuint>,
) {
    let params = env.mem.ptr_at_mut(params, 4);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetVertexAttribIuiv(index, pname, params)
    });
}

// -- Unsigned-integer uniform queries (OpenGL ES 3.0 §6.1.14 / §2.12.6) --
fn glGetUniformuiv(
    env: &mut Environment,
    program: GLuint,
    location: GLint,
    params: MutPtr<GLuint>,
) {
    let params = env.mem.ptr_at_mut(params, 16);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetUniformuiv(program, location, params)
    });
}

/// `void glGetUniformIndices(GLuint program, GLsizei uniformCount,
///   const GLchar *const *uniformNames, GLuint *uniformIndices)` — ES 3.0
/// §2.12.6.
fn glGetUniformIndices(
    env: &mut Environment,
    program: GLuint,
    uniform_count: GLsizei,
    uniform_names: ConstPtr<ConstPtr<GLubyte>>,
    uniform_indices: MutPtr<GLuint>,
) {
    if uniform_count <= 0 {
        return;
    }
    let n = uniform_count as usize;
    let mut owned: Vec<std::ffi::CString> = Vec::with_capacity(n);
    for i in 0..n {
        let name_ptr: ConstPtr<GLubyte> = env.mem.read(uniform_names + (i as GuestUSize));
        if name_ptr.is_null() {
            owned.push(std::ffi::CString::default());
        } else {
            owned.push(
                std::ffi::CString::new(env.mem.cstr_at(name_ptr).to_vec()).unwrap_or_default(),
            );
        }
    }
    let ptrs: Vec<*const std::os::raw::c_char> = owned.iter().map(|s| s.as_ptr()).collect();
    let mut results: Vec<GLuint> = vec![0; n];
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetUniformIndices(program, uniform_count, ptrs.as_ptr(), results.as_mut_ptr());
    });
    for (i, v) in results.iter().enumerate() {
        env.mem.write(uniform_indices + (i as GuestUSize), *v);
    }
}

// -- Transform feedback varyings (OpenGL ES 3.0 §2.15.2 / §6.1.12) --
fn glTransformFeedbackVaryings(
    env: &mut Environment,
    program: GLuint,
    count: GLsizei,
    varyings: ConstPtr<ConstPtr<GLubyte>>,
    buffer_mode: GLenum,
) {
    if count <= 0 {
        return;
    }
    let n = count as usize;
    let mut owned: Vec<std::ffi::CString> = Vec::with_capacity(n);
    for i in 0..n {
        let name_ptr: ConstPtr<GLubyte> = env.mem.read(varyings + (i as GuestUSize));
        if name_ptr.is_null() {
            owned.push(std::ffi::CString::default());
        } else {
            owned.push(
                std::ffi::CString::new(env.mem.cstr_at(name_ptr).to_vec()).unwrap_or_default(),
            );
        }
    }
    let ptrs: Vec<*const std::os::raw::c_char> = owned.iter().map(|s| s.as_ptr()).collect();
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.TransformFeedbackVaryings(program, count, ptrs.as_ptr(), buffer_mode);
    });
}

fn glGetTransformFeedbackVarying(
    env: &mut Environment,
    program: GLuint,
    index: GLuint,
    buf_size: GLsizei,
    length: MutPtr<GLsizei>,
    size: MutPtr<GLint>,
    type_: MutPtr<GLenum>,
    name: MutPtr<GLubyte>,
) {
    let mut host_length: GLsizei = 0;
    let mut host_size: GLint = 0;
    let mut host_type: GLenum = 0;
    let mut host_name: Vec<u8> = vec![0u8; buf_size.max(0) as usize];
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetTransformFeedbackVarying(
            program,
            index,
            buf_size,
            &mut host_length,
            &mut host_size,
            &mut host_type,
            host_name.as_mut_ptr().cast(),
        );
    });
    if !length.is_null() {
        env.mem.write(length, host_length);
    }
    if !size.is_null() {
        env.mem.write(size, host_size);
    }
    if !type_.is_null() {
        env.mem.write(type_, host_type);
    }
    if !name.is_null() && buf_size > 0 {
        let copy = (host_length as usize + 1).min(buf_size as usize);
        let dst = env.mem.ptr_at_mut(name, copy as GuestUSize);
        unsafe {
            std::ptr::copy_nonoverlapping(host_name.as_ptr(), dst, copy);
        }
    }
}

// -- Sync object query (OpenGL ES 3.0 §6.1.8) --
fn glGetSynciv(
    env: &mut Environment,
    sync: GLuint,
    pname: GLenum,
    buf_size: GLsizei,
    length: MutPtr<GLsizei>,
    values: MutPtr<GLint>,
) {
    let mut host_length: GLsizei = 0;
    let count = buf_size.max(0) as usize;
    let mut host_values: Vec<GLint> = vec![0; count];
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetSynciv(
            sync as usize,
            pname,
            buf_size,
            &mut host_length,
            host_values.as_mut_ptr(),
        );
    });
    if !length.is_null() {
        env.mem.write(length, host_length);
    }
    let written = (host_length.max(0) as usize).min(count);
    for i in 0..written {
        env.mem.write(values + (i as GuestUSize), host_values[i]);
    }
}

/// `void glGetBufferPointerv(GLenum target, GLenum pname, void **params)` —
/// ES 3.0 §6.1.15. The only valid `pname` is `GL_BUFFER_MAP_POINTER`, which
/// is NULL unless the buffer is currently mapped. touchHLE exposes buffer
/// mappings to the guest via glMapBufferOES (which hands the app a *guest*
/// pointer and keeps its own host<->guest mirror), so the raw host pointer
/// the driver returns is not meaningful in the guest address space. We still
/// forward the real query, and report the guest-visible result, which is NULL
/// when no guest-visible mapping is active — the GL default and common case.
fn glGetBufferPointerv(
    env: &mut Environment,
    target: GLenum,
    pname: GLenum,
    params: MutPtr<MutVoidPtr>,
) {
    let mut host_ptr: *mut GLvoid = std::ptr::null_mut();
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.GetBufferPointerv(target, pname, &mut host_ptr);
    });
    let _ = host_ptr;
    env.mem.write(params, Ptr::null());
}

// -- Integer uniforms --
fn glUniform1ui(env: &mut Environment, location: GLint, v0: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.Uniform1ui(location, v0) });
}

fn glUniform2ui(env: &mut Environment, location: GLint, v0: GLuint, v1: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Uniform2ui(location, v0, v1)
    });
}

fn glUniform3ui(env: &mut Environment, location: GLint, v0: GLuint, v1: GLuint, v2: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Uniform3ui(location, v0, v1, v2)
    });
}

fn glUniform4ui(
    env: &mut Environment,
    location: GLint,
    v0: GLuint,
    v1: GLuint,
    v2: GLuint,
    v3: GLuint,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.Uniform4ui(location, v0, v1, v2, v3)
    });
}

fn glUniform1uiv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), (count as GuestUSize) * 4);
        gles.Uniform1uiv(location, count, slice.as_ptr().cast())
    });
}

fn glUniform2uiv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), (count as GuestUSize) * 8);
        gles.Uniform2uiv(location, count, slice.as_ptr().cast())
    });
}

fn glUniform3uiv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), (count as GuestUSize) * 12);
        gles.Uniform3uiv(location, count, slice.as_ptr().cast())
    });
}

fn glUniform4uiv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLuint>) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), (count as GuestUSize) * 16);
        gles.Uniform4uiv(location, count, slice.as_ptr().cast())
    });
}

fn glUniformMatrix2x3fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), (count as GuestUSize) * 24);
        gles.UniformMatrix2x3fv(location, count, transpose, slice.as_ptr().cast())
    });
}

fn glUniformMatrix3x2fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), (count as GuestUSize) * 24);
        gles.UniformMatrix3x2fv(location, count, transpose, slice.as_ptr().cast())
    });
}

fn glUniformMatrix2x4fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), (count as GuestUSize) * 32);
        gles.UniformMatrix2x4fv(location, count, transpose, slice.as_ptr().cast())
    });
}

fn glUniformMatrix4x2fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), (count as GuestUSize) * 32);
        gles.UniformMatrix4x2fv(location, count, transpose, slice.as_ptr().cast())
    });
}

fn glUniformMatrix3x4fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), (count as GuestUSize) * 48);
        gles.UniformMatrix3x4fv(location, count, transpose, slice.as_ptr().cast())
    });
}

fn glUniformMatrix4x3fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let slice = mem.bytes_at(value.cast(), (count as GuestUSize) * 48);
        gles.UniformMatrix4x3fv(location, count, transpose, slice.as_ptr().cast())
    });
}

// -- Uniform blocks --
fn glGetUniformBlockIndex(
    env: &mut Environment,
    program: GLuint,
    uniform_block_name: ConstPtr<u8>,
) -> GLuint {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let name_cstr = mem.cstr_at(uniform_block_name);
        let name_c = std::ffi::CString::new(name_cstr).unwrap();
        gles.GetUniformBlockIndex(program, name_c.as_ptr())
    })
}

fn glUniformBlockBinding(
    env: &mut Environment,
    program: GLuint,
    uniform_block_index: GLuint,
    uniform_block_binding: GLuint,
) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.UniformBlockBinding(program, uniform_block_index, uniform_block_binding)
    });
}

// -- Sync objects --
//
// `GLsync` is opaque on the host (a pointer). We expose it to the guest as
// an opaque 32-bit handle. EAGL keeps the mapping in
// `EAGLContextHostObject::sync_objects`.

fn glFenceSync(env: &mut Environment, condition: GLenum, flags: GLbitfield) -> GLuint {
    let host_sync = with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.FenceSync(condition, flags)
    });
    // Allocate a guest-visible "handle" by storing the host sync in a
    // table keyed by its own address (which is unique). Truncate to 32
    // bits for the handle exposed to guest code.
    host_sync as GLuint
}

fn glIsSync(env: &mut Environment, sync: GLuint) -> GLboolean {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.IsSync(sync as usize) })
}

fn glDeleteSync(env: &mut Environment, sync: GLuint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe { gles.DeleteSync(sync as usize) });
}

fn glClientWaitSync(
    env: &mut Environment,
    sync: GLuint,
    flags: GLbitfield,
    timeout_lo: GLuint,
    timeout_hi: GLuint,
) -> GLenum {
    let timeout = ((timeout_hi as u64) << 32) | (timeout_lo as u64);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.ClientWaitSync(sync as usize, flags, timeout)
    })
}

fn glWaitSync(
    env: &mut Environment,
    sync: GLuint,
    flags: GLbitfield,
    timeout_lo: GLuint,
    timeout_hi: GLuint,
) {
    let timeout = ((timeout_hi as u64) << 32) | (timeout_lo as u64);
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.WaitSync(sync as usize, flags, timeout)
    });
}

// -- Misc --
fn glGetStringi(env: &mut Environment, name: GLenum, index: GLuint) -> ConstPtr<u8> {
    let host_ptr: *const GLubyte =
        with_ctx_and_mem(env, |gles, _mem| unsafe { gles.GetStringi(name, index) });
    if host_ptr.is_null() {
        return Ptr::null();
    }
    // Copy the C string into guest memory once and intern the pointer in
    // EAGL's per-context string cache.
    let bytes = unsafe { std::ffi::CStr::from_ptr(host_ptr as *const _) }.to_bytes_with_nul();
    let guest_buf: MutPtr<u8> = env.mem.alloc(bytes.len() as GuestUSize).cast();
    env.mem
        .bytes_at_mut(guest_buf, bytes.len() as GuestUSize)
        .copy_from_slice(bytes);
    guest_buf.cast_const()
}

fn glGetFragDataLocation(env: &mut Environment, program: GLuint, name: ConstPtr<u8>) -> GLint {
    with_ctx_and_mem(env, |gles, mem| unsafe {
        let name_cstr = mem.cstr_at(name);
        let name_c = std::ffi::CString::new(name_cstr).unwrap();
        gles.GetFragDataLocation(program, name_c.as_ptr())
    })
}

fn glProgramParameteri(env: &mut Environment, program: GLuint, pname: GLenum, value: GLint) {
    with_ctx_and_mem(env, |gles, _mem| unsafe {
        gles.ProgramParameteri(program, pname, value)
    });
}

unsafe fn clamp_fog_state_values(gles: &mut dyn GLES) -> Option<(f32, f32)> {
    // ES2/3 has shader fog; fixed-function enum queries poison GL error state.
    if gles.is_es2() { return None; }
    let mut fog_enabled: GLboolean = 0;
    gles.GetBooleanv(gles11::FOG, &mut fog_enabled);
    if fog_enabled != 0 {
        let mut fog_start: GLfloat = 0.0;
        let mut fog_end: GLfloat = 0.0;
        gles.GetFloatv(gles11::FOG_START, &mut fog_start);
        gles.GetFloatv(gles11::FOG_END, &mut fog_end);
        if fog_start == fog_end {
            let new_fog_start = fog_end - 0.001;
            gles.Fogf(gles11::FOG_START, new_fog_start);
            return Some((fog_start, fog_end));
        }
    }
    None
}
unsafe fn restore_fog_state_values(gles: &mut dyn GLES, from_backup: Option<(f32, f32)>) {
    if let Some((fog_start, fog_end)) = from_backup {
        gles.Fogf(gles11::FOG_START, fog_start);
        gles.Fogf(gles11::FOG_END, fog_end);
    }
}

/// `void glLabelObjectEXT(GLenum type, GLuint object, GLsizei length, const GLchar *label)`
///
/// Part of GL_EXT_debug_label.  Labels an OpenGL ES object for debugging
/// purposes.  The label is used only by GPU debugging tools (Instruments,
/// RenderDoc) and has no effect on rendering.  We expose a no-op
/// implementation so that apps which unconditionally call this extension
/// function no longer trigger the "unimplemented function" warning.
///
/// Reference: <https://registry.khronos.org/OpenGL/extensions/EXT/EXT_debug_label.txt>
fn glLabelObjectEXT(
    _env: &mut Environment,
    _type_: GLenum,
    _object: GLuint,
    _length: GLsizei,
    _label: ConstPtr<u8>,
) {
    // No-op: debug labels have no functional impact.
}

/// `void glGetObjectLabelEXT(GLenum type, GLuint object, GLsizei bufSize,
///                            GLsizei *length, GLchar *label)`
///
/// Retrieves the debug label previously set by glLabelObjectEXT.  Since we
/// do not store labels, we return an empty string.
///
/// Reference: <https://registry.khronos.org/OpenGL/extensions/EXT/EXT_debug_label.txt>
fn glGetObjectLabelEXT(
    env: &mut Environment,
    _type_: GLenum,
    _object: GLuint,
    buf_size: GLsizei,
    length: MutPtr<GLsizei>,
    label: MutPtr<u8>,
) {
    if !label.is_null() && buf_size > 0 {
        env.mem.write(label, 0u8);
    }
    if !length.is_null() {
        env.mem.write(length, 0);
    }
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(glGetError()),
    export_c_func!(glEnable(_)),
    export_c_func!(glIsEnabled(_)),
    export_c_func!(glDisable(_)),
    export_c_func!(glClientActiveTexture(_)),
    export_c_func!(glEnableClientState(_)),
    export_c_func!(glDisableClientState(_)),
    export_c_func!(glGetBooleanv(_, _)),
    export_c_func!(glGetFloatv(_, _)),
    export_c_func!(glGetIntegerv(_, _)),
    export_c_func!(glGetFixedv(_, _)),
    export_c_func!(glGetPointerv(_, _)),
    export_c_func!(glGetTexEnviv(_, _, _)),
    export_c_func!(glGetTexEnvfv(_, _, _)),
    export_c_func!(glHint(_, _)),
    export_c_func!(glFinish()),
    export_c_func!(glFlush()),
    export_c_func!(glGetString(_)),
    export_c_func!(glAlphaFunc(_, _)),
    export_c_func!(glAlphaFuncx(_, _)),
    export_c_func!(glBlendFunc(_, _)),
    export_c_func!(glBlendEquationOES(_)),
    export_c_func!(glColorMask(_, _, _, _)),
    export_c_func!(glClipPlanef(_, _)),
    export_c_func!(glClipPlanex(_, _)),
    export_c_func!(glCullFace(_)),
    export_c_func!(glDepthFunc(_)),
    export_c_func!(glDepthMask(_)),
    export_c_func!(glDepthRangef(_, _)),
    export_c_func!(glDepthRangex(_, _)),
    export_c_func!(glFrontFace(_)),
    export_c_func!(glPolygonOffset(_, _)),
    export_c_func!(glPolygonOffsetx(_, _)),
    export_c_func!(glSampleCoverage(_, _)),
    export_c_func!(glSampleCoveragex(_, _)),
    export_c_func!(glShadeModel(_)),
    export_c_func!(glScissor(_, _, _, _)),
    export_c_func!(glViewport(_, _, _, _)),
    export_c_func!(glLineWidth(_)),
    export_c_func!(glLineWidthx(_)),
    export_c_func!(glStencilFunc(_, _, _)),
    export_c_func!(glStencilOp(_, _, _)),
    export_c_func!(glStencilMask(_)),
    export_c_func!(glLogicOp(_)),
    export_c_func!(glPointSize(_)),
    export_c_func!(glPointSizex(_)),
    export_c_func!(glPointParameterf(_, _)),
    export_c_func!(glPointParameterx(_, _)),
    export_c_func!(glPointParameterfv(_, _)),
    export_c_func!(glPointParameterxv(_, _)),
    export_c_func!(glFogf(_, _)),
    export_c_func!(glFogx(_, _)),
    export_c_func!(glFogfv(_, _)),
    export_c_func!(glFogxv(_, _)),
    export_c_func!(glLightf(_, _, _)),
    export_c_func!(glLightx(_, _, _)),
    export_c_func!(glLightfv(_, _, _)),
    export_c_func!(glLightxv(_, _, _)),
    export_c_func!(glLightModelf(_, _)),
    export_c_func!(glLightModelx(_, _)),
    export_c_func!(glLightModelfv(_, _)),
    export_c_func!(glLightModelxv(_, _)),
    export_c_func!(glMaterialf(_, _, _)),
    export_c_func!(glMaterialx(_, _, _)),
    export_c_func!(glMaterialfv(_, _, _)),
    export_c_func!(glMaterialxv(_, _, _)),
    export_c_func!(glIsBuffer(_)),
    export_c_func!(glGenBuffers(_, _)),
    export_c_func!(glDeleteBuffers(_, _)),
    export_c_func!(glBindBuffer(_, _)),
    export_c_func!(glBufferData(_, _, _, _)),
    export_c_func!(glBufferSubData(_, _, _, _)),
    export_c_func!(glColor4f(_, _, _, _)),
    export_c_func!(glColor4x(_, _, _, _)),
    export_c_func!(glColor4ub(_, _, _, _)),
    export_c_func!(glNormal3f(_, _, _)),
    export_c_func!(glNormal3x(_, _, _)),
    export_c_func!(glColorPointer(_, _, _, _)),
    export_c_func!(glNormalPointer(_, _, _)),
    export_c_func!(glTexCoordPointer(_, _, _, _)),
    export_c_func!(glVertexPointer(_, _, _, _)),
    export_c_func!(glPointSizePointerOES(_, _, _)),
    export_c_func!(glGetTexParameteriv(_, _, _)),
    export_c_func!(glGetTexParameterfv(_, _, _)),
    export_c_func!(glGetTexParameterxv(_, _, _)),
    export_c_func!(glGetTexEnvxv(_, _, _)),
    export_c_func!(glGetClipPlanef(_, _)),
    export_c_func!(glGetClipPlanex(_, _)),
    export_c_func!(glGetLightfv(_, _, _)),
    export_c_func!(glGetLightxv(_, _, _)),
    export_c_func!(glGetMaterialfv(_, _, _)),
    export_c_func!(glGetMaterialxv(_, _, _)),
    export_c_func!(glCompressedTexSubImage2D(_, _, _, _, _, _, _, _, _)),
    export_c_func!(glDrawTexfOES(_, _, _, _, _)),
    export_c_func!(glDrawTexiOES(_, _, _, _, _)),
    export_c_func!(glDrawTexxOES(_, _, _, _, _)),
    export_c_func!(glDrawTexfvOES(_)),
    export_c_func!(glDrawTexivOES(_)),
    export_c_func!(glDrawTexxvOES(_)),
    export_c_func!(glDrawTexsOES(_, _, _, _, _)),
    export_c_func!(glDrawTexsvOES(_)),
    export_c_func!(glRenderbufferStorageMultisampleAPPLE(_, _, _, _, _)),
    export_c_func!(glResolveMultisampleFramebufferAPPLE()),
    export_c_func!(glDiscardFramebufferEXT(_, _, _)),
    export_c_func!(glPushGroupMarkerEXT(_, _)),
    export_c_func!(glPopGroupMarkerEXT()),
    export_c_func!(glBindVertexArrayOES(_)),
    export_c_func!(glDeleteVertexArraysOES(_, _)),
    export_c_func!(glGenVertexArraysOES(_, _)),
    export_c_func!(glIsVertexArrayOES(_)),
    export_c_func!(glCurrentPaletteMatrixOES(_)),
    export_c_func!(glLoadPaletteFromModelViewMatrixOES()),
    export_c_func!(glMatrixIndexPointerOES(_, _, _, _)),
    export_c_func!(glWeightPointerOES(_, _, _, _)),
    export_c_func!(glGetBufferPointervOES(_, _, _)),
    export_c_func!(glDrawArrays(_, _, _)),
    export_c_func!(glDrawElements(_, _, _, _)),
    export_c_func!(glClear(_)),
    export_c_func!(glClearColor(_, _, _, _)),
    export_c_func!(glClearColorx(_, _, _, _)),
    export_c_func!(glClearDepthf(_)),
    export_c_func!(glClearDepthx(_)),
    export_c_func!(glClearStencil(_)),
    export_c_func!(glMatrixMode(_)),
    export_c_func!(glLoadIdentity()),
    export_c_func!(glLoadMatrixf(_)),
    export_c_func!(glLoadMatrixx(_)),
    export_c_func!(glMultMatrixf(_)),
    export_c_func!(glMultMatrixx(_)),
    export_c_func!(glPushMatrix()),
    export_c_func!(glPopMatrix()),
    export_c_func!(glOrthof(_, _, _, _, _, _)),
    export_c_func!(glOrthox(_, _, _, _, _, _)),
    export_c_func!(glFrustumf(_, _, _, _, _, _)),
    export_c_func!(glFrustumx(_, _, _, _, _, _)),
    export_c_func!(glRotatef(_, _, _, _)),
    export_c_func!(glRotatex(_, _, _, _)),
    export_c_func!(glScalef(_, _, _)),
    export_c_func!(glScalex(_, _, _)),
    export_c_func!(glTranslatef(_, _, _)),
    export_c_func!(glTranslatex(_, _, _)),
    export_c_func!(glPixelStorei(_, _)),
    export_c_func!(glReadPixels(_, _, _, _, _, _, _)),
    export_c_func!(glGenTextures(_, _)),
    export_c_func!(glDeleteTextures(_, _)),
    export_c_func!(glActiveTexture(_)),
    export_c_func!(glIsTexture(_)),
    export_c_func!(glBindTexture(_, _)),
    export_c_func!(glTexParameteri(_, _, _)),
    export_c_func!(glTexParameterf(_, _, _)),
    export_c_func!(glTexParameterx(_, _, _)),
    export_c_func!(glTexParameteriv(_, _, _)),
    export_c_func!(glTexParameterfv(_, _, _)),
    export_c_func!(glTexParameterxv(_, _, _)),
    export_c_func!(glTexImage2D(_, _, _, _, _, _, _, _, _)),
    export_c_func!(glTexSubImage2D(_, _, _, _, _, _, _, _, _)),
    export_c_func!(glCompressedTexImage2D(_, _, _, _, _, _, _, _)),
    export_c_func!(glCopyTexImage2D(_, _, _, _, _, _, _, _)),
    export_c_func!(glCopyTexSubImage2D(_, _, _, _, _, _, _, _)),
    export_c_func!(glTexEnvf(_, _, _)),
    export_c_func!(glTexEnvx(_, _, _)),
    export_c_func!(glTexEnvi(_, _, _)),
    export_c_func!(glTexEnvfv(_, _, _)),
    export_c_func!(glTexEnvxv(_, _, _)),
    export_c_func!(glTexEnviv(_, _, _)),
    export_c_func!(glMultiTexCoord4f(_, _, _, _, _)),
    export_c_func!(glMultiTexCoord4x(_, _, _, _, _)),
    export_c_func!(glGenFramebuffersOES(_, _)),
    export_c_func!(glGenRenderbuffersOES(_, _)),
    export_c_func!(glIsFramebufferOES(_)),
    export_c_func!(glIsRenderbufferOES(_)),
    export_c_func!(glBindFramebufferOES(_, _)),
    export_c_func!(glBindRenderbufferOES(_, _)),
    export_c_func!(glRenderbufferStorageOES(_, _, _, _)),
    export_c_func!(glFramebufferRenderbufferOES(_, _, _, _)),
    export_c_func!(glFramebufferTexture2DOES(_, _, _, _, _)),
    export_c_func!(glGetFramebufferAttachmentParameterivOES(_, _, _, _)),
    export_c_func!(glGetRenderbufferParameterivOES(_, _, _)),
    export_c_func!(glCheckFramebufferStatusOES(_)),
    export_c_func!(glDeleteFramebuffersOES(_, _)),
    export_c_func!(glDeleteRenderbuffersOES(_, _)),
    export_c_func!(glGenerateMipmapOES(_)),
    export_c_func!(glGenFramebuffers(_, _)),
    export_c_func!(glGenRenderbuffers(_, _)),
    export_c_func!(glIsFramebuffer(_)),
    export_c_func!(glIsRenderbuffer(_)),
    export_c_func!(glBindFramebuffer(_, _)),
    export_c_func!(glBindRenderbuffer(_, _)),
    export_c_func!(glRenderbufferStorage(_, _, _, _)),
    export_c_func!(glFramebufferRenderbuffer(_, _, _, _)),
    export_c_func!(glFramebufferTexture2D(_, _, _, _, _)),
    export_c_func!(glGetFramebufferAttachmentParameteriv(_, _, _, _)),
    export_c_func!(glGetRenderbufferParameteriv(_, _, _)),
    export_c_func!(glCheckFramebufferStatus(_)),
    export_c_func!(glDeleteFramebuffers(_, _)),
    export_c_func!(glDeleteRenderbuffers(_, _)),
    export_c_func!(glGenerateMipmap(_)),
    export_c_func!(glGetBufferParameteriv(_, _, _)),
    export_c_func!(glMapBufferOES(_, _)),
    export_c_func!(glUnmapBufferOES(_)),
    // OpenGL ES 2.0 entry points
    export_c_func!(glCreateProgram()),
    export_c_func!(glCreateShader(_)),
    export_c_func!(glBindAttribLocation(_, _, _)),
    export_c_func!(glGetAttribLocation(_, _)),
    export_c_func!(glGetUniformLocation(_, _)),
    export_c_func!(glUniformMatrix2fv(_, _, _, _)),
    export_c_func!(glUniformMatrix3fv(_, _, _, _)),
    export_c_func!(glUniformMatrix4fv(_, _, _, _)),
    export_c_func!(glUseProgram(_)),
    export_c_func!(glDeleteProgram(_)),
    export_c_func!(glDeleteShader(_)),
    export_c_func!(glCompileShader(_)),
    export_c_func!(glGetShaderPrecisionFormat(_, _, _, _)),
    export_c_func!(glAttachShader(_, _)),
    export_c_func!(glDetachShader(_, _)),
    export_c_func!(glLinkProgram(_)),
    export_c_func!(glValidateProgram(_)),
    export_c_func!(glIsShader(_)),
    export_c_func!(glIsProgram(_)),
    export_c_func!(glGetShaderiv(_, _, _)),
    export_c_func!(glGetShaderInfoLog(_, _, _, _)),
    export_c_func!(glGetShaderSource(_, _, _, _)),
    export_c_func!(glGetProgramiv(_, _, _)),
    export_c_func!(glGetProgramInfoLog(_, _, _, _)),
    export_c_func!(glGetActiveUniform(_, _, _, _, _, _, _)),
    export_c_func!(glGetActiveAttrib(_, _, _, _, _, _, _)),
    export_c_func!(glShaderSource(_, _, _, _)),
    export_c_func!(glEnableVertexAttribArray(_)),
    export_c_func!(glDisableVertexAttribArray(_)),
    export_c_func!(glVertexAttribPointer(_, _, _, _, _, _)),
    export_c_func!(glVertexAttrib1f(_, _)),
    export_c_func!(glVertexAttrib1fv(_, _)),
    export_c_func!(glVertexAttrib2f(_, _, _)),
    export_c_func!(glVertexAttrib3f(_, _, _, _)),
    export_c_func!(glVertexAttrib4f(_, _, _, _, _)),
    export_c_func!(glUniform1i(_, _)),
    export_c_func!(glUniform2i(_, _, _)),
    export_c_func!(glUniform3i(_, _, _, _)),
    export_c_func!(glUniform4i(_, _, _, _, _)),
    export_c_func!(glUniform1f(_, _)),
    export_c_func!(glUniform2f(_, _, _)),
    export_c_func!(glUniform3f(_, _, _, _)),
    export_c_func!(glUniform4f(_, _, _, _, _)),
    export_c_func!(glUniform1iv(_, _, _)),
    export_c_func!(glUniform2iv(_, _, _)),
    export_c_func!(glUniform3iv(_, _, _)),
    export_c_func!(glUniform4iv(_, _, _)),
    export_c_func!(glUniform1fv(_, _, _)),
    export_c_func!(glUniform2fv(_, _, _)),
    export_c_func!(glUniform3fv(_, _, _)),
    export_c_func!(glUniform4fv(_, _, _)),
    export_c_func!(glReleaseShaderCompiler()),
    export_c_func!(glBlendColor(_, _, _, _)),
    export_c_func!(glBlendEquation(_)),
    export_c_func!(glBlendEquationSeparate(_, _)),
    export_c_func!(glBlendFuncSeparate(_, _, _, _)),
    // `GL_OES_blend_equation_separate` / `GL_OES_blend_func_separate`
    // aliases: identical behaviour, just the extension-suffixed names.
    export_c_func_aliased!("glBlendEquationSeparateOES", glBlendEquationSeparate(_, _)),
    export_c_func_aliased!("glBlendFuncSeparateOES", glBlendFuncSeparate(_, _, _, _)),
    export_c_func!(glShaderBinary(_, _, _, _, _)),
    export_c_func!(glGetActiveUniformsiv(_, _, _, _, _)),
    export_c_func!(glGetActiveUniformBlockiv(_, _, _, _)),
    export_c_func!(glGetActiveUniformBlockName(_, _, _, _, _)),
    export_c_func!(glProgramBinary(_, _, _, _)),
    export_c_func!(glGetProgramBinary(_, _, _, _, _)),
    export_c_func!(glStencilFuncSeparate(_, _, _, _)),
    export_c_func!(glStencilOpSeparate(_, _, _, _)),
    export_c_func!(glStencilMaskSeparate(_, _)),
    export_c_func!(glGenVertexArrays(_, _)),
    export_c_func!(glBindVertexArray(_)),
    export_c_func!(glDeleteVertexArrays(_, _)),
    export_c_func!(glIsVertexArray(_)),
    // OpenGL ES 3.0 entry points
    export_c_func!(glUnmapBuffer(_)),
    export_c_func_aliased!("glMapBufferRangeEXT", glMapBufferRange(_, _, _, _)),
    export_c_func!(glMapBufferRange(_, _, _, _)),
    export_c_func_aliased!(
        "glFlushMappedBufferRangeEXT",
        glFlushMappedBufferRange(_, _, _)
    ),
    export_c_func!(glFlushMappedBufferRange(_, _, _)),
    export_c_func!(glCopyBufferSubData(_, _, _, _, _)),
    export_c_func!(glBindBufferBase(_, _, _)),
    export_c_func!(glBindBufferRange(_, _, _, _, _)),
    export_c_func!(glDrawRangeElements(_, _, _, _, _, _)),
    export_c_func!(glDrawArraysInstanced(_, _, _, _)),
    export_c_func!(glDrawElementsInstanced(_, _, _, _, _)),
    export_c_func!(glVertexAttribDivisor(_, _)),
    export_c_func!(glReadBuffer(_)),
    export_c_func!(glDrawBuffers(_, _)),
    export_c_func!(glClearBufferiv(_, _, _)),
    export_c_func!(glClearBufferuiv(_, _, _)),
    export_c_func!(glClearBufferfv(_, _, _)),
    export_c_func!(glClearBufferfi(_, _, _, _)),
    export_c_func!(glBlitFramebuffer(_, _, _, _, _, _, _, _, _, _)),
    export_c_func!(glRenderbufferStorageMultisample(_, _, _, _, _)),
    export_c_func!(glFramebufferTextureLayer(_, _, _, _, _)),
    export_c_func!(glInvalidateFramebuffer(_, _, _)),
    export_c_func!(glInvalidateSubFramebuffer(_, _, _, _, _, _, _)),
    export_c_func!(glTexImage3D(_, _, _, _, _, _, _, _, _, _)),
    export_c_func!(glTexSubImage3D(_, _, _, _, _, _, _, _, _, _, _)),
    export_c_func!(glCopyTexSubImage3D(_, _, _, _, _, _, _, _, _)),
    export_c_func!(glTexStorage2D(_, _, _, _, _)),
    export_c_func!(glTexStorage3D(_, _, _, _, _, _)),
    export_c_func!(glGenQueries(_, _)),
    export_c_func!(glDeleteQueries(_, _)),
    export_c_func!(glIsQuery(_)),
    export_c_func!(glBeginQuery(_, _)),
    export_c_func!(glEndQuery(_)),
    export_c_func!(glGetQueryiv(_, _, _)),
    export_c_func!(glGetQueryObjectuiv(_, _, _)),
    export_c_func!(glGenSamplers(_, _)),
    export_c_func!(glDeleteSamplers(_, _)),
    export_c_func!(glIsSampler(_)),
    export_c_func!(glBindSampler(_, _)),
    export_c_func!(glSamplerParameteri(_, _, _)),
    export_c_func!(glSamplerParameterf(_, _, _)),
    export_c_func!(glBeginTransformFeedback(_)),
    export_c_func!(glEndTransformFeedback()),
    export_c_func!(glBindTransformFeedback(_, _)),
    export_c_func!(glDeleteTransformFeedbacks(_, _)),
    export_c_func!(glGenTransformFeedbacks(_, _)),
    export_c_func!(glIsTransformFeedback(_)),
    export_c_func!(glPauseTransformFeedback()),
    export_c_func!(glResumeTransformFeedback()),
    export_c_func!(glVertexAttribIPointer(_, _, _, _, _)),
    export_c_func!(glVertexAttribI4i(_, _, _, _, _)),
    export_c_func!(glVertexAttribI4ui(_, _, _, _, _)),
    export_c_func!(glUniform1ui(_, _)),
    export_c_func!(glUniform2ui(_, _, _)),
    export_c_func!(glUniform3ui(_, _, _, _)),
    export_c_func!(glUniform4ui(_, _, _, _, _)),
    export_c_func!(glUniform1uiv(_, _, _)),
    export_c_func!(glUniform2uiv(_, _, _)),
    export_c_func!(glUniform3uiv(_, _, _)),
    export_c_func!(glUniform4uiv(_, _, _)),
    export_c_func!(glUniformMatrix2x3fv(_, _, _, _)),
    export_c_func!(glUniformMatrix3x2fv(_, _, _, _)),
    export_c_func!(glUniformMatrix2x4fv(_, _, _, _)),
    export_c_func!(glUniformMatrix4x2fv(_, _, _, _)),
    export_c_func!(glUniformMatrix3x4fv(_, _, _, _)),
    export_c_func!(glUniformMatrix4x3fv(_, _, _, _)),
    export_c_func!(glGetUniformBlockIndex(_, _)),
    export_c_func!(glUniformBlockBinding(_, _, _)),
    export_c_func!(glFenceSync(_, _)),
    export_c_func!(glIsSync(_)),
    export_c_func!(glDeleteSync(_)),
    export_c_func!(glClientWaitSync(_, _, _, _)),
    export_c_func!(glWaitSync(_, _, _, _)),
    export_c_func!(glGetStringi(_, _)),
    export_c_func!(glGetFragDataLocation(_, _)),
    export_c_func!(glProgramParameteri(_, _, _)),
    // OpenGL ES 3.0 entry points whose backend implementations already exist
    // but were missing from the guest export table (caused "unhandled
    // external relocation" warnings for GL ES 3.0 apps such as Marmalade/Life).
    export_c_func!(glCompressedTexImage3D(_, _, _, _, _, _, _, _, _)),
    export_c_func!(glCompressedTexSubImage3D(_, _, _, _, _, _, _, _, _, _, _)),
    export_c_func!(glSamplerParameteriv(_, _, _)),
    export_c_func!(glSamplerParameterfv(_, _, _)),
    export_c_func!(glGetSamplerParameteriv(_, _, _)),
    export_c_func!(glGetSamplerParameterfv(_, _, _)),
    export_c_func!(glGetInteger64v(_, _)),
    export_c_func!(glGetIntegeri_v(_, _, _)),
    export_c_func!(glGetInteger64i_v(_, _, _)),
    export_c_func!(glGetBufferParameteri64v(_, _, _)),
    export_c_func!(glGetBufferPointerv(_, _, _)),
    export_c_func!(glGetInternalformativ(_, _, _, _, _)),
    export_c_func!(glVertexAttribI4iv(_, _)),
    export_c_func!(glVertexAttribI4uiv(_, _)),
    export_c_func!(glGetVertexAttribfv(_, _, _)),
    export_c_func!(glGetVertexAttribiv(_, _, _)),
    export_c_func!(glGetVertexAttribPointerv(_, _, _)),
    export_c_func!(glGetVertexAttribIiv(_, _, _)),
    export_c_func!(glGetVertexAttribIuiv(_, _, _)),
    export_c_func!(glGetUniformuiv(_, _, _)),
    export_c_func!(glGetUniformIndices(_, _, _, _)),
    export_c_func!(glTransformFeedbackVaryings(_, _, _, _)),
    export_c_func!(glGetTransformFeedbackVarying(_, _, _, _, _, _, _)),
    export_c_func!(glGetSynciv(_, _, _, _, _)),
    // GL_EXT_debug_label — debug-label extension, no-op implementations.
    // Reference: <https://registry.khronos.org/OpenGL/extensions/EXT/EXT_debug_label.txt>
    export_c_func!(glLabelObjectEXT(_, _, _, _)),
    export_c_func!(glGetObjectLabelEXT(_, _, _, _, _)),
];

#[cfg(test)]
mod shader_preprocessor_normalization_tests {
    use super::normalize_shader_preprocessor_whitespace;

    #[test]
    fn inserts_space_before_comment_on_endif() {
        let src = "#if defined(FOO)\nvoid main() {}\n#endif//comment\n";
        let out = normalize_shader_preprocessor_whitespace(src);
        assert!(out.contains("#endif //comment"));
    }

    #[test]
    fn inserts_space_before_comment_on_if() {
        let src = "#if defined(FOO)//bar\nvoid main() {}\n#endif\n";
        let out = normalize_shader_preprocessor_whitespace(src);
        assert!(out.contains("#if defined(FOO) //bar"));
    }

    #[test]
    fn strips_stray_carriage_returns() {
        let src = "#if defined(FOO)\r\nvoid main() {}\r\n#endif\r\n";
        let out = normalize_shader_preprocessor_whitespace(src);
        assert!(!out.contains('\r'));
        assert!(out.contains("#if defined(FOO)"));
        assert!(out.contains("#endif"));
    }

    #[test]
    fn leaves_already_spaced_directives_unchanged() {
        let src = "#if defined(FOO) // bar\nvoid main() {}\n#endif // baz\n";
        let out = normalize_shader_preprocessor_whitespace(src);
        assert_eq!(out, src.replace('\r', ""));
    }

    #[test]
    fn does_not_touch_comments_in_body_code() {
        let src = "void main() {\n  float x = 1.0;//no space needed here\n}\n";
        let out = normalize_shader_preprocessor_whitespace(src);
        assert_eq!(out, src);
    }

    #[test]
    fn inserts_space_before_block_comment_on_directive() {
        let src = "#endif/* trailing */\nvoid main() {}\n";
        let out = normalize_shader_preprocessor_whitespace(src);
        assert!(out.contains("#endif /* trailing */"));
    }

    #[test]
    fn normalizes_directive_spanning_concatenated_source_strings() {
        // Guest code often calls glShaderSource with count > 1, splitting the
        // source into several strings. touchHLE concatenates them before
        // normalization; verify that a directive whose trailing `//comment`
        // only becomes adjacent *after* concatenation is still fixed up.
        // Real case: Gameloft's "9mm" glues `#endif//...` at a chunk boundary,
        // which real PowerVR SGX tolerated but desktop/Adreno GLSL rejects with
        // "unexpected tokens following #endif".
        let chunk_a = "#if defined(FOO)\nvoid main() {}\n#endif";
        let chunk_b = "//trailing comment\n";
        let joined = format!("{chunk_a}{chunk_b}");
        let out = normalize_shader_preprocessor_whitespace(&joined);
        assert!(out.contains("#endif //trailing comment"));
        assert!(!out.contains("#endif//trailing comment"));
    }
}

#[cfg(test)]
mod hd_cache_tests {
    use super::*;

    fn value(bytes: &[u8], name: &str) -> HdCacheValue {
        Some((1, 1, Arc::new(bytes.to_vec()), name.to_owned()))
    }

    #[test]
    fn bounded_cache_shares_pixels_and_evicts_oldest_entry() {
        let mut cache = HdCacheState::new(4);
        let pixels = Arc::new(vec![1u8, 2]);
        cache.insert(
            "a".to_owned(),
            Some((1, 1, Arc::clone(&pixels), "a".to_owned())),
        );
        cache.insert("b".to_owned(), value(&[3, 4], "b"));
        let hit = cache.lookup("a").expect("a must be cached").expect("value");
        assert!(Arc::ptr_eq(&hit.2, &pixels));
        cache.insert("c".to_owned(), value(&[5, 6], "c"));
        assert!(cache.lookup("a").is_some());
        assert!(cache.lookup("c").is_some());
        assert!(cache.lookup("b").is_none(), "least-recently-used entry survived");
    }
}

#[cfg(test)]
mod texture_sync_tests {
    use super::should_synchronize_texture_upload;
    use crate::ds_perf_control::TextureSyncMode;

    #[test]
    fn empty_uploads_never_request_a_driver_finish() {
        for mode in [
            TextureSyncMode::Off,
            TextureSyncMode::Flush,
            TextureSyncMode::Finish,
        ] {
            assert!(!should_synchronize_texture_upload(mode, false));
        }
    }

    #[test]
    fn payload_uploads_keep_the_selected_sync_mode() {
        assert!(!should_synchronize_texture_upload(TextureSyncMode::Off, true));
        assert!(should_synchronize_texture_upload(TextureSyncMode::Flush, true));
        assert!(should_synchronize_texture_upload(TextureSyncMode::Finish, true));
    }
}

#[cfg(test)]
mod hd_replacement_marker_tests {
    use super::{ds_clear_hd_texture_replaced, ds_hd_texture_was_replaced, ds_mark_hd_texture_replaced};

    #[test]
    fn deleting_a_texture_can_release_its_replacement_marker() {
        let texture = 0xfeed_ba11;
        ds_mark_hd_texture_replaced(texture);
        assert!(ds_hd_texture_was_replaced(texture));
        ds_clear_hd_texture_replaced(texture);
        assert!(!ds_hd_texture_was_replaced(texture));
    }
}

#[cfg(test)]
mod r4_fixed_fog_boundary_tests {
    use super::*;
    struct Driver { shader:bool, enabled:bool, start:f32, end:f32, queries:u32, writes:Vec<(GLenum,f32)> }
    impl GLES for Driver {
        fn is_es2(&self)->bool {self.shader}
        unsafe fn GetBooleanv(&mut self,p:GLenum,v:*mut GLboolean) {assert!(!self.shader,"fixed-function query on shader driver");assert_eq!(p,gles11::FOG);self.queries+=1;*v=self.enabled as GLboolean;}
        unsafe fn GetFloatv(&mut self,p:GLenum,v:*mut GLfloat) {self.queries+=1;*v=match p {gles11::FOG_START=>self.start,gles11::FOG_END=>self.end,_=>panic!("unexpected query")};}
        unsafe fn Fogf(&mut self,p:GLenum,v:GLfloat) {self.writes.push((p,v));}
    }
    fn driver(shader:bool,enabled:bool,start:f32,end:f32)->Driver {Driver {shader,enabled,start,end,queries:0,writes:Vec::new()}}
    #[test]
    fn shader_draws_never_query_fixed_function_fog() {let mut g=driver(true,true,2.,2.);unsafe {assert!(clamp_fog_state_values(&mut g).is_none());}assert_eq!(g.queries,0);assert!(g.writes.is_empty());}
    #[test]
    fn fixed_function_degenerate_range_still_clamps_and_restores() {let mut g=driver(false,true,2.,2.);unsafe {let old=clamp_fog_state_values(&mut g);assert_eq!(old,Some((2.,2.)));restore_fog_state_values(&mut g,old);}assert_eq!(g.queries,3);assert_eq!(g.writes,vec![(gles11::FOG_START,1.999),(gles11::FOG_START,2.),(gles11::FOG_END,2.)]);}
    #[test]
    fn fixed_function_disabled_and_valid_ranges_keep_original_state() {for (enabled,start,end,n) in [(false,2.,2.,1),(true,1.,2.,3)] {let mut g=driver(false,enabled,start,end);unsafe {assert!(clamp_fog_state_values(&mut g).is_none());}assert_eq!(g.queries,n);assert!(g.writes.is_empty());}}
}

//! PC confirmation for the original loading hint layer. No game state writes.
use crate::{mem::ConstVoidPtr, Environment};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};

static OWNER: AtomicU32 = AtomicU32::new(0);
static FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static READY: AtomicBool = AtomicBool::new(false);
static REQUEST: AtomicU32 = AtomicU32::new(0);

pub(crate) const DRAW_PUSH: u32 = 0xe92d_40f0;

#[derive(Clone, Copy)]
struct LegacyLayout {
    draw: u32,
    vptr: u32,
    ready_offset: u32,
    draw_third_word: u32,
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn legacy_layout(env: &Environment) -> Option<LegacyLayout> {
    use crate::ds_legacy_input_profile::LegacyInputProfile;
    if !crate::window::ds_native_inputs() {
        return None;
    }
    let edition = LegacyInputProfile::identify(
        env.bundle.bundle_identifier(),
        env.bundle.bundle_version(),
        env.bins.first()?.executable_sha256,
    )?;
    let (draw, vptr, ready_offset, draw_third_word) = match edition {
        LegacyInputProfile::EarlyIpad => (0x002f_3e30, 0x007b_1f2c, 0x34, 0xe52d_8004),
        LegacyInputProfile::Iphone => (0x0024_54b8, 0x0076_4fb8, 0x34, 0xe92d_0900),
        LegacyInputProfile::World => (0x0029_b1d0, 0x006e_deb4, 0x30, 0xe52d_8004),
    };
    Some(LegacyLayout { draw, vptr, ready_offset, draw_third_word })
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn legacy_layout(_env: &Environment) -> Option<LegacyLayout> {
    None
}

fn word(env: &Environment, address: u32) -> Option<u32> {
    let b = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(address), 4)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

pub(super) fn legacy_draw_site(env: &Environment) -> Option<u32> {
    let layout = legacy_layout(env)?;
    (word(env, layout.draw) == Some(DRAW_PUSH)
        && word(env, layout.draw + 4) == Some(0xe28d_700c)
        && word(env, layout.draw + 8) == Some(layout.draw_third_word))
        .then_some(layout.draw)
}

// The native pointer-release handler in World reads +0x30. The other
// verified handlers read +0x34 before signalling the world to start.
fn owner_layout(env: &Environment, owner: u32) -> Option<(u32, bool)> {
    if !cfg!(target_os = "windows") || owner == 0 {
        return None;
    }
    let vptr = word(env, owner)?;
    if let Some(profile) = crate::ds_frontend_profile::identify(env) {
        return (vptr == profile.loading_wait_vptr()).then_some((0x34, false));
    }
    let legacy = legacy_layout(env)?;
    (vptr == legacy.vptr).then_some((legacy.ready_offset, true))
}

fn ready_byte(env: &Environment, owner: u32, offset: u32) -> bool {
    owner.checked_add(offset)
        .and_then(|address| env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(address), 1))
        .is_some_and(|b| b[0] != 0)
}

pub(super) fn reset() {
    OWNER.store(0, Relaxed);
    FRAME.store(u64::MAX, Relaxed);
    READY.store(false, Relaxed);
    cancel();
}
pub(super) fn cancel() {
    REQUEST.store(0, Relaxed);
}

pub(super) fn observe(env: &Environment, owner: u32) {
    let Some((offset, _)) = owner_layout(env, owner) else { return; };
    if OWNER.swap(owner, Relaxed) != owner {
        cancel();
    }
    FRAME.store(crate::window::DS_FRAME_COUNTER.load(Relaxed), Relaxed);
    let ready = ready_byte(env, owner, offset);
    crate::ds_diagnostics::loading(crate::window::DS_FRAME_COUNTER.load(Relaxed),ready);
    if READY.swap(ready, Relaxed) != ready {
        echo!("[DSloadingUI] owner={owner:#x} ready={ready}");
    }
}

pub(super) fn active() -> bool {
    cfg!(target_os = "windows")
        && OWNER.load(Relaxed) != 0
        && crate::window::DS_FRAME_COUNTER
            .load(Relaxed)
            .saturating_sub(FRAME.load(Relaxed))
            <= 2
}
pub(super) fn ready() -> bool {
    active() && READY.load(Relaxed)
}
pub(super) fn queue() {
    if ready() {
        REQUEST.store(OWNER.load(Relaxed), Relaxed);
    }
}

pub(super) fn tick(env: &mut Environment) {
    let owner = REQUEST.swap(0, Relaxed);
    if owner == 0
        || owner != OWNER.load(Relaxed)
        || !ready()
    {
        return;
    }
    let Some((offset, legacy)) = owner_layout(env, owner) else { return; };
    if !ready_byte(env, owner, offset) {
        return;
    }
    // Original LayerLoadingWait::OnEvent accepts PointerRelease(type 3) only
    // when ready. Send the guest touch outside the inner hint carousel.
    if let Some(window) = env.window.as_mut() {
        if legacy {
            window.ds_queue_normalized_touch("tap", 0.5, 0.975);
        } else {
            window.ds_queue_action("tap 240 300".into());
        }
        echo!("[DSloadingUI] confirm -> native release owner={owner:#x}");
    }
}

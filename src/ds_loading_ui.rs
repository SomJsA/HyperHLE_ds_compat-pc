//! PC confirmation for the original loading hint layer. No game state writes.
use crate::{mem::ConstVoidPtr, Environment};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};

static OWNER: AtomicU32 = AtomicU32::new(0);
static FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static READY: AtomicBool = AtomicBool::new(false);
static REQUEST: AtomicU32 = AtomicU32::new(0);

fn word(env: &Environment, address: u32) -> Option<u32> {
    let b = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(address), 4)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
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
    if !cfg!(target_os = "windows") || word(env, owner) != Some(0x00a8_73b8) {
        return;
    }
    if OWNER.swap(owner, Relaxed) != owner {
        cancel();
    }
    FRAME.store(crate::window::DS_FRAME_COUNTER.load(Relaxed), Relaxed);
    let ready = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(owner + 0x34), 1)
        .is_some_and(|b| b[0] != 0);
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
        || word(env, owner) != Some(0x00a8_73b8)
    {
        return;
    }
    // Original LayerLoadingWait::OnEvent accepts PointerRelease(type 3) only
    // after +0x34 becomes ready. Use the real guest pointer-event constructor
    // and dispatch route. Coordinates are outside the inner hint carousel;
    // the layer's native continuation branch itself ignores coordinates.
    // Never set readiness, pop layers, or force the world into gameplay here.
    if let Some(window) = env.window.as_mut() {
        window.ds_queue_action("tap 240 300".into());
        echo!("[DSloadingUI] confirm -> native release owner={owner:#x}");
    }
}

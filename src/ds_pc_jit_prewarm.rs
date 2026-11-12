//! Bounded translation-only prewarm for the first combat/blood path.
//! The list is generated from an exact original-IPA cold-hit trace and only
//! translates immutable guest blocks; it does not execute guest code, call HLE,
//! touch GL/audio, mutate game state, or change the native JIT cache policy.
mod addresses { include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ds_pc_jit_prewarm_addresses.rs")); }
use std::sync::{OnceLock, atomic::{AtomicBool, Ordering}};
pub(crate) fn maybe(env: &mut crate::Environment) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static DONE: AtomicBool = AtomicBool::new(false);
    if !*ENABLED.get_or_init(|| {
        cfg!(all(windows, target_arch = "x86_64")) &&
        std::env::var("TOUCHHLE_DS_PC_JIT_PREWARM").as_deref() != Ok("0")
    }) || DONE.load(Ordering::Acquire) { return; }
    let Some((_, player)) = crate::frameworks::opengles::ds_current_hud_player_public(env) else { return; };
    let state: u32 = env.mem.read(crate::mem::ConstPtr::from_bits(player + 0x208));
    if state > 18 { return; }
    let started = std::time::Instant::now();
    env.cpu.precompile_blocks(&mut env.mem, addresses::BLOCKS);
    DONE.store(true, Ordering::Release);
    echo!("[DSpc-jit-prewarm] translated {} immutable first-combat blocks before gameplay action in {:.3}ms", addresses::BLOCKS.len(), started.elapsed().as_secs_f64()*1000.0);
}

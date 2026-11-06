//! Opt-in read-only native M3GApplication update-clock observer. The original
//! STR r0,[sp] is replayed exactly; no time value or scheduling is rewritten.
use crate::{cpu::Cpu, mem::{ConstVoidPtr, MutPtr}, Environment};
use std::time::Instant;
#[path = "ds_game_clock_metrics.rs"]
mod metrics;

const SITE: u32 = 0x0053_0a84;
const STORE: u32 = 0xe58d_0000;
const BKPT: u32 = 0xe120_0070;

#[derive(Default)]
pub(crate) struct State {
    armed: bool,
    owner: u32,
    window: Option<metrics::Window>,
}

fn word(env: &Environment, address: u32) -> Option<u32> {
    let bytes = env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(address), 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

pub(crate) fn prime(env: &mut Environment) {
    if std::env::var("TOUCHHLE_DS_GAME_CLOCK_TRACE").as_deref() != Ok("1") { return; }
    if env.bundle.bundle_identifier() != "com.ea.deadspace.bv"
        || word(env, SITE) != Some(STORE)
        || word(env, SITE - 4) != Some(0xebfe_e933)
        || word(env, 0x004e_af54) != Some(0xe92d_4090) {
        echo!("[DSgame-clock] unknown original instructions; observer not armed");
        return;
    }
    env.mem.write(MutPtr::<u32>::from_bits(SITE), BKPT);
    env.cpu.invalidate_cache_range(SITE, 4);
    env.framework_state.opengles.ds_game_clock.armed = true;
    echo!("[DSgame-clock] original M3GApplication timer observer armed; deltas unchanged; diagnostic overhead present");
}

pub(crate) fn handle_breakpoint(env: &mut Environment) -> bool {
    if !env.framework_state.opengles.ds_game_clock.armed || env.cpu.regs()[Cpu::PC] != SITE { return false; }
    let regs = *env.cpu.regs();
    let delta = regs[0] as i32;
    let owner = regs[4];
    // Replay the original store, preserving all registers and flags apart
    // from advancing PC to the next original instruction.
    env.mem.write(MutPtr::<u32>::from_bits(regs[Cpu::SP]), regs[0]);
    env.cpu.regs_mut()[Cpu::PC] = SITE + 4;
    let now = Instant::now();
    let state = &mut env.framework_state.opengles.ds_game_clock;
    if state.owner != owner || state.window.is_none() {
        state.owner = owner;
        state.window = Some(metrics::Window::new(now));
        return true; // First delta predates our observation window.
    }
    if let Some(sample) = state.window.as_mut().unwrap().push(delta, now) {
        echo!("[DSgame-clock] owner={owner:#x} wall_ms={:.3} native_ms={} ticks={} ratio={:.6} min_ms={} max_ms={} invalid={}",
            sample.wall_ms, sample.native_ms, sample.ticks,
            sample.native_ms as f64 / sample.wall_ms,
            sample.min, sample.max, sample.invalid);
    }
    true
}

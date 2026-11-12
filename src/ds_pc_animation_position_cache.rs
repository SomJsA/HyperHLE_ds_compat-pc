//! Explicit opt-in memoization for the original AnimationController::GetPosition.
//! The candidate replays only an exact object-state/time/FPSCR match and keeps
//! the original ARM routine as the miss and fallback path.
use crate::{
    abi::GuestFunction,
    cpu::Cpu,
    mem::{ConstVoidPtr, MutPtr},
    Environment,
};
use std::{path::PathBuf, sync::atomic::Ordering::Relaxed};

const ENTRY: u32 = 0x414bf8;
const FIRST_WORD: u32 = 0xe79f4003;
const BKPT: u32 = 0xe1200070;
const FINGERPRINT: u64 = 903989323754398323;
const STATE_OFFSET: u32 = 0x4c;
const STATE_BYTES: u32 = 12;
const FPSCR_STICKY: u32 = 0x9f;

#[derive(Clone, Copy)]
struct Cache {
    object: u32,
    time_bits: u32,
    state: [u8; STATE_BYTES as usize],
    control: u32,
    result: u32,
    regs: [u32; 3],
    flags: u32,
    extregs: [u32; 16],
}

#[derive(Default)]
pub(crate) struct State {
    configured: bool,
    trigger: Option<PathBuf>,
    last_frame: u64,
    armed: bool,
    disabled: bool,
    calls: u64,
    hits: u64,
    misses: u64,
    cache: Option<Cache>,
}

#[derive(Clone, Copy)]
struct Args {
    object: u32,
    time_bits: u32,
    state: [u8; STATE_BYTES as usize],
    control: u32,
}

fn bytes(env: &Environment, at: u32, len: u32) -> Option<&[u8]> {
    if at < 0x1000 || u64::from(at) + u64::from(len) > (1u64 << 32) {
        return None;
    }
    let data = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(at), len)?;
    (data.len() == len as usize).then_some(data)
}

fn word(env: &Environment) -> Option<u32> {
    Some(u32::from_le_bytes(bytes(env, ENTRY, 4)?.try_into().ok()?))
}

fn fingerprint(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

fn arm(env: &mut Environment) -> bool {
    let s = &env.framework_state.opengles.ds_pc_animation_position_cache;
    if s.disabled || !s.configured {
        return false;
    }
    if s.armed {
        return true;
    }
    if fingerprint(bytes(env, ENTRY, 40).unwrap_or(&[])) != FINGERPRINT
        || word(env) != Some(FIRST_WORD)
    {
        echo!("[DSpc-animation-position-cache] unknown original code; no hook installed");
        return false;
    }
    env.mem.write(MutPtr::<u32>::from_bits(ENTRY), BKPT);
    env.cpu.invalidate_cache_range(ENTRY, 4);
    env.framework_state
        .opengles
        .ds_pc_animation_position_cache
        .armed = true;
    true
}

fn restore(env: &mut Environment) {
    if word(env) == Some(BKPT) {
        env.mem.write(MutPtr::<u32>::from_bits(ENTRY), FIRST_WORD);
    } else {
        echo!("[DSpc-animation-position-cache] restore conflict; unknown word retained");
    }
    env.cpu.invalidate_cache_range(ENTRY, 4);
    env.framework_state
        .opengles
        .ds_pc_animation_position_cache
        .armed = false;
}

pub(crate) fn prime(env: &mut Environment) {
    if env.bundle.bundle_identifier() != "com.ea.deadspace.bv" {
        return;
    }
    if std::env::var("TOUCHHLE_DS_PC_ANIMATION_POSITION_CACHE").as_deref() != Ok("1") {
        return;
    }
    let Some(path) = std::env::var_os("TOUCHHLE_DS_PC_ANIMATION_POSITION_TRIGGER") else {
        echo!("[DSpc-animation-position-cache] disabled: explicit trigger required");
        return;
    };
    let s = &mut env.framework_state.opengles.ds_pc_animation_position_cache;
    s.configured = true;
    s.trigger = Some(PathBuf::from(path));
    echo!("[DSpc-animation-position-cache] waiting explicit trigger; exact state/time replay");
}

pub(crate) fn poll(env: &mut Environment) {
    if !env
        .framework_state
        .opengles
        .ds_pc_animation_position_cache
        .configured
    {
        return;
    }
    let frame = crate::window::DS_FRAME_COUNTER.load(Relaxed);
    let s = &mut env.framework_state.opengles.ds_pc_animation_position_cache;
    if s.last_frame == frame || s.trigger.is_none() {
        return;
    }
    s.last_frame = frame;
    if !s.trigger.as_ref().unwrap().is_file() {
        return;
    }
    s.trigger = None;
    if arm(env) {
        echo!(
            "[DSpc-animation-position-cache] armed frame={}; exact memoization only",
            frame
        );
    }
}

fn args(env: &Environment, before_fp: u32) -> Option<Args> {
    let regs = env.cpu.regs();
    let state_at = regs[0].checked_add(STATE_OFFSET)?;
    Some(Args {
        object: regs[0],
        time_bits: regs[1],
        state: bytes(env, state_at, STATE_BYTES)?.try_into().ok()?,
        control: before_fp & !FPSCR_STICKY,
    })
}

fn matches(cache: Cache, args: &Args) -> bool {
    cache.object == args.object
        && cache.time_bits == args.time_bits
        && cache.state == args.state
        && cache.control == args.control
}

fn report(env: &mut Environment) {
    let s = &mut env.framework_state.opengles.ds_pc_animation_position_cache;
    if s.calls <= 4 || s.calls.is_power_of_two() {
        echo!(
            "[DSpc-animation-position-cache] frame={} calls={} hits={} misses={}; exact replay counts, not CPU time",
            crate::window::DS_FRAME_COUNTER.load(Relaxed),
            s.calls,
            s.hits,
            s.misses
        );
    }
}

fn run_original(env: &mut Environment, ret: GuestFunction) {
    restore(env);
    GuestFunction::from_addr_and_thumb_flag(ENTRY, false).call_without_pushing_stack_frame(env);
    if !arm(env) {
        env.framework_state
            .opengles
            .ds_pc_animation_position_cache
            .disabled = true;
    }
    env.cpu.branch(ret);
}

pub(crate) fn handle_breakpoint(env: &mut Environment) -> bool {
    if !env
        .framework_state
        .opengles
        .ds_pc_animation_position_cache
        .armed
        || env.cpu.regs()[Cpu::PC] != ENTRY
        || word(env) != Some(BKPT)
    {
        return false;
    }
    if env.cpu.cpsr() & ((1 << 9) | Cpu::CPSR_THUMB) != 0 {
        restore(env);
        env.framework_state
            .opengles
            .ds_pc_animation_position_cache
            .disabled = true;
        env.cpu
            .branch(GuestFunction::from_addr_and_thumb_flag(ENTRY, false));
        return true;
    }
    let before_fp = env.cpu.fpscr();
    let ret = GuestFunction::from_addr_with_thumb_bit(env.cpu.regs()[Cpu::LR]);
    let Some(args) = args(env, before_fp) else {
        run_original(env, ret);
        return true;
    };
    let cached = env
        .framework_state
        .opengles
        .ds_pc_animation_position_cache
        .cache;
    if cached.is_some_and(|entry| matches(entry, &args)) {
        let entry = cached.unwrap();
        let regs = env.cpu.regs_mut();
        regs[0] = entry.result;
        regs[1..4].copy_from_slice(&entry.regs);
        env.cpu.extregs_mut()[..16].copy_from_slice(&entry.extregs);
        env.cpu.set_fpscr(before_fp | entry.flags);
        env.cpu.branch(ret);
        let s = &mut env.framework_state.opengles.ds_pc_animation_position_cache;
        s.calls += 1;
        s.hits += 1;
        report(env);
        return true;
    }

    run_original(env, ret);
    let regs = {
        let regs = env.cpu.regs();
        [regs[1], regs[2], regs[3]]
    };
    let result = env.cpu.regs()[0];
    let after_fp = env.cpu.fpscr();
    let extregs = {
        let ext = env.cpu.extregs_mut();
        ext[..16].try_into().unwrap()
    };
    env.framework_state
        .opengles
        .ds_pc_animation_position_cache
        .cache = Some(Cache {
        object: args.object,
        time_bits: args.time_bits,
        state: args.state,
        control: args.control,
        result,
        regs,
        flags: after_fp & FPSCR_STICKY,
        extregs,
    });
    let s = &mut env.framework_state.opengles.ds_pc_animation_position_cache;
    s.calls += 1;
    s.misses += 1;
    report(env);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_requires_object_time_state_and_control() {
        let state = [7u8; STATE_BYTES as usize];
        let args = Args {
            object: 0x1000,
            time_bits: 42,
            state,
            control: 0x4000_0000,
        };
        let entry = Cache {
            object: args.object,
            time_bits: args.time_bits,
            state,
            control: args.control,
            result: 0x3f80_0000,
            regs: [1, 2, 3],
            flags: 0x20,
            extregs: [9; 16],
        };
        assert!(matches(entry, &args));
        let mut changed = args;
        changed.time_bits ^= 1;
        assert!(!matches(entry, &changed));
        changed = args;
        changed.state[11] ^= 1;
        assert!(!matches(entry, &changed));
        changed = args;
        changed.control = 0;
        assert!(!matches(entry, &changed));
    }
}

//! Bounded entry-frequency probe, never an alternate skinning implementation.
//! Only the fingerprinted first ARM PUSH is replayed; original body executes.
use crate::{
    abi::GuestFunction,
    cpu::Cpu,
    mem::{ConstVoidPtr, Mem, MutPtr},
    Environment,
};
use std::{path::PathBuf, sync::atomic::Ordering::Relaxed, time::Instant};
const ENTRIES: [u32; 2] = [0x0037_f814, 0x0037_fac8];
const PUSH: u32 = 0xe92d_40f0;
const BKPT: u32 = 0xe120_0070;
const LIMIT: u64 = 64;
#[derive(Default)]
pub(crate) struct State {
    configured: bool,
    trigger: Option<PathBuf>,
    last_checked: u64,
    armed: [bool; 2],
    calls: [u64; 2],
    requested: [u64; 2],
    first_frame: [Option<u64>; 2],
    last_frame: [u64; 2],
    started: Option<Instant>,
}
fn region(env: &Environment, at: u32, len: u32) -> Option<&[u8]> {
    let bytes = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(at), len)?;
    (bytes.len() == len as usize).then_some(bytes)
}
fn word(env: &Environment, at: u32) -> Option<u32> {
    Some(u32::from_le_bytes(region(env, at, 4)?.try_into().ok()?))
}
fn fingerprint(env: &Environment, at: u32, len: u32) -> Option<u64> {
    Some(
        region(env, at, len)?
            .iter()
            .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
            }),
    )
}
pub(crate) fn prime(env: &mut Environment) {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64"))
        || std::env::var("TOUCHHLE_DS_GUEST_SKIN_PROBE").as_deref() != Ok("1")
    {
        return;
    }
    let Some(path) = std::env::var_os("TOUCHHLE_DS_GUEST_SKIN_PROBE_TRIGGER") else {
        echo!("[DSguest-skin-probe] disabled: explicit trigger path required");
        return;
    };
    if env.bundle.bundle_identifier() != "com.ea.deadspace.bv" {
        return;
    }
    let state = &mut env.framework_state.opengles.ds_guest_skin_probe;
    state.configured = true;
    state.trigger = Some(PathBuf::from(path));
    echo!("[DSguest-skin-probe] waiting for explicit trigger;64calls/entry maximum; original arithmetic retained");
}
pub(crate) fn poll(env: &mut Environment) {
    let state = &mut env.framework_state.opengles.ds_guest_skin_probe;
    if !state.configured || state.trigger.is_none() {
        return;
    }
    let frame = crate::window::DS_FRAME_COUNTER.load(Relaxed);
    if state.last_checked == frame {
        return;
    }
    state.last_checked = frame;
    if !state.trigger.as_ref().unwrap().is_file() {
        return;
    }
    state.trigger = None;
    let known = fingerprint(env, ENTRIES[0], 692) == Some(55_936_553_603_488_786)
        && fingerprint(env, ENTRIES[1], 716) == Some(10_538_268_167_844_965_500)
        && fingerprint(env, 0x0053_d1a4, 16) == Some(5_222_870_166_602_214_087)
        && ENTRIES.iter().all(|at| word(env, *at) == Some(PUSH));
    if !known {
        env.framework_state.opengles.ds_guest_skin_probe.configured = false;
        echo!("[DSguest-skin-probe] unknown instructions; no patch installed");
        return;
    }
    for at in ENTRIES {
        env.mem.write(MutPtr::<u32>::from_bits(at), BKPT);
        env.cpu.invalidate_cache_range(at, 4);
    }
    let state = &mut env.framework_state.opengles.ds_guest_skin_probe;
    state.armed = [true; 2];
    state.started = Some(Instant::now());
    echo!(
        "[DSguest-skin-probe] armed frame={} fingerprinted2entries; no arithmetic bypass",
        frame
    );
}
fn push_plan(regs: &[u32; 16]) -> Option<(u32, [u8; 20])> {
    let sp = regs[Cpu::SP].checked_sub(20)?;
    if sp & 3 != 0 {
        return None;
    }
    let mut bytes = [0u8; 20];
    for (i, r) in [4, 5, 6, 7, Cpu::LR].into_iter().enumerate() {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&regs[r].to_le_bytes());
    }
    Some((sp, bytes))
}
pub(crate) fn replay_push(cpu: &mut Cpu, mem: &mut Mem, entry: u32) -> bool {
    // BE data or Thumb execution do not match this exact known ARM prologue.
    if cpu.cpsr() & ((1 << 9) | Cpu::CPSR_THUMB) != 0 {
        return false;
    }
    let Some((sp, bytes)) = push_plan(cpu.regs()) else {
        return false;
    };
    let Some(dest) = mem.get_bytes_fallible_mut(ConstVoidPtr::from_bits(sp), 20) else {
        return false;
    };
    if dest.len() != 20 {
        return false;
    }
    dest.copy_from_slice(&bytes);
    cpu.regs_mut()[Cpu::SP] = sp;
    cpu.branch(GuestFunction::from_addr_and_thumb_flag(entry + 4, false));
    true
}
fn restore(env: &mut Environment, kind: usize) -> bool {
    let at = ENTRIES[kind];
    if word(env, at) != Some(BKPT) {
        echo!(
            "[DSguest-skin-probe] restore-conflict kind={}; unknown word retained",
            kind
        );
        // Invalidate stale breakpoint code, but retain the unknown word.
        env.cpu.invalidate_cache_range(at, 4);
        env.framework_state.opengles.ds_guest_skin_probe.armed[kind] = false;
        return false;
    }
    env.mem.write(MutPtr::<u32>::from_bits(at), PUSH);
    env.cpu.invalidate_cache_range(at, 4);
    env.framework_state.opengles.ds_guest_skin_probe.armed[kind] = false;
    true
}
pub(crate) fn handle_breakpoint(env: &mut Environment) -> bool {
    let state = &env.framework_state.opengles.ds_guest_skin_probe;
    if !state.configured {
        return false;
    }
    let Some(kind) = ENTRIES.iter().position(|at| *at == env.cpu.regs()[Cpu::PC]) else {
        return false;
    };
    if !state.armed[kind] || word(env, ENTRIES[kind]) != Some(BKPT) {
        return false;
    }
    let args = [env.cpu.regs()[1], env.cpu.regs()[2]];
    if !replay_push(&mut env.cpu, &mut env.mem, ENTRIES[kind]) {
        // Resume the actual word even if an external change prevented restore.
        restore(env, kind);
        env.cpu.branch(GuestFunction::from_addr_and_thumb_flag(
            ENTRIES[kind],
            false,
        ));
        echo!(
            "[DSguest-skin-probe] native fallback kind={} unsupported stack/CPU state",
            kind
        );
        return true;
    }
    let frame = crate::window::DS_FRAME_COUNTER.load(Relaxed);
    let state = &mut env.framework_state.opengles.ds_guest_skin_probe;
    state.calls[kind] += 1;
    state.requested[kind] = state.requested[kind].saturating_add(u64::from(args[1]));
    state.first_frame[kind].get_or_insert(frame);
    state.last_frame[kind] = frame;
    let n = state.calls[kind];
    if n <= 8 || n % 16 == 0 {
        echo!("[DSguest-skin-probe] sample kind={} calls={} frame={} first_frame={} r1={} r2={} requested_sum={} elapsed_us={}",kind,n,frame,state.first_frame[kind].unwrap(),args[0],args[1],state.requested[kind],state.started.unwrap().elapsed().as_micros());
    }
    // PUSH already consumed SP/PC: never tell the dispatcher "unhandled" here.
    if n >= LIMIT && restore(env, kind) {
        echo!("[DSguest-skin-probe] restored kind={} after={}calls; subsequent execution unobserved/native",kind,n);
    }
    true
}

#[cfg(all(test, windows, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use crate::cpu::{CpuError, CpuState};
    fn execute(probed: bool, thumb_return: bool) -> ([u32; 16], u32, Vec<u8>) {
        let mut mem = Mem::new();
        mem.set_null_segment_size(0x1000);
        let code = [
            if probed { BKPT } else { PUSH },
            0xe280_0003,
            0xe3a0_4063,
            0xe3a0_704d,
            0xe8bd_80f0,
        ];
        for (i, op) in code.into_iter().enumerate() {
            mem.write(MutPtr::<u32>::from_bits(0x2000 + i as u32 * 4), op);
        }
        if thumb_return {
            mem.write(MutPtr::<u16>::from_bits(0x2100), 0xdf01);
        } else {
            mem.write(MutPtr::<u32>::from_bits(0x2100), 0xef00_0001);
        }
        let mut cpu = Cpu::new(Some(&mut mem));
        cpu.set_cpsr(Cpu::CPSR_USER_MODE | 0xa800_0000);
        for (i, r) in cpu.regs_mut().iter_mut().enumerate() {
            *r = 0x1020_3000 + i as u32 * 0x100;
        }
        cpu.regs_mut()[Cpu::SP] = 0x9000;
        cpu.regs_mut()[Cpu::LR] = 0x2100 | thumb_return as u32;
        cpu.regs_mut()[Cpu::PC] = 0x2000;
        let mut ticks = 1_000_000;
        if probed {
            assert!(matches!(
                cpu.run_or_step(&mut mem, Some(&mut ticks)),
                CpuState::Error(CpuError::Breakpoint)
            ));
            assert_eq!(cpu.regs()[Cpu::PC], 0x2004);
            cpu.regs_mut()[Cpu::PC] -= 4; // Environment::debug_cpu_error does this.
            assert!(replay_push(&mut cpu, &mut mem, 0x2000));
        }
        assert!(matches!(
            cpu.run_or_step(&mut mem, Some(&mut ticks)),
            CpuState::Svc(1)
        ));
        let stack = mem
            .get_bytes_fallible(ConstVoidPtr::from_bits(0x9000 - 20), 20)
            .unwrap()
            .to_vec();
        (*cpu.regs(), cpu.cpsr(), stack)
    }
    #[test]
    fn guest_probe_actual_jit_push_replay_matches_native_regs_flags_stack_arm_and_thumb_return() {
        for thumb in [false, true] {
            assert_eq!(execute(false, thumb), execute(true, thumb));
        }
    }
    #[test]
    fn guest_probe_stack_plan_rejects_underflow_and_misalignment_and_keeps_lr_thumb_bit() {
        let mut r = [0u32; 16];
        r[Cpu::SP] = 16;
        assert!(push_plan(&r).is_none());
        r[Cpu::SP] = 0x9002;
        assert!(push_plan(&r).is_none());
        r[Cpu::SP] = 0x9000;
        r[Cpu::LR] = 0x1234_5679;
        let (sp, b) = push_plan(&r).unwrap();
        assert_eq!(sp, 0x9000 - 20);
        assert_eq!(&b[16..], &0x1234_5679u32.to_le_bytes());
        assert_eq!(r[Cpu::SP], 0x9000);
    }
}

//! Opt-in PC position-skin validation and bounded host acceleration.
use crate::{
    abi::GuestFunction,
    cpu::Cpu,
    mem::{ConstVoidPtr, MutPtr},
    Environment,
};
use std::{
    path::PathBuf,
    sync::atomic::Ordering::Relaxed,
    time::{Duration, Instant},
};
#[path = "ds_pc_skin_bounds.rs"]
mod bounds;
use bounds::{overlap, span};
const ENTRY: u32 = 0x37fac8;
const BKPT: u32 = 0xe1200070;
const PUSH: u32 = 0xe92d40f0;
const MAX_VERTICES: u32 = 32768;
#[derive(Default)]
struct Scratch {
    positions: Vec<f32>,
    weights: Vec<f32>,
    indices: Vec<i8>,
    matrices: Vec<f32>,
    output: Vec<f32>,
    output_bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Candidate {
    dest: u32,
    bytes_len: usize,
    flags: u32,
}
#[derive(Default)]
pub(crate) struct State {
    configured: bool,
    disabled: bool,
    strided: bool,
    mode_compared: u64,
    until_recheck: u16,
    host_calls: u64,
    trigger: Option<PathBuf>,
    last_checked: u64,
    armed: bool,
    calls: u64,
    compared: u64,
    mismatch: u64,
    unsupported: u64,
    vertices: u64,
    capture_ns: u128,
    candidate_ns: u128,
    original_ns: u128,
    handler_ns: u128,
    accelerate: bool,
    accelerated: u64,
    last_report: Option<Instant>,
    // Packed host-skin capture reuses these buffers between calls. The
    // strided path already borrows guest memory directly and does not use it.
    scratch: Scratch,
}
fn bytes(env: &Environment, at: u32, len: usize) -> Option<&[u8]> {
    if at < 0x1000 || len == 0 || u64::from(at).checked_add(len as u64)? > 1u64 << 32 {
        return None;
    }
    let s = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(at), len.try_into().ok()?)?;
    (s.len() == len).then_some(s)
}
fn word(env: &Environment, at: u32) -> Option<u32> {
    Some(u32::from_le_bytes(bytes(env, at, 4)?.try_into().ok()?))
}
fn fingerprint(env: &Environment, at: u32, len: usize) -> Option<u64> {
    Some(
        bytes(env, at, len)?
            .iter()
            .fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ *b as u64).wrapping_mul(0x100000001b3)
            }),
    )
}
pub(crate) fn prime(env: &mut Environment) {
    let shadow = std::env::var("TOUCHHLE_DS_PC_SKIN_SHADOW").as_deref() == Ok("1");
    let host = std::env::var("TOUCHHLE_DS_PC_SKIN_MATH").as_deref() == Ok("host");
    if env.bundle.bundle_identifier() != "com.ea.deadspace.bv" || (!shadow && !host) {
        return;
    }
    if std::env::var("TOUCHHLE_DS_GUEST_SKIN_PROBE").as_deref() == Ok("1") {
        echo!("[DSpc-skin-shadow] disabled: entry probe already owns prologue");
        return;
    }
    let Some(path) = std::env::var_os("TOUCHHLE_DS_PC_SKIN_TRIGGER") else {
        echo!("[DSpc-skin-shadow] disabled: explicit trigger required");
        return;
    };
    env.framework_state.opengles.ds_pc_skin_shadow.trigger = Some(PathBuf::from(path));
    env.framework_state.opengles.ds_pc_skin_shadow.accelerate = host;
    env.framework_state.opengles.ds_pc_skin_shadow.configured = true;
    env.framework_state.opengles.ds_pc_skin_shadow.strided =
        std::env::var("TOUCHHLE_DS_PC_SKIN_INPUT").as_deref() == Ok("strided");
    echo!(
        "[DSpc-skin-shadow] waiting explicit trigger; mode={}",
        if host {
            "host-after64-matches"
        } else {
            "shadow-only"
        }
    );
}
fn arm(env: &mut Environment) -> bool {
    let s = &env.framework_state.opengles.ds_pc_skin_shadow;
    if !s.configured || s.disabled {
        return false;
    }
    if env.framework_state.opengles.ds_pc_skin_shadow.armed {
        return true;
    }
    if fingerprint(env, ENTRY, 716) != Some(10538268167844965500)
        || fingerprint(env, 0x53d1a4, 16) != Some(5222870166602214087)
        || word(env, ENTRY) != Some(PUSH)
    {
        echo!("[DSpc-skin-shadow] unknown code; no patch installed");
        return false;
    }
    env.mem.write(MutPtr::<u32>::from_bits(ENTRY), BKPT);
    env.cpu.invalidate_cache_range(ENTRY, 4);
    env.framework_state.opengles.ds_pc_skin_shadow.armed = true;
    true
}
pub(crate) fn set_acceleration(env: &mut Environment, enabled: bool) {
    if !env.framework_state.opengles.ds_pc_skin_shadow.configured {
        return;
    }
    env.framework_state.opengles.ds_pc_skin_shadow.trigger = None;
    if enabled {
        env.framework_state.opengles.ds_pc_skin_shadow.accelerate = true;
        if !arm(env) {
            env.framework_state.opengles.ds_pc_skin_shadow.accelerate = false;
            return;
        }
        echo!("[DSpc-skin-shadow] runtime host acceleration ON");
    } else {
        if env.framework_state.opengles.ds_pc_skin_shadow.armed {
            restore(env);
        }
        env.framework_state.opengles.ds_pc_skin_shadow.accelerate = false;
        echo!("[DSpc-skin-shadow] runtime host acceleration OFF; original entry restored");
    }
}
pub(crate) fn poll(env: &mut Environment) {
    let input_request = crate::window::DS_PC_SKIN_INPUT_REQUEST.swap(-1, Relaxed);
    if input_request >= 0 {
        let s = &mut env.framework_state.opengles.ds_pc_skin_shadow;
        if s.configured && !s.disabled {
            s.strided = input_request == 1;
            // Every input-mode request starts its own original64 verification.
            // Never reuse a packed mode's matches to enable strided output.
            s.mode_compared = 0;
            s.until_recheck = 0;
            echo!(
                "[DSpc-skin-shadow] runtime input={}; original64 warmup reset",
                if s.strided { "strided" } else { "packed" }
            );
        }
    }
    let request = crate::window::DS_PC_SKIN_HOST_REQUEST.swap(-1, Relaxed);
    if request >= 0 {
        set_acceleration(env, request == 1);
    }
    let s = &mut env.framework_state.opengles.ds_pc_skin_shadow;
    if s.trigger.is_none() {
        return;
    }
    let frame = crate::window::DS_FRAME_COUNTER.load(Relaxed);
    if s.last_checked == frame {
        return;
    }
    s.last_checked = frame;
    if !s.trigger.as_ref().unwrap().is_file() {
        return;
    }
    s.trigger = None;
    if !arm(env) {
        return;
    }
    echo!(
        "[DSpc-skin-shadow] armed frame={}; mode={}; bounded32768 vertices",
        frame,
        if env.framework_state.opengles.ds_pc_skin_shadow.accelerate {
            "host-after64-matches-recheck256"
        } else {
            "shadow-only"
        }
    );
}
struct Array {
    at: u32,
    stride: usize,
}
fn array(
    env: &Environment,
    global: u32,
    components: u32,
    size: usize,
    max_stride: usize,
) -> Result<Array, &'static str> {
    let obj = word(env, global).ok_or("array-global")?;
    let actual =
        word(env, obj.checked_add(68).ok_or("object-overflow")?).ok_or("array-components")?;
    let base = word(env, obj.checked_add(76).ok_or("object-overflow")?).ok_or("array-base")?;
    let offset = word(env, obj.checked_add(84).ok_or("object-overflow")?).ok_or("array-offset")?;
    let stride =
        word(env, obj.checked_add(88).ok_or("object-overflow")?).ok_or("array-stride")? as usize;
    if actual != components || stride < size || stride > max_stride || (size > 4 && stride % 4 != 0)
    {
        return Err("unsupported-array-layout");
    }
    Ok(Array {
        at: span(base, u64::from(offset), size)?,
        stride,
    })
}
struct Snapshot {
    dest: u32,
    count: u32,
    positions: Vec<f32>,
    weights: Vec<f32>,
    indices: Vec<i8>,
    matrices: Vec<f32>,
}
fn floats(slice: &[u8], stride: usize, components: usize, count: usize, result: &mut Vec<f32>) {
    result.clear();
    result.reserve(count * components);
    for i in 0..count {
        for c in 0..components {
            let at = i * stride + c * 4;
            result.push(f32::from_le_bytes(slice[at..at + 4].try_into().unwrap()));
        }
    }
}
fn capture(
    env: &Environment,
    args: [u32; 4],
    scratch: &mut Scratch,
) -> Result<Snapshot, &'static str> {
    let [dest, first, count, _] = args;
    if count == 0 || count > MAX_VERTICES || first > 1_048_576 {
        return Err("vertex-range");
    }
    let pos = array(env, 0x00ab_b844, 3, 12, 64)?;
    let idx = array(env, 0x00ab_b848, 4, 4, 16)?;
    let wgt = array(env, 0x00ab_b84c, 4, 16, 64)?;
    let range = |a: &Array, size: usize| -> Result<(u32, usize), &'static str> {
        let len = (count as usize - 1) * a.stride + size;
        span(a.at, u64::from(first) * a.stride as u64, len).map(|at| (at, len))
    };
    let pr = range(&pos, 12)?;
    let ir = range(&idx, 4)?;
    let wr = range(&wgt, 16)?;
    let destination = (
        span(dest, u64::from(first) * 12, count as usize * 12)?,
        count as usize * 12,
    );
    if [pr, ir, wr].iter().any(|r| overlap(destination, *r)) {
        return Err("aliased-output");
    }
    floats(
        bytes(env, pr.0, pr.1).ok_or("positions-unreadable")?,
        pos.stride,
        3,
        count as usize,
        &mut scratch.positions,
    );
    floats(
        bytes(env, wr.0, wr.1).ok_or("weights-unreadable")?,
        wgt.stride,
        4,
        count as usize,
        &mut scratch.weights,
    );
    let raw_indices = bytes(env, ir.0, ir.1).ok_or("indices-unreadable")?;
    scratch.indices.clear();
    scratch.indices.reserve(count as usize * 4);
    let mut max_index = 0i8;
    for i in 0..count as usize {
        let mut active = true;
        for b in 0..4 {
            let w = scratch.weights[i * 4 + b];
            if b != 0 && w.to_bits() & 0x7000_0000 == 0 {
                active = false;
            }
            let index = raw_indices[i * idx.stride + b] as i8;
            if active {
                if index < 0 || !w.is_finite() {
                    return Err("unsupported-weight-index");
                }
                max_index = max_index.max(index);
            }
            scratch.indices.push(index);
        }
    }
    if scratch.positions.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite-position");
    }
    let holder = word(env, 0x00ab_b83c).ok_or("palette-holder")?;
    let matrix_base =
        word(env, holder.checked_add(8).ok_or("palette-overflow")?).ok_or("palette-data")?;
    let mr = (
        span(matrix_base, 0, (max_index as usize + 1) * 64)?,
        (max_index as usize + 1) * 64,
    );
    if overlap(destination, mr) {
        return Err("aliased-palette");
    }
    floats(
        bytes(env, mr.0, mr.1).ok_or("palette-unreadable")?,
        64,
        16,
        max_index as usize + 1,
        &mut scratch.matrices,
    );
    if scratch.matrices.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite-palette");
    }
    bytes(env, destination.0, destination.1).ok_or("destination-unreadable")?;
    Ok(Snapshot {
        dest: destination.0,
        count,
        positions: std::mem::take(&mut scratch.positions),
        weights: std::mem::take(&mut scratch.weights),
        indices: std::mem::take(&mut scratch.indices),
        matrices: std::mem::take(&mut scratch.matrices),
    })
}

impl Scratch {
    fn reclaim(&mut self, snapshot: Snapshot) {
        self.positions = snapshot.positions;
        self.weights = snapshot.weights;
        self.indices = snapshot.indices;
        self.matrices = snapshot.matrices;
    }
}

fn capture_strided(
    env: &Environment,
    args: [u32; 4],
) -> Result<(u32, crate::ds_pc_skin_math::strided::Inputs<'_>), &'static str> {
    use crate::ds_pc_skin_math::strided::{palette_matrices, Inputs};
    let [dest, first, count, _] = args;
    if count == 0 || count > MAX_VERTICES || first > 1_048_576 {
        return Err("vertex-range");
    }
    let pos = array(env, 0x00ab_b844, 3, 12, 64)?;
    let idx = array(env, 0x00ab_b848, 4, 4, 16)?;
    let wgt = array(env, 0x00ab_b84c, 4, 16, 64)?;
    let range = |a: &Array, size: usize| -> Result<(u32, usize), &'static str> {
        let len = (count as usize - 1) * a.stride + size;
        span(a.at, u64::from(first) * a.stride as u64, len).map(|at| (at, len))
    };
    let pr = range(&pos, 12)?;
    let ir = range(&idx, 4)?;
    let wr = range(&wgt, 16)?;
    let destination = (
        span(dest, u64::from(first) * 12, count as usize * 12)?,
        count as usize * 12,
    );
    if [pr, ir, wr].iter().any(|r| overlap(destination, *r)) {
        return Err("aliased-output");
    }
    let positions = bytes(env, pr.0, pr.1).ok_or("positions-unreadable")?;
    let weights = bytes(env, wr.0, wr.1).ok_or("weights-unreadable")?;
    let indices = bytes(env, ir.0, ir.1).ok_or("indices-unreadable")?;
    let matrices = palette_matrices(weights, wgt.stride, indices, idx.stride, count)?;
    let holder = word(env, 0x00ab_b83c).ok_or("palette-holder")?;
    let matrix_base =
        word(env, holder.checked_add(8).ok_or("palette-overflow")?).ok_or("palette-data")?;
    let mr = (span(matrix_base, 0, matrices * 64)?, matrices * 64);
    if overlap(destination, mr) {
        return Err("aliased-palette");
    }
    let matrices = bytes(env, mr.0, mr.1).ok_or("palette-unreadable")?;
    bytes(env, destination.0, destination.1).ok_or("destination-unreadable")?;
    let input = Inputs {
        positions,
        position_stride: pos.stride,
        weights,
        weight_stride: wgt.stride,
        indices,
        index_stride: idx.stride,
        matrices,
        count,
    };
    crate::ds_pc_skin_math::strided::validate_floats(&input)?;
    Ok((destination.0, input))
}

// All borrowed guest views end before original execution or output commit.
fn evaluate(
    env: &Environment,
    args: [u32; 4],
    strided: bool,
    scratch: &mut Scratch,
) -> (Result<Candidate, &'static str>, u128, u128) {
    let t = Instant::now();
    if strided {
        let snapshot = capture_strided(env, args);
        let capture_ns = t.elapsed().as_nanos();
        let t = Instant::now();
        let candidate = snapshot.and_then(|(dest, input)| {
            crate::ds_pc_skin_math::strided::calculate_validated_into_reuse(
                &input,
                &mut scratch.output,
                &mut scratch.output_bytes,
            )
            .map(|output| Candidate {
                dest,
                bytes_len: output.bytes.len(),
                flags: output.flags,
            })
        });
        (candidate, capture_ns, t.elapsed().as_nanos())
    } else {
        let snapshot = capture(env, args, scratch);
        let capture_ns = t.elapsed().as_nanos();
        let t = Instant::now();
        let candidate = match snapshot {
            Ok(snapshot) => {
                let result = crate::ds_pc_skin_math::calculate_validated_into_reuse(
                    &snapshot.positions,
                    &snapshot.weights,
                    &snapshot.indices,
                    &snapshot.matrices,
                    snapshot.count,
                    &mut scratch.output,
                    &mut scratch.output_bytes,
                )
                .map(|output| Candidate {
                    dest: snapshot.dest,
                    bytes_len: output.bytes.len(),
                    flags: output.flags,
                });
                scratch.reclaim(snapshot);
                result
            }
            Err(error) => Err(error),
        };
        (candidate, capture_ns, t.elapsed().as_nanos())
    }
}

fn original_after_push(env: &mut Environment) -> bool {
    let regs = *env.cpu.regs();
    let Some(sp) = regs[Cpu::SP].checked_sub(20).filter(|v| v & 3 == 0) else {
        return false;
    };
    if env.cpu.cpsr() & ((1 << 9) | Cpu::CPSR_THUMB) != 0 {
        return false;
    }
    let ret = env.dyld.return_to_host_routine().addr_with_thumb_bit();
    let mut data = [0u8; 20];
    for (i, r) in [4, 5, 6, 7, Cpu::LR].into_iter().enumerate() {
        let value = if r == Cpu::LR { ret } else { regs[r] };
        data[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    let Some(dest) = env
        .mem
        .get_bytes_fallible_mut(ConstVoidPtr::from_bits(sp), 20)
    else {
        return false;
    };
    if dest.len() != 20 {
        return false;
    }
    dest.copy_from_slice(&data);
    env.cpu.regs_mut()[Cpu::SP] = sp;
    GuestFunction::from_addr_and_thumb_flag(ENTRY + 4, false).call_without_pushing_stack_frame(env);
    assert_eq!(
        env.cpu.regs()[Cpu::SP],
        regs[Cpu::SP],
        "Original skin stack contract changed"
    );
    env.cpu
        .branch(GuestFunction::from_addr_with_thumb_bit(regs[Cpu::LR]));
    true
}
fn restore(env: &mut Environment) {
    if word(env, ENTRY) == Some(BKPT) {
        env.mem.write(MutPtr::<u32>::from_bits(ENTRY), PUSH);
    } else {
        echo!("[DSpc-skin-shadow] restore conflict; unknown word retained");
    }
    env.cpu.invalidate_cache_range(ENTRY, 4);
    env.framework_state.opengles.ds_pc_skin_shadow.armed = false;
    env.framework_state.opengles.ds_pc_skin_shadow.accelerate = false;
}
pub(crate) fn handle_breakpoint(env: &mut Environment) -> bool {
    if !env.framework_state.opengles.ds_pc_skin_shadow.armed
        || env.cpu.regs()[Cpu::PC] != ENTRY
        || word(env, ENTRY) != Some(BKPT)
    {
        return false;
    }
    if env.cpu.cpsr() & ((1 << 9) | Cpu::CPSR_THUMB) != 0 {
        restore(env);
        env.framework_state.opengles.ds_pc_skin_shadow.disabled = true;
        env.cpu
            .branch(GuestFunction::from_addr_and_thumb_flag(ENTRY, false));
        return true;
    }
    let start = Instant::now();
    let args = env.cpu.regs()[..4].try_into().unwrap();
    let before_fp = env.cpu.fpscr();
    let strided = env.framework_state.opengles.ds_pc_skin_shadow.strided;
    let mut scratch = std::mem::take(&mut env.framework_state.opengles.ds_pc_skin_shadow.scratch);
    let evaluated = evaluate(env, args, strided, &mut scratch);
    let (candidate, capture_ns, candidate_ns) = evaluated;
    let (accelerate, matches, remaining) = {
        let s = &env.framework_state.opengles.ds_pc_skin_shadow;
        (s.accelerate, s.mode_compared, s.until_recheck)
    };
    let recheck = remaining == 0;
    if accelerate && matches >= 64 && !recheck {
        if let Ok(candidate) = candidate.as_ref() {
            let candidate_bytes = &scratch.output_bytes[..candidate.bytes_len];
            let Some(destination) = env.mem.get_bytes_fallible_mut(
                ConstVoidPtr::from_bits(candidate.dest),
                candidate.bytes_len as u32,
            ) else {
                restore(env);
                env.framework_state.opengles.ds_pc_skin_shadow.disabled = true;
                env.cpu
                    .branch(GuestFunction::from_addr_and_thumb_flag(ENTRY, false));
                env.framework_state.opengles.ds_pc_skin_shadow.scratch = scratch;
                echo!("[DSpc-skin-shadow] accelerated destination became unreadable; disabled");
                return true;
            };
            if destination.len() != candidate_bytes.len() {
                restore(env);
                env.framework_state.opengles.ds_pc_skin_shadow.disabled = true;
                env.cpu
                    .branch(GuestFunction::from_addr_and_thumb_flag(ENTRY, false));
                env.framework_state.opengles.ds_pc_skin_shadow.scratch = scratch;
                return true;
            }
            destination.copy_from_slice(candidate_bytes);
            let return_to = GuestFunction::from_addr_with_thumb_bit(env.cpu.regs()[Cpu::LR]);
            env.cpu.set_fpscr(before_fp | candidate.flags);
            env.cpu.branch(return_to);
            let s = &mut env.framework_state.opengles.ds_pc_skin_shadow;
            s.calls += 1;
            s.vertices += u64::from(args[2]);
            s.capture_ns += capture_ns;
            s.candidate_ns += candidate_ns;
            s.accelerated += 1;
            s.host_calls += 1;
            s.until_recheck -= 1;
            s.handler_ns += start.elapsed().as_nanos();
            s.scratch = scratch;
            return true;
        }
    }
    let t = Instant::now();
    if !original_after_push(env) {
        restore(env);
        env.framework_state.opengles.ds_pc_skin_shadow.disabled = true;
        env.cpu
            .branch(GuestFunction::from_addr_and_thumb_flag(ENTRY, false));
        env.framework_state.opengles.ds_pc_skin_shadow.scratch = scratch;
        echo!("[DSpc-skin-shadow] unsupported CPU/stack; restored original");
        return true;
    }
    let original_ns = t.elapsed().as_nanos();
    let equal = candidate.as_ref().ok().map(|c| {
        bytes(env, c.dest, c.bytes_len) == Some(&scratch.output_bytes[..c.bytes_len])
            && env.cpu.fpscr() == (before_fp | c.flags)
    });
    let fp = env.cpu.fpscr();
    let frame = crate::window::DS_FRAME_COUNTER.load(Relaxed);
    let s = &mut env.framework_state.opengles.ds_pc_skin_shadow;
    s.calls += 1;
    s.vertices += u64::from(args[2]);
    s.capture_ns += capture_ns;
    s.candidate_ns += candidate_ns;
    s.original_ns += original_ns;
    match equal {
        Some(true) => {
            s.compared += 1;
            s.mode_compared += 1;
            if s.mode_compared >= 64 && s.until_recheck == 0 {
                s.until_recheck = 256;
            }
            if s.accelerate && s.mode_compared == 64 {
                echo!("[DSpc-skin-shadow] 64 live matches; host acceleration enabled with 256-call rechecks");
                echo!(
                    "[DSpc-skin-shadow] input={} original64 matches; host eligible",
                    if s.strided { "strided" } else { "packed" }
                );
            }
        }
        Some(false) => {
            s.mismatch += 1;
            s.disabled = true;
        }
        None => s.unsupported += 1,
    }
    s.handler_ns += start.elapsed().as_nanos();
    if s.calls <= 4 || equal == Some(false) {
        echo!("[DSpc-skin-shadow] sample calls={} frame={} first={} count={} equal={:?} unsupported={:?} fp_before={:#x} fp_after={:#x} candidate_flags={:?} original_ns={} candidate_ns={}",
            s.calls,frame,args[1],args[2],equal,candidate.as_ref().err(),before_fp,fp,candidate.as_ref().ok().map(|c|c.flags),original_ns,candidate_ns);
    }
    let now = Instant::now();
    if s.last_report
        .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(2))
        || equal == Some(false)
    {
        s.last_report = Some(now);
        echo!("[DSpc-skin-shadow] frame={} calls={} vertices={} compared={} mismatch={} unsupported={} capture_ns={} candidate_ns={} original_ns={} handler_ns={} accelerated={}; nested wall includes bridge/scheduling",
            frame,s.calls,s.vertices,s.compared,s.mismatch,s.unsupported,s.capture_ns,s.candidate_ns,s.original_ns,s.handler_ns,s.accelerated);
        echo!("[DSpc-skin-input] input={} mode_compared={} host_calls={} recheck_remaining={}; accelerated now counts actual host outputs only",
            if s.strided { "strided" } else { "packed" }, s.mode_compared, s.host_calls, s.until_recheck);
    }
    s.scratch = scratch;
    if equal == Some(false) {
        restore(env);
        echo!(
            "[DSpc-skin-shadow] mismatch; process observation disabled, original output retained"
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_capture_float_buffer_reuses_capacity() {
        let bytes = [
            1.0f32.to_le_bytes(),
            (-2.0f32).to_le_bytes(),
            3.0f32.to_le_bytes(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let mut values = Vec::with_capacity(3);
        floats(&bytes, 12, 3, 1, &mut values);
        let pointer = values.as_ptr();
        floats(&bytes, 12, 3, 1, &mut values);
        assert_eq!(values, [1.0, -2.0, 3.0]);
        assert_eq!(values.as_ptr(), pointer);
    }

    #[test]
    fn packed_capture_reclaim_returns_owned_buffers() {
        let mut scratch = Scratch::default();
        scratch.positions.reserve(4);
        scratch.weights.reserve(4);
        scratch.indices.reserve(4);
        scratch.matrices.reserve(4);
        let capacities = [
            scratch.positions.capacity(),
            scratch.weights.capacity(),
            scratch.indices.capacity(),
            scratch.matrices.capacity(),
        ];
        let snapshot = Snapshot {
            dest: 0,
            count: 0,
            positions: std::mem::take(&mut scratch.positions),
            weights: std::mem::take(&mut scratch.weights),
            indices: std::mem::take(&mut scratch.indices),
            matrices: std::mem::take(&mut scratch.matrices),
        };
        scratch.reclaim(snapshot);
        assert_eq!(
            capacities,
            [
                scratch.positions.capacity(),
                scratch.weights.capacity(),
                scratch.indices.capacity(),
                scratch.matrices.capacity(),
            ]
        );
    }

    #[test]
    fn packed_output_bytes_reuse_capacity() {
        let mut bytes = Vec::new();
        let first = crate::ds_pc_skin_math::pack_bytes(&[1.0, -2.0, 3.0], &mut bytes, 0)
            .bytes
            .to_vec();
        let capacity = bytes.capacity();
        assert_eq!(
            first,
            &[0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x40, 0x40,]
        );
        let second_len = crate::ds_pc_skin_math::pack_bytes(&[4.0, 5.0], &mut bytes, 0)
            .bytes
            .len();
        assert_eq!(bytes.capacity(), capacity);
        assert_eq!(second_len, 8);
    }
}

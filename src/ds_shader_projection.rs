//! Original-verified shader projection repair, at the same pure matrix
//! boundary as the accepted GLES1 finite-far opening repair.
use crate::{
    cpu::Cpu,
    mem::{ConstVoidPtr, MutPtr},
    Environment,
};
const ENTRY: u32 = 0x0056_75e0;
const SITE: u32 = 0x0056_763c;
const STORE: u32 = 0xe580_21c8;
const BKPT: u32 = 0xe120_0070;
const EXPECTED: &[u32] = &[
    0xe3a02001, 0xe5c02118, 0xe5912000, 0xe58021a0, 0xe5912004, 0xe58021a4, 0xe5912008, 0xe58021a8,
    0xe591200c, 0xe58021ac, 0xe5912010, 0xe58021b0, 0xe5912014, 0xe58021b4, 0xe5912018, 0xe58021b8,
    0xe591201c, 0xe58021bc, 0xe5912020, 0xe58021c0, 0xe5912024, 0xe58021c4, 0xe5912028, 0xe58021c8,
    0xe591202c, 0xe58021cc, 0xe5912030, 0xe58021d0, 0xe5912034, 0xe58021d4, 0xe5912038, 0xe58021d8,
    0xe591103c, 0xe58011dc, 0xe12fff1e,
];
const IPAD_ENTRY: u32 = 0x0059_d110;
const IPAD_SITE: u32 = 0x0059_d170;
const IPAD_EXPECTED: &[u32] = &[0xe3a03001,0xe5c03118,0xe591c000,0xe5919004,0xe5912008,0xe591300c,0xe580c1a0,0xe58091a4,0xe591c010,0xe5919014,0xe58021a8,0xe58031ac,0xe5912018,0xe591301c,0xe580c1b0,0xe58091b4,0xe591c020,0xe5919024,0xe58021b8,0xe58031bc,0xe5912028,0xe591302c,0xe580c1c0,0xe58091c4,0xe58021c8,0xe58031cc,0xe591c030,0xe5919034,0xe5912038,0xe591303c,0xe580c1d0,0xe58091d4,0xe58021d8,0xe58031dc,0xe12fff1e];
#[derive(Default)]
pub(crate) struct State {
    armed: bool,
    site: u32,
    corrected: u64,
}
fn word(env: &Environment, at: u32) -> Option<u32> {
    Some(u32::from_le_bytes(
        env.mem
            .get_bytes_fallible(ConstVoidPtr::from_bits(at), 4)?
            .try_into()
            .ok()?,
    ))
}
fn matches(entry: Option<u32>, mut read: impl FnMut(u32) -> Option<u32>) -> bool {
    entry == Some(ENTRY)
        && EXPECTED
            .iter()
            .enumerate()
            .all(|(i, v)| read(ENTRY + i as u32 * 4) == Some(*v))
}
fn opening_projection(m: &[f32; 16]) -> bool {
    m.iter().all(|v| v.is_finite())
        && (-1.006..=-1.001).contains(&m[10])
        && (-0.105..=-0.095).contains(&m[14])
        && (m[11] + 1.0).abs() < 0.00001
        && [2, 3, 6, 7, 8, 9, 12, 13, 15]
            .iter()
            .all(|i| m[*i].abs() < 0.00001)
}
fn matches_ipad(entry: Option<u32>, mut read: impl FnMut(u32) -> Option<u32>) -> bool {
    entry == Some(IPAD_ENTRY) && IPAD_EXPECTED.iter().enumerate().all(|(i,v)| read(IPAD_ENTRY+i as u32*4)==Some(*v))
}
pub(crate) fn prime(env: &mut Environment) {
    if !matches!(env.bundle.bundle_identifier(), "com.ea.deadspace.bv" | "com.ea.deadspace.ipad.bv")
        || std::env::var("TOUCHHLE_DS_ES2_PROJ_FIX").as_deref() == Ok("0")
        || std::env::var("TOUCHHLE_DS_PROJ_Z_INFINITE").as_deref() == Ok("0")
        || std::env::var_os("TOUCHHLE_DS_NO_PROJ_Z_FIX").is_some()
    {
        return;
    }
    let entry = env.bins[0]
        .exported_symbols
        .get("__ZN3m3g17ShaderRendererAPI13SetProjectionERKN10Vectormath3Aos7Matrix4E")
        .copied();
    let site = if env.bundle.bundle_identifier()=="com.ea.deadspace.ipad.bv" && env.bundle.bundle_version()=="1.0.3" && matches_ipad(entry,|a|word(env,a)) { IPAD_SITE }
        else if env.bundle.bundle_identifier()=="com.ea.deadspace.bv" && matches(entry,|a|word(env,a)) { SITE } else {

        echo!("[DSes2-projection] unknown original code; unchanged");
        return;
    };
    env.mem.write(MutPtr::<u32>::from_bits(site), BKPT);
    env.cpu.invalidate_cache_range(site, 4);
    env.framework_state.opengles.ds_shader_projection.armed = true;
    env.framework_state.opengles.ds_shader_projection.site = site;
    echo!("[DSes2-projection] verified projection store armed");
}
pub(crate) fn handle_breakpoint(env: &mut Environment) -> bool {
    if !env.framework_state.opengles.ds_shader_projection.armed || env.cpu.regs()[Cpu::PC] != env.framework_state.opengles.ds_shader_projection.site {
        return false;
    }
    let regs = *env.cpu.regs();
    let corrected = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(regs[1]), 64)
        .and_then(|b| {
            let m: [f32; 16] = std::array::from_fn(|i| {
                f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())
            });
            (m[10].to_bits() == regs[2] && opening_projection(&m)).then_some(())
        })
        .is_some();
    // Replay STR r2,[r0,#0x1c8], retaining every GPR/FPR and CPSR bit.
    // Only the matrix destination changes. The source const-reference is untouched.
    let value = if corrected {
        (-1.0f32).to_bits()
    } else {
        regs[2]
    };
    let site=env.framework_state.opengles.ds_shader_projection.site;
    replay_store(&mut env.cpu, &mut env.mem, value, site);
    if corrected {
        let state = &mut env.framework_state.opengles.ds_shader_projection;
        state.corrected = state.corrected.saturating_add(1);
        if state.corrected == 1 {
            echo!("[DSes2-projection] opening finite-far row corrected; near coefficient and x/y unchanged");
        }
    }
    true
}

fn replay_store(cpu: &mut Cpu, mem: &mut crate::mem::Mem, value: u32, site: u32) {
    let address = cpu.regs()[0].wrapping_add(0x1c8);
    mem.write(MutPtr::<u32>::from_bits(address), value);
    cpu.regs_mut()[Cpu::PC] = site + 4;
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cpu::{CpuError, CpuState},
        mem::Mem,
    };
    #[test]
    fn original_full_function_required_and_unknown_entry_never_reads() {
        let read = |a| Some(EXPECTED[((a - ENTRY) / 4) as usize]);
        assert!(matches(Some(ENTRY), read));
        for i in 0..EXPECTED.len() {
            assert!(!matches(Some(ENTRY), |a| read(a)
                .map(|v| v ^ u32::from(a == ENTRY + i as u32 * 4))));
        }
        for entry in [None, Some(ENTRY + 1), Some(ENTRY + 4)] {
            assert!(!matches(entry, |_| panic!("unknown binary read")));
        }
        assert_eq!(EXPECTED[((SITE - ENTRY) / 4) as usize], STORE);
    }
    #[test]
    fn ipad_profile_requires_every_original_word_and_exact_entry() {
        let read=|a|Some(IPAD_EXPECTED[((a-IPAD_ENTRY)/4) as usize]);
        assert!(matches_ipad(Some(IPAD_ENTRY),read));
        assert!(!matches_ipad(Some(IPAD_ENTRY+4), |_|panic!("unknown entry read")));
        for i in 0..IPAD_EXPECTED.len() { assert!(!matches_ipad(Some(IPAD_ENTRY),|a|read(a).map(|v|v^u32::from(a==IPAD_ENTRY+i as u32*4)))); }
        assert_eq!(IPAD_EXPECTED[((IPAD_SITE-IPAD_ENTRY)/4) as usize],STORE);
    }
    fn projection() -> [f32; 16] {
        let mut m = [0.; 16];
        m[0] = 1.2;
        m[5] = 1.8;
        m[10] = -1.004008;
        m[11] = -1.;
        m[14] = -0.1002;
        m
    }
    #[test]
    fn pure_opening_projection_only_preserves_ortho_gameplay_and_unknown() {
        let m = projection();
        assert!(opening_projection(&m));
        let mut rotated = m;
        rotated[0] = 0.;
        rotated[1] = 1.2;
        rotated[4] = -1.8;
        rotated[5] = 0.;
        assert!(opening_projection(&rotated));
        for (i, value) in [
            (10, -1.0001),
            (14, -0.2),
            (11, 1.),
            (15, 1.),
            (12, 3.),
            (0, f32::NAN),
            (9, 0.1),
        ] {
            let mut different = m;
            different[i] = value;
            assert!(!opening_projection(&different), "index {i}");
        }
    }
    fn execute(replay: bool, corrected: bool) -> ([u32; 16], u32, u32, Vec<u8>) {
        let mut mem = Mem::new();
        mem.write(
            MutPtr::<u32>::from_bits(SITE),
            if replay { BKPT } else { STORE },
        );
        mem.write(MutPtr::<u32>::from_bits(SITE + 4), 0xef00_0001);
        let mut cpu = Cpu::new(Some(&mut mem));
        cpu.set_cpsr(Cpu::CPSR_USER_MODE | 0xa800_0000);
        cpu.set_fpscr(0x0100_0095);
        for (i, r) in cpu.regs_mut().iter_mut().enumerate() {
            *r = 0x1020_3000 + i as u32 * 0x100;
        }
        cpu.regs_mut()[0] = 0x6000;
        cpu.regs_mut()[1] = 0x7000;
        cpu.regs_mut()[2] = projection()[10].to_bits();
        cpu.regs_mut()[Cpu::SP] = 0x9000;
        cpu.regs_mut()[Cpu::PC] = SITE;
        mem.write(MutPtr::<u32>::from_bits(0x7000), 0x1234_5678);
        let mut ticks = 1_000_000;
        if replay {
            assert!(matches!(
                cpu.run_or_step(&mut mem, Some(&mut ticks)),
                CpuState::Error(CpuError::Breakpoint)
            ));
            cpu.regs_mut()[Cpu::PC] -= 4;
            let value = if corrected {
                (-1.0f32).to_bits()
            } else {
                cpu.regs()[2]
            };
            replay_store(&mut cpu, &mut mem, value, SITE);
        }
        assert!(matches!(
            cpu.run_or_step(&mut mem, Some(&mut ticks)),
            CpuState::Svc(1)
        ));
        let result = mem
            .get_bytes_fallible(ConstVoidPtr::from_bits(0x61c8), 4)
            .unwrap()
            .to_vec();
        assert_eq!(
            mem.read(crate::mem::ConstPtr::<u32>::from_bits(0x7000)),
            0x1234_5678
        );
        (*cpu.regs(), cpu.cpsr(), cpu.fpscr(), result)
    }
    #[test]
    fn actual_jit_store_replay_retains_registers_flags_and_source() {
        let original = execute(false, false);
        assert_eq!(original, execute(true, false));
        let corrected = execute(true, true);
        assert_eq!(original.0, corrected.0);
        assert_eq!(original.1, corrected.1);
        assert_eq!(original.2, corrected.2);
        assert_eq!(corrected.3, (-1.0f32).to_le_bytes());
    }
}

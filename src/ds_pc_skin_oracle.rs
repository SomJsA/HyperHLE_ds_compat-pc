//! Explicit private original-code fixture, never included in the repository/package.
use super::*;
use crate::{
    cpu::{Cpu, CpuState},
    mem::{ConstVoidPtr, Mem, MutPtr},
};
use std::{path::PathBuf, time::Instant};
const ENTRY: u32 = 0x37fac8;
const RETURN: u32 = 0x700000;
fn put(mem: &mut Mem, at: u32, bytes: &[u8]) {
    mem.get_bytes_fallible_mut(ConstVoidPtr::from_bits(at), bytes.len() as u32)
        .expect("fixture memory")
        .copy_from_slice(bytes);
}
fn word(mem: &mut Mem, at: u32, v: u32) {
    mem.write(MutPtr::<u32>::from_bits(at), v);
}
fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x100000001b3)
    })
}
fn fixture(mem: &mut Mem, path: &PathBuf, name: &str, at: u32, len: usize, hash: u64) {
    let bytes =
        std::fs::read(path.join(name)).expect("explicit private original-code fixture required");
    assert_eq!(bytes.len(), len);
    assert_eq!(fingerprint(&bytes), hash);
    put(mem, at, &bytes);
}
fn random(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    f32::from_bits((*seed & 0x807fffff) | ((120 + (*seed >> 23) % 14) << 23))
}
fn floats(mem: &mut Mem, at: u32, stride: usize, first: usize, values: &[f32], components: usize) {
    for (i, row) in values.chunks_exact(components).enumerate() {
        let bytes: Vec<u8> = row.iter().flat_map(|v| v.to_le_bytes()).collect();
        put(mem, at + ((i + first) * stride) as u32, &bytes);
    }
}
fn array(mem: &mut Mem, global: u32, obj: u32, base: u32, stride: u32, components: u32) {
    word(mem, global, obj);
    word(mem, obj + 68, components);
    word(mem, obj + 76, base - 16);
    word(mem, obj + 84, 16);
    word(mem, obj + 88, stride);
}
#[test]
#[ignore = "requires explicit private unchanged IPA code fixture; no copyrighted code in Git"]
fn pc_skin_original_arm_oracle_positions_bytes_fpscr_strides_and_controls() {
    let path = PathBuf::from(
        std::env::var_os("TOUCHHLE_DS_SKIN_ORACLE_FIXTURE").expect("fixture env required"),
    );
    let mut mem = Mem::new();
    mem.set_null_segment_size(0x1000);
    fixture(
        &mut mem,
        &path,
        "position-arm.bin",
        ENTRY,
        716,
        10538268167844965500,
    );
    fixture(
        &mut mem,
        &path,
        "getdata-arm.bin",
        0x53d1a4,
        16,
        5222870166602214087,
    );
    word(&mut mem, RETURN, 0xef000001);
    word(&mut mem, 0xabb83c, 0x800300);
    word(&mut mem, 0x800308, 0x400000);
    let mut cpu = Cpu::new(Some(&mut mem));
    let mut seed = 0x5a31d274;
    let mut passed = 0;
    let mut vertices = 0;
    let start = Instant::now();
    for case in 0..192usize {
        let count = [1, 2, 17, 257, 844, 31][case % 6];
        let first = [0, 1, 7][case % 3];
        let ps = [12, 16, 24, 32, 64][case % 5];
        let ws = [16, 20, 32, 64][case % 4];
        let is = [4, 8, 16][case % 3];
        let pos: Vec<f32> = (0..count * 3).map(|_| random(&mut seed)).collect();
        let mut wgt = vec![0.0; count * 4];
        let mut idx = vec![-1i8; count * 4];
        for i in 0..count {
            let active = 1 + (case + i) % 4;
            for b in 0..active {
                wgt[i * 4 + b] = random(&mut seed).abs();
                idx[i * 4 + b] = ((case + i + b) % 6) as i8;
            }
            if i % 5 == 0 {
                wgt[i * 4] = if case % 2 == 0 { 0.0 } else { -0.0 };
            }
        }
        let mat: Vec<f32> = (0..6 * 16).map(|_| random(&mut seed)).collect();
        array(&mut mem, 0xabb844, 0x800000, 0x100000, ps as u32, 3);
        array(&mut mem, 0xabb848, 0x800100, 0x200000, is as u32, 4);
        array(&mut mem, 0xabb84c, 0x800200, 0x300000, ws as u32, 4);
        floats(&mut mem, 0x100000, ps, first, &pos, 3);
        floats(&mut mem, 0x300000, ws, first, &wgt, 4);
        floats(&mut mem, 0x400000, 64, 0, &mat, 16);
        for i in 0..count {
            put(
                &mut mem,
                0x200000 + ((i + first) * is) as u32,
                &idx[i * 4..i * 4 + 4]
                    .iter()
                    .map(|v| *v as u8)
                    .collect::<Vec<_>>(),
            );
        }
        let destination = vec![0xa5; (first + count + 2) * 12];
        put(&mut mem, 0x600000, &destination);
        let controls = [
            0, 0x00400000, 0x00800000, 0x00c00000, 0x01000000, 0x02000000, 0x03c00000, 0x9f,
        ][case % 8];
        cpu.set_fpscr(controls);
        cpu.set_cpsr(Cpu::CPSR_USER_MODE | 0xa8000000);
        for (r, value) in cpu.regs_mut().iter_mut().enumerate() {
            *value = 0x12340000 + r as u32 * 0x100;
        }
        cpu.regs_mut()[0] = 0x600000;
        cpu.regs_mut()[1] = first as u32;
        cpu.regs_mut()[2] = count as u32;
        cpu.regs_mut()[Cpu::SP] = 0x900000;
        cpu.regs_mut()[Cpu::LR] = RETURN;
        cpu.regs_mut()[Cpu::PC] = ENTRY;
        let before = *cpu.regs();
        let mut ticks = 100_000_000;
        assert!(
            matches!(
                cpu.run_or_step(&mut mem, Some(&mut ticks)),
                CpuState::Svc(1)
            ),
            "case{case}"
        );
        for r in [4, 5, 6, 7, 8, 10, 11, Cpu::SP] {
            assert_eq!(cpu.regs()[r], before[r], "callee case{case} r{r}");
        }
        let actual = mem
            .get_bytes_fallible(ConstVoidPtr::from_bits(0x600000), destination.len() as u32)
            .unwrap();
        let expected =
            calculate(&pos, &wgt, &idx, &mat, count as u32).expect("ordinary fixture eligible");
        // Exercise actual bounded guest byte views, with every original stride
        // and nonzero starting vertex retained rather than packed copies.
        let view = |at, len| {
            mem.get_bytes_fallible(ConstVoidPtr::from_bits(at), len)
                .unwrap()
        };
        let borrowed = strided::calculate(&strided::Inputs {
            positions: view(
                0x100000 + (first * ps) as u32,
                ((count - 1) * ps + 12) as u32,
            ),
            position_stride: ps,
            weights: view(
                0x300000 + (first * ws) as u32,
                ((count - 1) * ws + 16) as u32,
            ),
            weight_stride: ws,
            indices: view(
                0x200000 + (first * is) as u32,
                ((count - 1) * is + 4) as u32,
            ),
            index_stride: is,
            matrices: view(0x400000, (mat.len() * 4) as u32),
            count: count as u32,
        })
        .expect("ordinary original guest strided fixture");
        assert_eq!(
            borrowed.bytes, expected.bytes,
            "strided versus packed/original bytes case{case}"
        );
        assert_eq!(
            borrowed.flags, expected.flags,
            "strided fullFPSCR flags case{case}"
        );
        assert_eq!(
            &actual[..first * 12],
            &destination[..first * 12],
            "prefix case{case}"
        );
        assert_eq!(
            &actual[(first + count) * 12..],
            &destination[(first + count) * 12..],
            "suffix case{case}"
        );
        assert_eq!(
            &actual[first * 12..(first + count) * 12],
            expected.bytes,
            "output case{case}"
        );
        assert_eq!(cpu.fpscr(), controls | expected.flags, "FPSCR case{case}");
        passed += 1;
        vertices += count;
    }
    println!("ARM oracle passed{passed} cases/{vertices} vertices, packed+borrowed strided fulloutput+FPSCR+calleeGPR+guards; elapsed_ms={} includes fixture copies,JIT and kernels; not FPS",start.elapsed().as_millis());
}
#[test]
#[allow(deprecated)]
fn pc_skin_kernel_rejects_unsupported_and_restores_host_mxcsr() {
    use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
    struct Restore(u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                _mm_setcsr(self.0);
            }
        }
    }
    let saved = unsafe { _mm_getcsr() };
    let _restore = Restore(saved);
    let altered = (saved & !0x6000) | 0x6000 | 0x3f | 0x1f80 | 0x8040;
    unsafe {
        _mm_setcsr(altered);
    }
    let mut mat = vec![0.0; 16];
    for i in 0..4 {
        mat[i * 5] = 1.0;
    }
    let pos = [1.25, -2.5, 3.75];
    let w = [1.0, 0.0, f32::NAN, f32::NAN];
    let idx = [0, -1, -1, -1];
    let result = calculate(&pos, &w, &idx, &mat, 1).unwrap();
    assert_eq!(
        result.bytes,
        pos.into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(result.flags, 0);
    assert_eq!(unsafe { _mm_getcsr() }, altered);
    let raw_pos = pos
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let raw_w = w.into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>();
    let raw_mat = mat.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let view = strided::Inputs {
        positions: &raw_pos,
        position_stride: 12,
        weights: &raw_w,
        weight_stride: 16,
        indices: &[0, 255, 255, 255],
        index_stride: 4,
        matrices: &raw_mat,
        count: 1,
    };
    let borrowed = strided::calculate(&view).unwrap();
    assert_eq!(borrowed.bytes, result.bytes);
    assert_eq!(borrowed.flags, 0);
    assert_eq!(unsafe { _mm_getcsr() }, altered);
    assert!(calculate(&[f32::NAN, 0.0, 0.0], &w, &idx, &mat, 1).is_err());
    assert!(calculate(&[f32::from_bits(1), 0.0, 0.0], &w, &idx, &mat, 1).is_err());
    assert!(calculate(&pos, &w, &[-1, 0, 0, 0], &mat, 1).is_err());
    assert!(calculate(&pos, &w, &[1, 0, 0, 0], &mat, 1).is_err());
    assert!(calculate(&pos, &w, &idx, &mat, 0).is_err());
    // Ordinary inputs can still overflow/underflow: never commit these results.
    assert!(calculate(&[f32::MAX, 0.0, 0.0], &[2.0, 0.0, 0.0, 0.0], &idx, &mat, 1).is_err());
    mat[0] = f32::MIN_POSITIVE;
    assert!(calculate(&[f32::MIN_POSITIVE, 0.0, 0.0], &w, &idx, &mat, 1).is_err());
    assert_eq!(unsafe { _mm_getcsr() }, altered);
}

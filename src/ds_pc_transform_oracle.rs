//! Original-code fixture is explicit and private, never included in Git/package.
use super::*;
use crate::{cpu::{Cpu, CpuContext, CpuState},mem::{ConstVoidPtr,Mem,MutPtr}};
use std::path::PathBuf;
const ENTRY:u32=0x4ea204;
const ARENA:u32=0x200000;
const ARM_RETURN:u32=0x700000;
const THUMB_RETURN:u32=0x701000;
fn put(mem:&mut Mem,at:u32,data:&[u8]) {
    mem.get_bytes_fallible_mut(ConstVoidPtr::from_bits(at),data.len() as u32).unwrap().copy_from_slice(data);
}
fn bytes(mem:&Mem,at:u32,len:u32)->Vec<u8> {
    mem.get_bytes_fallible(ConstVoidPtr::from_bits(at),len).unwrap().to_vec()
}
fn matrix(mem:&Mem,object:u32)->[f32;16] {
    let data=bytes(mem,object+16,64);
    std::array::from_fn(|i|f32::from_le_bytes(data[i*4..i*4+4].try_into().unwrap()))
}
fn store_matrix(mem:&mut Mem,object:u32,values:&[f32;16]) {
    for (i,value) in values.iter().enumerate() {put(mem,object+16+i as u32*4,&value.to_le_bytes());}
}
fn random(seed:&mut u32)->f32 {
    *seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);
    f32::from_bits((*seed&0x807f_ffff)|((120+(*seed>>23)%14)<<23))
}
fn fingerprint(data:&[u8])->u64 {
    data.iter().fold(0xcbf29ce484222325,|h,b|(h^u64::from(*b)).wrapping_mul(0x100000001b3))
}
#[test]
#[ignore="requires explicit private original108byte ARM fixture; no game code in Git"]
fn pc_transform_original_arm_oracle_outputs_flags_aliases_and_abi() {
    let folder=PathBuf::from(std::env::var_os("TOUCHHLE_DS_TRANSFORM_ORACLE_FIXTURE").expect("explicit original fixture"));
    let original=std::fs::read(folder.join("postmultiply-arm.bin")).unwrap();
    assert_eq!(original.len(),108);assert_eq!(fingerprint(&original),15179455414201691398);
    assert_eq!(u32::from_le_bytes(original[..4].try_into().unwrap()),0xe2802010);
    let mut mem=Mem::new();mem.set_null_segment_size(0x1000);
    put(&mut mem,ENTRY,&original);
    mem.write(MutPtr::<u32>::from_bits(ARM_RETURN),0xef000001);
    mem.write(MutPtr::<u16>::from_bits(THUMB_RETURN),0xdf01);
    let mut cpu=Cpu::new(Some(&mut mem));let mut seed=0x571cab49;
    for case in 0..384usize {
        put(&mut mem,ARENA,&[0xa5;512]);
        let lhs=ARENA+128+(case%4) as u32*4;
        let rhs=match case%5 {0=>ARENA+320,1=>lhs,2=>lhs+16,3=>lhs-16,_=>lhs+32};
        let mut a=std::array::from_fn(|_|random(&mut seed));
        let mut b=std::array::from_fn(|_|random(&mut seed));
        if case%8==0 {for i in 0..16 {a[i]=if i%5==0{1.0}else{-0.0};}}
        if case%8==1 {for i in 0..16 {b[i]=if i%5==0{-1.0}else{0.0};}}
        store_matrix(&mut mem,lhs,&a);store_matrix(&mut mem,rhs,&b);
        let before=bytes(&mem,ARENA,512);
        let a=matrix(&mem,lhs);let b=matrix(&mem,rhs);
        let candidate=calculate(&a,&b).expect("ordinary reference matrices");
        let controls=[0,0x00400000,0x00800000,0x00c00000,0x01000000,0x02000000,0x03c00000,0x9f,0x6000001f][case%9];
        let thumb=case%2==1;
        let ret=if thumb{THUMB_RETURN|1}else{ARM_RETURN};
        let mut initial=CpuContext::new();
        initial.regs=std::array::from_fn(|i|0x12340000+i as u32*0x100);
        initial.extregs=std::array::from_fn(|i|0x9a450000+i as u32*37);
        initial.regs[0]=lhs;initial.regs[1]=rhs;initial.regs[Cpu::SP]=0x900000;
        initial.regs[Cpu::LR]=ret;initial.regs[Cpu::PC]=ENTRY;
        initial.cpsr=Cpu::CPSR_USER_MODE|0xa8000000;initial.fpscr=controls;
        let regs=initial.regs;let callee_fp=initial.extregs[16..32].to_vec();
        cpu.swap_context(&mut initial);
        let mut ticks=1_000_000;
        assert!(matches!(cpu.run_or_step(&mut mem,Some(&mut ticks)),CpuState::Svc(1)),"case{case}");
        let mut after=CpuContext::new();cpu.swap_context(&mut after);
        let mut expected=before.clone();let at=(lhs+16-ARENA) as usize;
        expected[at..at+64].copy_from_slice(&candidate.bytes);
        assert_eq!(bytes(&mem,ARENA,512),expected,"full arena/headers/aliases case{case}");
        assert_eq!(after.fpscr,controls|candidate.flags,"completeFPSCR case{case}");
        for r in [0,4,5,6,7,8,9,10,11,Cpu::SP,Cpu::LR] {assert_eq!(after.regs[r],regs[r],"calleeGPR case{case} r{r}");}
        assert_eq!(&after.extregs[16..32],callee_fp.as_slice(),"calleeD8-D15 case{case}");
        assert_eq!(after.cpsr, (Cpu::CPSR_USER_MODE|0xa8000000)|if thumb{Cpu::CPSR_THUMB}else{0},"CPSR case{case}");
        assert_eq!(after.regs[1],rhs+80);assert_eq!(after.regs[2],lhs+80);assert_eq!(after.regs[3],lhs+16);assert_eq!(after.regs[12],lhs+80);
        // Compare exact replay of the non-PUSH first ADD against a full original
        // execution, including every register and both ARM/Thumb return paths.
        put(&mut mem,ARENA,&before);
        let mut replay=CpuContext::new();replay.regs=regs;
        replay.extregs=std::array::from_fn(|i|0x9a450000+i as u32*37);
        replay.regs[2]=regs[0].wrapping_add(16);replay.regs[Cpu::PC]=ENTRY+4;
        replay.cpsr=Cpu::CPSR_USER_MODE|0xa8000000;replay.fpscr=controls;
        cpu.swap_context(&mut replay);let mut ticks=1_000_000;
        assert!(matches!(cpu.run_or_step(&mut mem,Some(&mut ticks)),CpuState::Svc(1)));
        let mut replay_after=CpuContext::new();cpu.swap_context(&mut replay_after);
        assert_eq!(replay_after.regs,after.regs,"ADD replay GPR case{case}");
        assert_eq!(replay_after.extregs,after.extregs,"ADD replay allFPR case{case}");
        assert_eq!(replay_after.cpsr,after.cpsr);assert_eq!(replay_after.fpscr,after.fpscr);
        assert_eq!(bytes(&mem,ARENA,512),expected,"ADD replay arena case{case}");
        // Exercise the actual host register-output plan against every native
        // GPR/FPR, including volatile SIMD values and both return instruction sets.
        let mut hosted=CpuContext::new();hosted.regs=regs;
        hosted.extregs=std::array::from_fn(|i|0x9a450000+i as u32*37);
        hosted.cpsr=Cpu::CPSR_USER_MODE|0xa8000000;hosted.fpscr=controls;
        cpu.swap_context(&mut hosted);
        crate::ds_pc_transform::apply_register_outputs(&mut cpu,[lhs,rhs],&a,&b,&candidate.bytes);
        cpu.set_fpscr(controls|candidate.flags);
        cpu.branch(crate::abi::GuestFunction::from_addr_with_thumb_bit(ret));
        let mut ticks=1_000_000;
        assert!(matches!(cpu.run_or_step(&mut mem,Some(&mut ticks)),CpuState::Svc(1)));
        let mut hosted_after=CpuContext::new();cpu.swap_context(&mut hosted_after);
        assert_eq!(hosted_after.regs,after.regs,"host exactGPR case{case}");
        assert_eq!(hosted_after.extregs,after.extregs,"host exactFPR case{case}");
        assert_eq!(hosted_after.cpsr,after.cpsr);assert_eq!(hosted_after.fpscr,after.fpscr);
    }
    println!("Transform ARM oracle384cases/6144outputfloats: fullarena+headers+alias,fullFPSCR,calleeGPR/FPR,CPSR,exactADDreplay,ARM/Thumbreturns; no FPS claim");
}

#[test]
#[allow(deprecated)]
fn pc_transform_kernel_rejects_specials_and_restores_mxcsr() {
    use std::arch::x86_64::{_mm_getcsr,_mm_setcsr};
    struct Restore(u32);
    impl Drop for Restore {fn drop(&mut self){unsafe{_mm_setcsr(self.0)}}}
    let saved=unsafe{_mm_getcsr()};let _restore=Restore(saved);
    let altered=(saved&!0x6000)|0x6000|0x3f|0x1f80|0x8040;unsafe{_mm_setcsr(altered)};
    let mut identity=[0.0f32;16];for i in 0..4{identity[i*5]=1.0;}
    let result=calculate(&identity,&identity).unwrap();
    assert_eq!(result.flags,0);
    for (i,value) in identity.iter().enumerate(){assert_eq!(&result.bytes[i*4..i*4+4],&value.to_le_bytes());}
    assert_eq!(unsafe{_mm_getcsr()},altered);
    for value in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY,f32::from_bits(1)] {
        let mut bad=identity;bad[0]=value;assert!(calculate(&bad,&identity).is_err());
    }
    let mut big=identity;big[0]=f32::MAX;let mut two=identity;two[0]=2.0;
    assert!(calculate(&big,&two).is_err());
    let mut tiny=identity;tiny[0]=f32::MIN_POSITIVE;assert!(calculate(&tiny,&tiny).is_err());
    assert_eq!(unsafe{_mm_getcsr()},altered);
}

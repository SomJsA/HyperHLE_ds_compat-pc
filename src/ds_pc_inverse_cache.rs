//! Explicit opt-in memoization for the original Matrix4 inverse routine.
//! The candidate never substitutes a new inverse implementation: it only
//! replays a byte/FPSCR/FPR result after an exact input-and-control match.
use crate::{abi::GuestFunction,cpu::Cpu,mem::{ConstVoidPtr,MutPtr},Environment};
use std::{path::PathBuf,sync::atomic::Ordering::Relaxed};

const ENTRY:u32=0x2b83d4;
const FIRST_WORD:u32=0xe52d7004;
const BKPT:u32=0xe1200070;
const FINGERPRINT:u64=17774755090543758103;
const FPSCR_STICKY:u32=0x9f;

#[derive(Clone,Copy)]
struct Cache {
    input:[u32;16],
    control:u32,
    output:[u8;64],
    flags:u32,
    extregs:[u32;16],
}

#[derive(Default)]
pub(crate) struct State {
    configured:bool,
    trigger:Option<PathBuf>,
    last_frame:u64,
    armed:bool,
    disabled:bool,
    calls:u64,
    hits:u64,
    misses:u64,
    cache:Option<Cache>,
}

fn bytes(env:&Environment,at:u32,len:u32)->Option<&[u8]> {
    if at<0x1000 || u64::from(at)+u64::from(len)>(1u64<<32) {return None;}
    let data=env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(at),len)?;
    (data.len()==len as usize).then_some(data)
}
fn word(env:&Environment)->Option<u32> {Some(u32::from_le_bytes(bytes(env,ENTRY,4)?.try_into().ok()?))}
fn fingerprint(data:&[u8])->u64 {
    data.iter().fold(0xcbf29ce484222325,|h,b|(h^u64::from(*b)).wrapping_mul(0x100000001b3))
}
fn arm(env:&mut Environment)->bool {
    let s=&env.framework_state.opengles.ds_pc_inverse_cache;
    if s.disabled || !s.configured {return false;}
    if s.armed {return true;}
    if fingerprint(bytes(env,ENTRY,108).unwrap_or(&[]))!=FINGERPRINT || word(env)!=Some(FIRST_WORD) {
        echo!("[DSpc-inverse-cache] unknown original code; no hook installed");
        return false;
    }
    env.mem.write(MutPtr::<u32>::from_bits(ENTRY),BKPT);
    env.cpu.invalidate_cache_range(ENTRY,4);
    env.framework_state.opengles.ds_pc_inverse_cache.armed=true;
    true
}
fn restore(env:&mut Environment) {
    if word(env)==Some(BKPT) {env.mem.write(MutPtr::<u32>::from_bits(ENTRY),FIRST_WORD);}
    else {echo!("[DSpc-inverse-cache] restore conflict; unknown word retained");}
    env.cpu.invalidate_cache_range(ENTRY,4);
    env.framework_state.opengles.ds_pc_inverse_cache.armed=false;
}
pub(crate) fn prime(env:&mut Environment) {
    if env.bundle.bundle_identifier()!="com.ea.deadspace.bv" {return;}
    if std::env::var("TOUCHHLE_DS_PC_INVERSE_CACHE").as_deref()!=Ok("1") {return;}
    let Some(path)=std::env::var_os("TOUCHHLE_DS_PC_INVERSE_TRIGGER") else {
        echo!("[DSpc-inverse-cache] disabled: explicit trigger required");
        return;
    };
    let s=&mut env.framework_state.opengles.ds_pc_inverse_cache;
    s.configured=true;
    s.trigger=Some(PathBuf::from(path));
    echo!("[DSpc-inverse-cache] waiting explicit trigger; exact input/control replay");
}
pub(crate) fn poll(env:&mut Environment) {
    if !env.framework_state.opengles.ds_pc_inverse_cache.configured {return;}
    let frame=crate::window::DS_FRAME_COUNTER.load(Relaxed);
    let s=&mut env.framework_state.opengles.ds_pc_inverse_cache;
    if s.last_frame==frame || s.trigger.is_none() {return;}
    s.last_frame=frame;
    if !s.trigger.as_ref().unwrap().is_file() {return;}
    s.trigger=None;
    if arm(env) {echo!("[DSpc-inverse-cache] armed frame={}; exact memoization only",frame);}
}

fn input_key(env:&Environment,input:u32)->Option<[u32;16]> {
    let data=bytes(env,input,64)?;
    Some(std::array::from_fn(|i|u32::from_le_bytes(data[i*4..i*4+4].try_into().unwrap())))
}
fn cache_matches(cache:Cache,input:&[u32;16],control:u32)->bool {
    cache.input==*input && cache.control==control
}
fn report(env:&mut Environment) {
    let s=&mut env.framework_state.opengles.ds_pc_inverse_cache;
    if s.calls<=4 || s.calls.is_power_of_two() {
        echo!("[DSpc-inverse-cache] frame={} calls={} hits={} misses={}; exact replay counts, not CPU time",
            crate::window::DS_FRAME_COUNTER.load(Relaxed),s.calls,s.hits,s.misses);
    }
}

pub(crate) fn handle_breakpoint(env:&mut Environment)->bool {
    if !env.framework_state.opengles.ds_pc_inverse_cache.armed
        || env.cpu.regs()[Cpu::PC]!=ENTRY || word(env)!=Some(BKPT) {return false;}
    if env.cpu.cpsr()&((1<<9)|Cpu::CPSR_THUMB)!=0 {
        restore(env);
        env.framework_state.opengles.ds_pc_inverse_cache.disabled=true;
        env.cpu.branch(GuestFunction::from_addr_and_thumb_flag(ENTRY,false));
        return true;
    }
    let destination=env.cpu.regs()[0];
    let input=env.cpu.regs()[1];
    let Some(key)=input_key(env,input) else {
        restore(env);
        env.framework_state.opengles.ds_pc_inverse_cache.disabled=true;
        env.cpu.branch(GuestFunction::from_addr_and_thumb_flag(ENTRY,false));
        return true;
    };
    let before_fp=env.cpu.fpscr();
    let control=before_fp & !FPSCR_STICKY;
    let cached=env.framework_state.opengles.ds_pc_inverse_cache.cache;
    if cached.is_some_and(|entry|cache_matches(entry,&key,control)) {
        let entry=cached.unwrap();
        let Some(dest)=env.mem.get_bytes_fallible_mut(ConstVoidPtr::from_bits(destination),64) else {
            restore(env);
            env.framework_state.opengles.ds_pc_inverse_cache.disabled=true;
            env.cpu.branch(GuestFunction::from_addr_and_thumb_flag(ENTRY,false));
            return true;
        };
        dest.copy_from_slice(&entry.output);
        env.cpu.extregs_mut()[..16].copy_from_slice(&entry.extregs);
        env.cpu.set_fpscr(before_fp|entry.flags);
        let ret=GuestFunction::from_addr_with_thumb_bit(env.cpu.regs()[Cpu::LR]);
        env.cpu.branch(ret);
        let s=&mut env.framework_state.opengles.ds_pc_inverse_cache;
        s.calls+=1;s.hits+=1;report(env);
        return true;
    }

    let ret=GuestFunction::from_addr_with_thumb_bit(env.cpu.regs()[Cpu::LR]);
    restore(env);
    GuestFunction::from_addr_and_thumb_flag(ENTRY,false).call_without_pushing_stack_frame(env);
    let output=bytes(env,destination,64).map(|data|data.try_into().unwrap());
    let after_fp=env.cpu.fpscr();
    let extregs={let ext=env.cpu.extregs_mut();ext[..16].try_into().unwrap()};
    if let Some(output)=output {
        env.framework_state.opengles.ds_pc_inverse_cache.cache=Some(Cache{
            input:key,
            control,
            output,
            flags:after_fp&FPSCR_STICKY,
            extregs,
        });
    }
    if !arm(env) {
        env.framework_state.opengles.ds_pc_inverse_cache.disabled=true;
    }
    env.cpu.branch(ret);
    let s=&mut env.framework_state.opengles.ds_pc_inverse_cache;
    s.calls+=1;s.misses+=1;report(env);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_requires_control_and_all_matrix_bits() {
        let input=[0x1234_5678u32;16];
        let entry=Cache{input,control:0x4000_0000,output:[7;64],flags:0x20,extregs:[9;16]};
        assert!(cache_matches(entry,&input,0x4000_0000));
        assert!(!cache_matches(entry,&input,0x0000_0000));
        let mut changed=input;changed[15]^=1;
        assert!(!cache_matches(entry,&changed,0x4000_0000));
    }
}

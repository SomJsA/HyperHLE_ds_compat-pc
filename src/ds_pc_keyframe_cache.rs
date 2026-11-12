//! Explicit opt-in memoization for the original KeyframeSequence::Sample.
//! No interpolation is reimplemented: a hit replays an earlier original
//! result only when the object state, scalar inputs and FPSCR controls match.
use crate::{abi::GuestFunction,cpu::Cpu,mem::{ConstVoidPtr,MutPtr},Environment};
use std::{path::PathBuf,sync::atomic::Ordering::Relaxed};

const ENTRY:u32=0x558f2c;
const FIRST_WORD:u32=0xe92d40f0;
const BKPT:u32=0xe1200070;
const FINGERPRINT:u64=9480937193432433308;
const STATE_OFFSET:u32=0x40;
const STATE_BYTES:u32=0x40;
const FPSCR_STICKY:u32=0x9f;

#[derive(Clone,Copy)]
struct Cache {
    object:u32,
    time_bits:u32,
    selector:u32,
    control:u32,
    state_before:[u8;64],
    state_after:[u8;64],
    output:[u8;64],
    output_len:u8,
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
fn overlap(a:u32,alen:u32,b:u32,blen:u32)->bool {
    let a1=u64::from(a)+u64::from(alen);let b1=u64::from(b)+u64::from(blen);
    u64::from(a)<b1&&u64::from(b)<a1
}
fn arm(env:&mut Environment)->bool {
    let s=&env.framework_state.opengles.ds_pc_keyframe_cache;
    if s.disabled || !s.configured {return false;}
    if s.armed {return true;}
    if fingerprint(bytes(env,ENTRY,108).unwrap_or(&[]))!=FINGERPRINT || word(env)!=Some(FIRST_WORD) {
        echo!("[DSpc-keyframe-cache] unknown original code; no hook installed");
        return false;
    }
    env.mem.write(MutPtr::<u32>::from_bits(ENTRY),BKPT);
    env.cpu.invalidate_cache_range(ENTRY,4);
    env.framework_state.opengles.ds_pc_keyframe_cache.armed=true;
    true
}
fn restore(env:&mut Environment) {
    if word(env)==Some(BKPT) {env.mem.write(MutPtr::<u32>::from_bits(ENTRY),FIRST_WORD);}
    else {echo!("[DSpc-keyframe-cache] restore conflict; unknown word retained");}
    env.cpu.invalidate_cache_range(ENTRY,4);
    env.framework_state.opengles.ds_pc_keyframe_cache.armed=false;
}
pub(crate) fn prime(env:&mut Environment) {
    if env.bundle.bundle_identifier()!="com.ea.deadspace.bv" {return;}
    if std::env::var("TOUCHHLE_DS_PC_KEYFRAME_CACHE").as_deref()!=Ok("1") {return;}
    let Some(path)=std::env::var_os("TOUCHHLE_DS_PC_KEYFRAME_TRIGGER") else {
        echo!("[DSpc-keyframe-cache] disabled: explicit trigger required");
        return;
    };
    let s=&mut env.framework_state.opengles.ds_pc_keyframe_cache;
    s.configured=true;s.trigger=Some(PathBuf::from(path));
    echo!("[DSpc-keyframe-cache] waiting explicit trigger; exact state replay");
}
pub(crate) fn poll(env:&mut Environment) {
    if !env.framework_state.opengles.ds_pc_keyframe_cache.configured {return;}
    let frame=crate::window::DS_FRAME_COUNTER.load(Relaxed);
    let s=&mut env.framework_state.opengles.ds_pc_keyframe_cache;
    if s.last_frame==frame||s.trigger.is_none(){return;}
    s.last_frame=frame;
    if !s.trigger.as_ref().unwrap().is_file(){return;}
    s.trigger=None;
    if arm(env){echo!("[DSpc-keyframe-cache] armed frame={}; exact memoization only",frame);}
}

struct Args {
    object:u32,
    time_bits:u32,
    selector:u32,
    output:u32,
    output_len:u32,
    state:[u8;64],
    control:u32,
}
fn args(env:&Environment,before_fp:u32)->Option<Args> {
    let regs=env.cpu.regs();
    let object=regs[0];let output=regs[3];
    let state=bytes(env,object.checked_add(STATE_OFFSET)?,STATE_BYTES)?;
    let components=u32::from_le_bytes(state[4..8].try_into().ok()?);
    if !(1..=16).contains(&components)||overlap(object.checked_add(STATE_OFFSET)?,STATE_BYTES,output,components*4){return None;}
    bytes(env,output,components*4)?;
    Some(Args{object,time_bits:regs[1],selector:regs[2],output,output_len:components*4,state:state.try_into().ok()?,control:before_fp&!FPSCR_STICKY})
}
fn matches(cache:Cache,a:&Args)->bool {
    cache.object==a.object&&cache.time_bits==a.time_bits&&cache.selector==a.selector&&cache.control==a.control&&cache.state_before==a.state
}
fn report(env:&mut Environment) {
    let s=&mut env.framework_state.opengles.ds_pc_keyframe_cache;
    if s.calls<=4||s.calls.is_power_of_two(){echo!("[DSpc-keyframe-cache] frame={} calls={} hits={} misses={}; exact replay counts, not CPU time",crate::window::DS_FRAME_COUNTER.load(Relaxed),s.calls,s.hits,s.misses);}
}
pub(crate) fn handle_breakpoint(env:&mut Environment)->bool {
    if !env.framework_state.opengles.ds_pc_keyframe_cache.armed||env.cpu.regs()[Cpu::PC]!=ENTRY||word(env)!=Some(BKPT){return false;}
    if env.cpu.cpsr()&((1<<9)|Cpu::CPSR_THUMB)!=0 {restore(env);env.framework_state.opengles.ds_pc_keyframe_cache.disabled=true;env.cpu.branch(GuestFunction::from_addr_and_thumb_flag(ENTRY,false));return true;}
    let before_fp=env.cpu.fpscr();
    let Some(a)=args(env,before_fp) else {
        let ret=GuestFunction::from_addr_with_thumb_bit(env.cpu.regs()[Cpu::LR]);
        restore(env);
        GuestFunction::from_addr_and_thumb_flag(ENTRY,false).call_without_pushing_stack_frame(env);
        if !arm(env){env.framework_state.opengles.ds_pc_keyframe_cache.disabled=true;}
        env.cpu.branch(ret);
        return true;
    };
    let cached=env.framework_state.opengles.ds_pc_keyframe_cache.cache;
    if cached.is_some_and(|entry|matches(entry,&a)) {
        let entry=cached.unwrap();
        let Some(dest)=env.mem.get_bytes_fallible_mut(ConstVoidPtr::from_bits(a.output),a.output_len) else {restore(env);env.framework_state.opengles.ds_pc_keyframe_cache.disabled=true;env.cpu.branch(GuestFunction::from_addr_and_thumb_flag(ENTRY,false));return true;};
        dest.copy_from_slice(&entry.output[..a.output_len as usize]);
        let Some(state_dest)=env.mem.get_bytes_fallible_mut(ConstVoidPtr::from_bits(a.object+STATE_OFFSET),STATE_BYTES) else {
            restore(env);env.framework_state.opengles.ds_pc_keyframe_cache.disabled=true;
            env.cpu.branch(GuestFunction::from_addr_and_thumb_flag(ENTRY,false));return true;
        };
        state_dest.copy_from_slice(&entry.state_after);
        env.cpu.extregs_mut()[..16].copy_from_slice(&entry.extregs);
        env.cpu.set_fpscr(before_fp|entry.flags);
        let ret=GuestFunction::from_addr_with_thumb_bit(env.cpu.regs()[Cpu::LR]);env.cpu.branch(ret);
        let s=&mut env.framework_state.opengles.ds_pc_keyframe_cache;s.calls+=1;s.hits+=1;report(env);return true;
    }
    let ret=GuestFunction::from_addr_with_thumb_bit(env.cpu.regs()[Cpu::LR]);
    restore(env);
    GuestFunction::from_addr_and_thumb_flag(ENTRY,false).call_without_pushing_stack_frame(env);
    let output=bytes(env,a.output,a.output_len).map(|data|{let mut out=[0;64];out[..data.len()].copy_from_slice(data);out});
    let state_after=bytes(env,a.object+STATE_OFFSET,STATE_BYTES).map(|data|data.try_into().unwrap());
    let extregs={let ext=env.cpu.extregs_mut();ext[..16].try_into().unwrap()};
    let after_fp=env.cpu.fpscr();
    if let (Some(output),Some(state_after))=(output,state_after){env.framework_state.opengles.ds_pc_keyframe_cache.cache=Some(Cache{object:a.object,time_bits:a.time_bits,selector:a.selector,control:a.control,state_before:a.state,state_after,output,output_len:a.output_len as u8,flags:after_fp&FPSCR_STICKY,extregs});}
    if !arm(env){env.framework_state.opengles.ds_pc_keyframe_cache.disabled=true;}
    env.cpu.branch(ret);
    let s=&mut env.framework_state.opengles.ds_pc_keyframe_cache;s.calls+=1;s.misses+=1;report(env);true
}

#[cfg(test)]
mod tests {
    #[test]
    fn sample_cache_control_key_is_separate_from_sticky_flags(){
        let sticky=0x9f;let before=0x4000_009f;assert_eq!(before&!sticky,0x4000_0000);
    }
}

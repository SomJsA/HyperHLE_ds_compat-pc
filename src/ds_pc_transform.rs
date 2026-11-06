//! Explicit original-verified PC matrix fast path. No guest/GL worker sharing.
use crate::{abi::GuestFunction,cpu::Cpu,mem::{ConstVoidPtr,MutPtr},Environment};
use std::{path::PathBuf,sync::atomic::Ordering::Relaxed,time::{Duration,Instant}};
const ENTRY:u32=0x4ea204;
const FIRST_ADD:u32=0xe2802010;
const BKPT:u32=0xe1200070;
const FINGERPRINT:u64=15179455414201691398;

#[derive(Default)]
pub(crate) struct State {
    configured:bool,trigger:Option<PathBuf>,last_frame:u64,armed:bool,disabled:bool,host:bool,
    calls:u64,compared:u64,mismatches:u64,unsupported:u64,host_calls:u64,until_recheck:u16,
    last_report:Option<Instant>,
}
fn bytes(env:&Environment,at:u32,len:u32)->Option<&[u8]> {
    if at<0x1000 || u64::from(at)+u64::from(len)>(1u64<<32) {return None;}
    let data=env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(at),len)?;
    (data.len()==len as usize).then_some(data)
}
fn word(env:&Environment)->Option<u32> {Some(u32::from_le_bytes(bytes(env,ENTRY,4)?.try_into().ok()?))}
fn arm(env:&mut Environment)->bool {
    let s=&env.framework_state.opengles.ds_pc_transform;
    if s.disabled || !s.configured {return false;}
    if s.armed {return true;}
    let hash=bytes(env,ENTRY,108).map(|data|data.iter().fold(0xcbf29ce484222325u64,|h,b|(h^u64::from(*b)).wrapping_mul(0x100000001b3)));
    if hash!=Some(FINGERPRINT)||word(env)!=Some(FIRST_ADD) {
        echo!("[DSpc-transform] unknown original code; no hook installed");return false;
    }
    env.mem.write(MutPtr::<u32>::from_bits(ENTRY),BKPT);env.cpu.invalidate_cache_range(ENTRY,4);
    env.framework_state.opengles.ds_pc_transform.armed=true;true
}
fn restore(env:&mut Environment) {
    if word(env)==Some(BKPT) {env.mem.write(MutPtr::<u32>::from_bits(ENTRY),FIRST_ADD);}
    else {echo!("[DSpc-transform] restore conflict; unknown word retained");}
    env.cpu.invalidate_cache_range(ENTRY,4);env.framework_state.opengles.ds_pc_transform.armed=false;
}
pub(crate) fn prime(env:&mut Environment) {
    if env.bundle.bundle_identifier()!="com.ea.deadspace.bv" {return;}
    let mode=std::env::var("TOUCHHLE_DS_PC_TRANSFORM_MATH").unwrap_or_default();
    if !matches!(mode.as_str(),"host"|"shadow") {return;}
    let Some(path)=std::env::var_os("TOUCHHLE_DS_PC_TRANSFORM_TRIGGER") else {
        echo!("[DSpc-transform] disabled: explicit trigger required");return;
    };
    let s=&mut env.framework_state.opengles.ds_pc_transform;
    s.configured=true;s.host=mode=="host";s.trigger=Some(PathBuf::from(path));
    echo!("[DSpc-transform] waiting explicit trigger; mode={}; original64 matches,periodic256",mode);
}
pub(crate) fn poll(env:&mut Environment) {
    if !env.framework_state.opengles.ds_pc_transform.configured {return;}
    let request=crate::window::DS_PC_TRANSFORM_HOST_REQUEST.swap(-1,Relaxed);
    if request>=0 {
        env.framework_state.opengles.ds_pc_transform.trigger=None;
        if request==0 {
            if env.framework_state.opengles.ds_pc_transform.armed {restore(env);}
            env.framework_state.opengles.ds_pc_transform.host=false;
            echo!("[DSpc-transform] runtime host OFF; original entry restored; no observation");
        } else {
            env.framework_state.opengles.ds_pc_transform.host=true;
            if arm(env) {echo!("[DSpc-transform] runtime host ON");}
        }
    }
    let frame=crate::window::DS_FRAME_COUNTER.load(Relaxed);
    let s=&mut env.framework_state.opengles.ds_pc_transform;
    if s.last_frame==frame || s.trigger.is_none() {return;}
    s.last_frame=frame;
    if !s.trigger.as_ref().unwrap().is_file(){return;}
    s.trigger=None;
    if arm(env){echo!("[DSpc-transform] armed frame={}; exact first ADD replay",frame);}
}
struct Snapshot {destination:u32,a:[f32;16],b:[f32;16]}
fn matrix(env:&Environment,object:u32)->Result<(u32,[f32;16]),&'static str> {
    let at=object.checked_add(16).filter(|v|*v>=0x1000&&*v&3==0).ok_or("matrix-address")?;
    if object<0x1000 {return Err("matrix-object");}
    let data=bytes(env,at,64).ok_or("matrix-unreadable")?;
    Ok((at,std::array::from_fn(|i|f32::from_le_bytes(data[i*4..i*4+4].try_into().unwrap()))))
}
fn capture(env:&Environment,args:[u32;2])->Result<Snapshot,&'static str> {
    let (destination,a)=matrix(env,args[0])?;let (_,b)=matrix(env,args[1])?;
    // Original loads both complete matrices before any store: all guest aliases
    // are valid with these independent fixed snapshots, including object headers.
    Ok(Snapshot{destination,a,b})
}
fn original_after_add(env:&mut Environment) {
    let regs=*env.cpu.regs();
    env.cpu.regs_mut()[2]=regs[0].wrapping_add(16);
    GuestFunction::from_addr_and_thumb_flag(ENTRY+4,false).call_without_pushing_stack_frame(env);
    assert_eq!(env.cpu.regs()[Cpu::SP],regs[Cpu::SP],"Original matrix stack contract changed");
    env.cpu.branch(GuestFunction::from_addr_with_thumb_bit(regs[Cpu::LR]));
}
pub(crate) fn apply_register_outputs(cpu:&mut Cpu,args:[u32;2],a:&[f32;16],b:&[f32;16],output:&[u8;64]) {
    let regs=cpu.regs_mut();
    regs[1]=args[1].wrapping_add(80);regs[2]=args[0].wrapping_add(80);
    regs[3]=args[0].wrapping_add(16);regs[12]=args[0].wrapping_add(80);
    // Also reproduce the original caller-saved SIMD outputs, rather than relying
    // on callers treating them as clobbered. Callee D8-D15 remain unchanged.
    let ext=cpu.extregs_mut();
    for i in 0..16 {
        ext[i]=b[i].to_bits();ext[32+i]=a[i].to_bits();
        ext[48+i]=u32::from_le_bytes(output[i*4..i*4+4].try_into().unwrap());
    }
}
pub(crate) fn handle_breakpoint(env:&mut Environment)->bool {
    if !env.framework_state.opengles.ds_pc_transform.armed || env.cpu.regs()[Cpu::PC]!=ENTRY || word(env)!=Some(BKPT) {return false;}
    if env.cpu.cpsr()&((1<<9)|Cpu::CPSR_THUMB)!=0 {
        restore(env);env.framework_state.opengles.ds_pc_transform.disabled=true;
        env.cpu.branch(GuestFunction::from_addr_and_thumb_flag(ENTRY,false));return true;
    }
    let args=[env.cpu.regs()[0],env.cpu.regs()[1]];let before_fp=env.cpu.fpscr();
    let snapshot=capture(env,args);
    let candidate=snapshot.as_ref().map_err(|e|*e).and_then(|s|crate::ds_pc_transform_math::calculate(&s.a,&s.b));
    let s=&env.framework_state.opengles.ds_pc_transform;
    let accelerate=s.host&&s.compared>=64&&s.until_recheck>0;
    if accelerate {
        if let (Ok(snapshot),Ok(candidate))=(&snapshot,&candidate) {
            let Some(dest)=env.mem.get_bytes_fallible_mut(ConstVoidPtr::from_bits(snapshot.destination),64) else {
                restore(env);env.framework_state.opengles.ds_pc_transform.disabled=true;
                env.cpu.branch(GuestFunction::from_addr_and_thumb_flag(ENTRY,false));return true;
            };
            if dest.len()!=64 {restore(env);env.framework_state.opengles.ds_pc_transform.disabled=true;
                env.cpu.branch(GuestFunction::from_addr_and_thumb_flag(ENTRY,false));return true;}
            dest.copy_from_slice(&candidate.bytes);
            let ret=GuestFunction::from_addr_with_thumb_bit(env.cpu.regs()[Cpu::LR]);
            apply_register_outputs(&mut env.cpu,args,&snapshot.a,&snapshot.b,&candidate.bytes);
            env.cpu.set_fpscr(before_fp|candidate.flags);env.cpu.branch(ret);
            let s=&mut env.framework_state.opengles.ds_pc_transform;
            s.calls+=1;s.host_calls+=1;s.until_recheck-=1;
            report(env);return true;
        }
    }
    original_after_add(env);
    let equal=candidate.as_ref().ok().map(|c| {
        bytes(env,snapshot.as_ref().unwrap().destination,64)==Some(c.bytes.as_slice())&&env.cpu.fpscr()==(before_fp|c.flags)
    });
    let s=&mut env.framework_state.opengles.ds_pc_transform;s.calls+=1;
    match equal {
        Some(true)=>{s.compared+=1;if s.compared>=64&&s.until_recheck==0{s.until_recheck=256;}
            if s.compared==64 {echo!("[DSpc-transform] 64 live output/FPSCR matches; host eligible with256-call rechecks");}}
        Some(false)=>{s.mismatches+=1;s.disabled=true;}
        None=>s.unsupported+=1,
    }
    if equal==Some(false) {report(env);restore(env);echo!("[DSpc-transform] mismatch; disabled for process; original output retained");}
    else {report(env);}
    true
}
fn report(env:&mut Environment) {
    let s=&mut env.framework_state.opengles.ds_pc_transform;
    if s.calls<=4||s.calls.is_power_of_two()||s.calls%4096==0||s.mismatches>0 {
        let now=Instant::now();
        if s.calls>4 && s.mismatches==0 && s.last_report.is_some_and(|t|now.duration_since(t)<Duration::from_secs(2)) {return;}
        s.last_report=Some(now);
        echo!("[DSpc-transform] frame={} calls={} compared={} mismatch={} unsupported={} host_calls={} recheck_remaining={}; counts,notCPUtime",
            crate::window::DS_FRAME_COUNTER.load(Relaxed),s.calls,s.compared,s.mismatches,s.unsupported,s.host_calls,s.until_recheck);
    }
}

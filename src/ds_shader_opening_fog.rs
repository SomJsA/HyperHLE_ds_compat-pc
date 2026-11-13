//! Original-verified GLES2 counterpart of the accepted opening fog transition.
//! Intercept only the native linear-fog Uniform2f call. Original Fog objects,
//! matrices, guest arguments/registers and shader source remain unchanged.
use crate::{cpu::Cpu,mem::ConstVoidPtr,Environment};

struct Guard { name: &'static str, address: u32, words: &'static [u32] }
const GUARDS: &[Guard] = &[
Guard { name: "__ZN3m3g17ShaderRendererAPI25BindFixedFunctionUniformsEPNS_13ShaderBindingERKN2im4gles17ShaderDescriptionERKNS4_16ShaderParametersE", address: 0x568a00, words: &[0xe92d40f0,0xe28d700c,0xe52d8004,0xed2d8b04,0xe5d02351,0xe1a04001,0xe1a05000,0xe3120020,0xa000002,0xe5951388,0xe5940048,0xebfbf472,0xe5940044,0xe2852fe6,0xe3a01001,0xebfbf461,0xed950ae3,0xed951ae4,0xf2000d01,0xe5940054,0xee101a10,0xebfbf468,0xe59503a8,0xe3500000,0xa000037,0xebfe84e7,0xe3700001,0xa000034,0xe59503a8,0xebfe84e3,0xe3500050,0xa000003,0xe3500051,0xa000008,0xe3500052,0x1a000018,0xe5946050,0xe59503a8,0xebfe84c8,0xe1a01000,0xe1a00006,0xebfbf454,0xea000011,0xe59503a8,0xeeb78a00,0xe5946050,0xebfe84c9,0xe59513a8,0xee090a10,0xe1a00001,0xebfe84d0,0xee000a10,0xe59503a8,0xf2290d00,0xee880a00,0xee108a10,0xebfe84bf,0xe1a02000,0xe1a00006,0xe1a01008,0xebfbf43d,0xe59503a8,0xebfe84ae,0xed9f3a13,0xe7e71850,0xe6ef2070,0xe7e70450,0xee002a10,0xee011a10,0xee020a10,0xe594004c,0xf3bb0600,0xf3bb1601,0xf3bb2602,0xf3000d13,0xf3011d13,0xf3022d13,0xee103a10,0xee111a10,0xee122a10,0xebfbf425,0xecbd8b04,0xe49d8004,0xe8bd80f0,0x3b808081] },
Guard { name: "__ZN3m3g17ShaderRendererAPI6SetFogEPKNS_3FogE", address: 0x567c58, words: &[0xe92d40b0,0xe5902350,0xe1a04001,0xe1a05000,0xe28d7008,0xe3c22a0d,0xe5802350,0xe3510000,0xa00001b,0xe5d50394,0xe3500000,0x1a000018,0xe1a00004,0xebfe885d,0xe3700001,0xa000014,0xe1a00004,0xebfe8859,0xe3500050,0xa000008,0xe3500051,0xa000003,0xe3500052,0xa000007,0xe5950350,0xea000008,0xe5950350,0xe3800901,0xea000004,0xe5950350,0xe3800902,0xea000001,0xe5950350,0xe3800903,0xe5850350,0xe3800a01,0xe5850350,0xe58543a8,0xe8bd80b0] },
Guard { name: "__ZN3m3g17ShaderRendererAPI13SetProjectionERKN10Vectormath3Aos7Matrix4E", address: 0x5675e0, words: &[0xe3a02001,0xe5c02118,0xe5912000,0xe58021a0,0xe5912004,0xe58021a4,0xe5912008,0xe58021a8,0xe591200c,0xe58021ac,0xe5912010,0xe58021b0,0xe5912014,0xe58021b4,0xe5912018,0xe58021b8,0xe591201c,0xe58021bc,0xe5912020,0xe58021c0,0xe5912024,0xe58021c4,0xe5912028,0xe58021c8,0xe591202c,0xe58021cc,0xe5912030,0xe58021d0,0xe5912034,0xe58021d4,0xe5912038,0xe58021d8,0xe591103c,0xe58011dc,0xe12fff1e] },
Guard { name: "__ZNK3m3g3Fog14GetFarDistanceEv", address: 0x509de4, words: &[0xe590004c,0xe12fff1e] },
Guard { name: "__ZNK3m3g3Fog15GetNearDistanceEv", address: 0x509e10, words: &[0xe5900048,0xe12fff1e] },
Guard { name: "__ZNK3m3g3Fog7GetModeEv", address: 0x509e08, words: &[0xe5900040,0xe12fff1e] },
Guard { name: "__ZNK3m3g3Fog8GetColorEv", address: 0x509db8, words: &[0xe5900050,0xe12fff1e] },
Guard { name: "__ZN2im4gles9Uniform2fEiff", address: 0x465bec, words: &[0xea116653] }
];

const RETURN_PC:u32=0x0056_8af4;
#[derive(Default)]
pub(crate) struct State {
    armed:bool,fix:bool,trace:bool,corrected:u64,logs:u32,
    previous:Option<(u32,u32,u32,bool)>,last_error:Option<&'static str>,
}
fn word(env:&Environment,at:u32)->Option<u32> {
    Some(u32::from_le_bytes(env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(at),4)?.try_into().ok()?))
}
fn matches(mut symbol:impl FnMut(&str)->Option<u32>,mut read:impl FnMut(u32)->Option<u32>)->bool {
    GUARDS.iter().all(|g|symbol(g.name)==Some(g.address) && g.words.iter().enumerate().all(|(i,v)|read(g.address+i as u32*4)==Some(*v)))
}
pub(crate) fn prime(env:&mut Environment) {
    if env.bundle.bundle_identifier()!="com.ea.deadspace.bv" {return;}
    let fix=std::env::var("TOUCHHLE_DS_OPENING_FOG_FIX").as_deref()!=Ok("0");
    let trace=std::env::var("TOUCHHLE_DS_ES2_OPENING_FOG_TRACE").as_deref()==Ok("1");
    if !fix && !trace {return;}
    if !matches(|s|env.bins[0].exported_symbols.get(s).copied(),|a|word(env,a)) {
        echo!("[DSes2-opening-fog] unknown original code; unchanged");return;
    }
    let s=&mut env.framework_state.opengles.ds_shader_opening_fog;
    s.armed=true;s.fix=fix;s.trace=trace;
    echo!("[DSes2-opening-fog] original call verified; fix={} trace={}",fix,trace);
}
#[derive(Debug)]
struct Input {renderer:u32,start:f32,end:f32,enabled:bool}
fn input(regs:&[u32;16],location:i32,x:f32,y:f32,mut read:impl FnMut(u32)->Option<u32>)->Result<Input,&'static str> {
    if regs[Cpu::LR]!=RETURN_PC {return Err("caller");}
    let renderer=regs[5];let binding=regs[4];
    let mut field=|p:u32,o:u32|p.checked_add(o).and_then(&mut read);
    if field(binding,0x50)!=Some(location as u32) {return Err("binding");}
    let fog=field(renderer,0x3a8).filter(|v|*v!=0).ok_or("fog-pointer")?;
    if field(fog,0x40)!=Some(0x51) {return Err("mode");}
    if field(fog,0x50).ok_or("color")? & 0x00ff_ffff !=0 {return Err("color");}
    let p11=f32::from_bits(field(renderer,0x1cc).ok_or("projection")?);
    let p14=f32::from_bits(field(renderer,0x1d8).ok_or("projection")?);
    let p15=f32::from_bits(field(renderer,0x1dc).ok_or("projection")?);
    if ![p11,p14,p15].iter().all(|v|v.is_finite()) || (p11+1.).abs()>0.001 || p15.abs()>0.001 || !(-0.105..=-0.095).contains(&p14) {return Err("projection");}
    let start=f32::from_bits(field(fog,0x48).ok_or("range")?);
    let end=f32::from_bits(field(fog,0x4c).ok_or("range")?);
    if !start.is_finite() || !end.is_finite() || start<0. || end<0. || y.to_bits()!=end.to_bits() {return Err("range");}
    let expected=1f32/(end-start);
    if x.to_bits()!=expected.to_bits() && !(x.is_finite() && expected.is_finite() && (x-expected).abs()<=expected.abs().max(1.)*0.000001) {return Err("payload");}
    let flags=field(renderer,0x350).ok_or("flags")?;
    let enabled=flags&0xd000==0x5000 && field(renderer,0x394).ok_or("flags")?&255==0;
    Ok(Input{renderer,start,end,enabled})
}
pub(crate) fn parameters(env:&mut Environment,location:i32,x:f32,y:f32)->(f32,f32) {
    if env.cpu.regs()[Cpu::LR]!=RETURN_PC || !env.framework_state.opengles.ds_shader_opening_fog.armed {return (x,y);}
    let regs=*env.cpu.regs();
    let input=match input(&regs,location,x,y,|a|word(env,a)) {
        Ok(v)=>v,Err(reason)=> {
            let s=&mut env.framework_state.opengles.ds_shader_opening_fog;
            if s.trace && s.last_error!=Some(reason) && s.logs<128 {s.logs+=1;echo!("[DSes2-opening-fog] skip={}",reason);}
            s.last_error=Some(reason);return (x,y);
        }
    };
    let frame=crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    let fixed=crate::frameworks::opengles::shader_opening_fog_range(env,input.renderer,frame,input.start,input.end);
    let s=&mut env.framework_state.opengles.ds_shader_opening_fog;
    let corrected=s.fix && input.enabled && location>=0 && fixed.is_some();
    let result=if corrected {let (a,b)=fixed.unwrap();(1./(b-a),b)}else{(x,y)};
    let key=(input.renderer,input.start.to_bits(),input.end.to_bits(),corrected);
    if s.trace && s.previous!=Some(key) && s.logs<128 {
        s.logs+=1;
        echo!("[DSes2-opening-fog] frame={} renderer={:#x} range={}..{} original={:?} corrected={} effective={:?}",frame,input.renderer,input.start,input.end,(x,y),corrected,result);
    }
    s.previous=Some(key);s.last_error=None;
    if corrected {s.corrected=s.corrected.saturating_add(1);if s.corrected==1 {echo!("[DSes2-opening-fog] preserving original opening range 10..25");}}
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn fixture(start:f32,end:f32)->([u32;16],BTreeMap<u32,u32>) {
        let mut r=[0;16];r[4]=0x2000;r[5]=0x1000;r[Cpu::LR]=RETURN_PC;
        let m=BTreeMap::from([(0x2050,3),(0x13a8,0x3000),(0x3040,0x51),(0x3050,0xff000000),(0x3048,start.to_bits()),(0x304c,end.to_bits()),(0x11cc,(-1f32).to_bits()),(0x11d8,(-0.1f32).to_bits()),(0x11dc,0),(0x1350,0x5000),(0x1394,0)]);
        (r,m)
    }
    #[test]
    fn original_guard_checks_every_word_and_symbol() {
        let mut m=BTreeMap::new();for g in GUARDS {for(i,w)in g.words.iter().enumerate(){m.insert(g.address+i as u32*4,*w);}}
        let sym=|s:&str|GUARDS.iter().find(|g|g.name==s).map(|g|g.address);
        assert!(matches(sym,|a|m.get(&a).copied()));
        // Original GetColor is LDR r0,[r0,#0x50]; the adjacent field is not color.
        assert_eq!(GUARDS.iter().find(|g|g.address==0x509db8).unwrap().words[0],0xe5900050);
        for g in GUARDS {for(i,w)in g.words.iter().enumerate(){let a=g.address+i as u32*4;m.insert(a,w^1);assert!(!matches(sym,|p|m.get(&p).copied()));m.insert(a,*w);}}
        assert!(!matches(|_|None,|a|m.get(&a).copied()));
    }
    #[test]
    fn only_original_linear_black_perspective_payload_is_eligible() {
        let (r,m)=fixture(0.0016622568,0.004155642);let original=m.clone();let x=1./(0.004155642-0.0016622568);
        let i=input(&r,3,x,0.004155642,|a|m.get(&a).copied()).unwrap();assert!(i.enabled);assert_eq!(m,original);
        for (address,value) in [(0x3040,0x50),(0x3050,1),(0x11cc,0),(0x11d8,(-0.2f32).to_bits()),(0x11dc,1f32.to_bits()),(0x2050,4)] {
            let mut bad=m.clone();bad.insert(address,value);assert!(input(&r,3,x,0.004155642,|a|bad.get(&a).copied()).is_err());
        }
        let mut wrong=r;wrong[Cpu::LR]+=4;assert!(input(&wrong,3,x,0.004155642,|a|m.get(&a).copied()).is_err());
        assert!(input(&r,3,x+1.,0.004155642,|a|m.get(&a).copied()).is_err());
    }
    #[test]
    fn opening_sequence_is_context_and_renderer_owned() {
        let mut a=crate::gles::ds_fog_scope::GuestState::default();let mut b=crate::gles::ds_fog_scope::GuestState::default();
        assert_eq!(a.opening_range(7,1,10.,25.),None);assert_eq!(a.opening_range(7,2,0.,0.),None);
        assert_eq!(b.opening_range(7,3,0.04,0.1),None);assert_eq!(a.opening_range(8,3,0.04,0.1),None);
        assert_eq!(a.opening_range(7,3,0.04,0.1),Some((10.,25.)));
        assert_eq!(a.opening_range(7,4,10.,25.),None);assert_eq!(a.opening_range(7,5,0.04,0.1),None);
    }
}

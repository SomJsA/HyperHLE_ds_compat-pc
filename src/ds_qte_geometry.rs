//! Native HUD geometry for PC QTE labels. No framebuffer reads or game/input
//! writes. One verified MOV is observed/replayed in process memory only.
use crate::{abi::{CallFromHost,GuestFunction},cpu::Cpu,mem::{ConstVoidPtr,MutPtr},Environment};
#[path="ds_qte_geometry_bounds.rs"] mod bounds;
pub(crate) use bounds::Bounds;
const SITE:u32=0x0018_bdfc;
const MOV_R0_R4:u32=0xe1a0_0004;
const BKPT:u32=0xe120_0070;
const BL_TRANSLATE:u32=0xeb00_0000|((0x0048_d400-(SITE+12))/4);

#[derive(Default)] pub(crate) struct State {
    armed:bool,
    swipe:Option<(u32,u64,Bounds)>,
    tap:Option<(u32,u32,u32,u32,Bounds)>,
    logged:bool,
    last_stage:Option<(u32,u32,u32)>,
}
fn word(env:&Environment,address:u32)->Option<u32>{
    let b=env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(address),4)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}
fn screen(env:&Environment,hud:u32)->Option<(u32,u32)>{
    let renderer=word(env,hud.checked_add(8)?)?;
    if renderer==0{return None;}
    let w=word(env,renderer.checked_add(8)?)?;
    let h=word(env,renderer.checked_add(12)?)?;
    if !(64..=4096).contains(&w)||!(64..=4096).contains(&h){return None;}
    Some((w,h))
}
pub(crate) fn prime(env:&mut Environment){
    if !cfg!(target_os="windows") || std::env::var_os("TOUCHHLE_DS_QTE_OVERLAY").is_none()
        || env.bundle.bundle_identifier()!="com.ea.deadspace.bv" {return;}
    if word(env,SITE)!=Some(MOV_R0_R4)||word(env,SITE+4)!=Some(BL_TRANSLATE)
        || word(env,0x0018_bd20)!=Some(0xe92d_40f0)
        || word(env,0x0018_bd1c)!=Some(175.0f32.to_bits())
        || word(env,0x004a_a958)!=Some(0xe92d_40b0)
        || word(env,0x0048_c344)!=Some(0xe591_c004) {
        echo!("[DSqte-geometry] unknown native instructions; keeping original QTE glyphs, no pixel scan");return;
    }
    env.mem.write(MutPtr::<u32>::from_bits(SITE),BKPT);
    env.cpu.invalidate_cache_range(SITE,4);
    env.framework_state.opengles.ds_qte_geometry.armed=true;
    echo!("[DSqte-geometry] native HUD memory/render-parameter path armed; no framebuffer scan");
}
pub(crate) fn handle_breakpoint(env:&mut Environment)->bool{
    if !env.framework_state.opengles.ds_qte_geometry.armed||env.cpu.regs()[Cpu::PC]!=SITE{return false;}
    let _geometry_span=crate::ds_stutter_probe::Span::new(16,SITE as u64);
    let regs=*env.cpu.regs();
    // Replay MOV r0,r4 exactly; all other registers and flags stay untouched.
    env.cpu.regs_mut()[0]=regs[4];env.cpu.regs_mut()[Cpu::PC]=SITE+4;
    // r7+4 is RenderHUDSwipe's saved original LR. Other users (stomp/slash)
    // remain handled by their existing native HUD fields.
    if word(env,regs[7].wrapping_add(4))!=Some(0x0004_02b0){return true;}
    let hud=regs[5];
    let capture=(||{
        // RenderGrapple keeps its player in r4, saved by RenderHUDSwipe at
        // r7-12. Do not publish an old animation's hint after leaving state9.
        let player=word(env,regs[7].checked_sub(12)?)?;
        if word(env,player)!=Some(0x00a7_a4f8)||word(env,player.checked_add(0x208)?)!=Some(9){return None;}
        if regs[8]>>24==0{return None;}
        let (sw,sh)=screen(env,hud)?;
        let image=word(env,hud.checked_add(0x1a4)?)?;
        if image==0{return None;}
        let w=word(env,image.checked_add(0x18)?)? as f32;
        let h=word(env,image.checked_add(0x1c)?)? as f32;
        let angle=f32::from_bits(word(env,regs[7].checked_add(8)?)?);
        let scale=f32::from_bits(word(env,regs[7].checked_add(12)?)?);
        bounds::swipe(f32::from_bits(regs[1]),f32::from_bits(regs[2]),w,h,scale,angle,sw as f32,sh as f32)
    })();
    let frame=crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    env.framework_state.opengles.ds_qte_geometry.swipe=capture.map(|b|(hud,frame,b));
    if let Some(b)=capture {
        // Render callback supplies the current, already-clamped native anchor
        // before the original DrawImage. This can publish in the same frame.
        crate::ds_qte_overlay::publish(Some(crate::ds_qte_overlay::Prompt{x:b.x,y:b.y,width:b.width,height:b.height,
            kind:crate::ds_qte_overlay::PromptKind::Swipe,secondary:None}));
        if !env.framework_state.opengles.ds_qte_geometry.logged {
            env.framework_state.opengles.ds_qte_geometry.logged=true;
            echo!("[DSqte-geometry] native swipe hud={hud:#x} center=({:.2},{:.2}) size=({:.2},{:.2})",b.x,b.y,b.width,b.height);
        }
    }
    true
}
pub(crate) fn observe_stage(env:&mut Environment,player:u32,state:u32,grapple:u32){
    if !crate::ds_diagnostics::enabled(){return;}
    let stage=if state==9 {grapple}else{u32::MAX};
    let value=(player,state,stage);
    if env.framework_state.opengles.ds_qte_geometry.last_stage!=Some(value){
        env.framework_state.opengles.ds_qte_geometry.last_stage=Some(value);
        crate::ds_diagnostics::event("native_qte_stage",[state as u64,stage as u64,crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),0]);
    }
}
pub(crate) fn swipe(env:&Environment,hud:u32)->Option<Bounds>{
    let state=&env.framework_state.opengles.ds_qte_geometry;
    let (owner,frame,b)=state.swipe?;
    let now=crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    (state.armed&&owner==hud&&now.saturating_sub(frame)<=2).then_some(b)
}
pub(crate) fn tap(env:&mut Environment,hud:u32)->Option<Bounds>{
    let _geometry_span=crate::ds_stutter_probe::Span::new(15,hud as u64);
    if !env.framework_state.opengles.ds_qte_geometry.armed{return None;}
    let layout=word(env,hud.checked_add(0x1d0)?)?;
    if layout==0{return None;}
    let (sw,sh)=screen(env,hud)?;
    if let Some((owner,l,w,h,b))=env.framework_state.opengles.ds_qte_geometry.tap {
        if (owner,l,w,h)==(hud,layout,sw,sh){return Some(b);}
    }
    // RenderHUDTap names this native layout entity at0x8ee464. GetEntityRect
    // returns a four-float Rect through AAPCS's hidden result pointer. Its
    // parameters are originX/Y and native screen width/height. RenderHUDTap
    // uses175*tierFudge for originX (not zero), and0 for originY.
    if word(env,0x008e_e464)!=Some('I' as u32){return None;}
    let application:u32=GuestFunction::from_addr_and_thumb_flag(0x0000_b688,false).call_from_host(env,());
    let tier:f32=GuestFunction::from_addr_and_thumb_flag(0x0000_67e4,false).call_from_host(env,(application,));
    if !tier.is_finite()||!(0.1..=4.0).contains(&tier){return None;}
    let origin_x=f32::from_bits(word(env,0x0018_bd1c)?)*tier;
    let output=env.mem.alloc(16);
    for i in 0..4 {env.mem.write(MutPtr::<u32>::from_bits(output.to_bits()+4*i),0);}
    let _:()=GuestFunction::from_addr_and_thumb_flag(0x004a_a958,false).call_from_host(env,
        (output.to_bits(),layout,0x008e_e464u32,origin_x,0.0f32,sw as f32,sh as f32));
    let values=std::array::from_fn::<_,4,_>(|i|f32::from_bits(word(env,output.to_bits()+4*i as u32).unwrap_or(f32::NAN.to_bits())));
    env.mem.free(output);
    let b=bounds::rect(values[0],values[1],values[2],values[3],sw as f32,sh as f32)?;
    env.framework_state.opengles.ds_qte_geometry.tap=Some((hud,layout,sw,sh,b));
    echo!("[DSqte-geometry] native tap hud={hud:#x} center=({:.2},{:.2}) size=({:.2},{:.2})",b.x,b.y,b.width,b.height);
    Some(b)
}

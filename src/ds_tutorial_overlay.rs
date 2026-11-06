//! PC tutorial drawing takeover. Read the initialized TutorialAnim fields each
//! frame, so retries, repeated IDs and native completion keep their own timing.
//! Android deliberately retains the touch text and original gesture artwork.
use crate::abi::{CallFromHost,GuestFunction};
use crate::ds_chapter_overlay::{patch,wide,word};
use crate::Environment;
use std::sync::{Mutex,OnceLock,atomic::{AtomicBool,Ordering}};
const DRAW_SITE:u32=0x002d_6350;
const ORIGINAL:u32=0xebff_ab87;
const NOP:u32=0xe1a0_0000;
static READY:AtomicBool=AtomicBool::new(false);
#[derive(Default)]
pub(crate) struct State { checked:bool,supported:bool,suppressed:bool,last:Option<(u32,u32,String)> }
#[derive(Clone)]
pub(crate) struct Snapshot {pub title:String,pub body:String}
fn snapshot()-> &'static Mutex<Option<Snapshot>> {static SNAP:OnceLock<Mutex<Option<Snapshot>>>=OnceLock::new();SNAP.get_or_init(||Mutex::new(None))}
pub(crate) fn host_snapshot()->Option<Snapshot>{snapshot().lock().ok().and_then(|s|s.clone())}
pub(crate) fn host_ready(ready:bool){READY.store(ready,Ordering::Release);}
fn eligible(phase:u32,draw:u8,hud:u32,input:u32,chapter:u32)->bool {
    matches!(phase,1..=3)&&draw!=0&&!matches!(hud,2..=14)&&input==0&&chapter==0
}
pub(crate) fn observe(env:&mut Environment) {
    if cfg!(target_os="android") || std::env::var_os("TOUCHHLE_DS_TUTORIAL_OVERLAY").is_none(){return;}
    if !env.framework_state.opengles.ds_tutorial.checked {
        let supported=env.bundle.bundle_identifier().to_ascii_lowercase().contains("deadspace")
            && word(env,DRAW_SITE)==Some(ORIGINAL)
            && word(env,0x002d_633c)==Some(0xe92d_4080)
            && word(env,0x002d_6340)==Some(0xe5d0_20ec);
        let state=&mut env.framework_state.opengles.ds_tutorial;state.checked=true;state.supported=supported;
        echo!("[DStutorial-hook] supported={supported}; native input/update retained");
    }
    if !env.framework_state.opengles.ds_tutorial.supported {return;}
    let hud:u32=GuestFunction::from_addr_and_thumb_flag(0x0003_0474,false).call_from_host(env,());
    let world=word(env,hud.saturating_add(4)).unwrap_or(0);
    let object=hud.saturating_add(0x2e8);
    let phase=word(env,object.saturating_add(0x8c)).unwrap_or(0);
    let draw=env.mem.get_bytes_fallible(crate::mem::ConstVoidPtr::from_bits(object.saturating_add(0xec)),1).and_then(|v|v.first()).copied().unwrap_or(0);
    // InitTutorial stores its title/text IDs in +d0/+bc. Accept either order
    // only after semantic validation against the shipped translation table.
    let a=wide(env,object.saturating_add(0xd0));let b=wide(env,object.saturating_add(0xbc));
    let mut title=None;let mut body=None;
    for id in [a,b].into_iter().flatten() {
        if id.starts_with("TUTORIAL_") && crate::ds_subtitle_overlay::text_entry(&id).is_some() {
            if id.ends_with("_TITLE"){title=Some(id);}else{body=Some(id);}
        }
    }
    let desired=hud>=0x1000&&world>=0x1000&&title.is_some()&&body.is_some()&&READY.load(Ordering::Acquire)
        &&eligible(phase,draw,word(env,hud.saturating_add(0x234)).unwrap_or(u32::MAX),word(env,hud.saturating_add(0x240)).unwrap_or(u32::MAX),word(env,hud.saturating_add(0xdb4)).unwrap_or(1));
    let current=env.framework_state.opengles.ds_tutorial.suppressed;
    let valid=word(env,DRAW_SITE)==Some(if current{NOP}else{ORIGINAL});
    if valid && current!=desired {patch(env,DRAW_SITE,if desired{NOP}else{ORIGINAL});env.framework_state.opengles.ds_tutorial.suppressed=desired;}
    let value=if desired&&valid{Some(Snapshot{title:title.unwrap(),body:body.unwrap()})}else{None};
    let key=value.as_ref().map(|s|(world,phase,s.body.clone()));
    if env.framework_state.opengles.ds_tutorial.last!=key {
        echo!("[DStutorial-view] {:?}",key);env.framework_state.opengles.ds_tutorial.last=key;
    }
    if let Ok(mut snap)=snapshot().lock(){*snap=value;}
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn hidden_paused_finished_or_chapter_owned_tutorials_restore_native(){
        for p in 1..=3 {assert!(eligible(p,1,0,0,0));assert!(!eligible(p,0,0,0,0));assert!(!eligible(p,1,2,0,0));assert!(!eligible(p,1,0,1,0));assert!(!eligible(p,1,0,0,2));}
        assert!(!eligible(0,1,0,0,0));assert!(!eligible(4,1,0,0,0));
    }
}

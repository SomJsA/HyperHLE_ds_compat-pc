//! Ordered native weapon-mode selection and firing. No guest calls under locks.
use std::{collections::VecDeque,sync::Mutex};
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum Source {Mouse,Primary,Secondary,Diagnostic}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum Input {Set(bool),Toggle,Hold(bool),Fire(Source,Option<bool>),Release(Source),Cancel}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum Effect {Stop,Tilt,Fire}
#[derive(Clone,Copy,Debug)] pub struct Snapshot {pub player:u32,pub weapon:u32,pub alt:bool,pub has_mode:bool,pub can_tilt:bool,pub can_fire:bool}
#[derive(Default)] pub struct State {owner:Option<(u32,u32)>,target:Option<bool>,active:Option<Source>,pending:bool,held_mode:bool,restore:Option<(u32,u32)>,tilt_frame:Option<u64>}
impl State {
 pub const fn new()->Self {Self{owner:None,target:None,active:None,pending:false,held_mode:false,restore:None,tilt_frame:None}}
 fn suspend(&mut self){if self.held_mode {self.restore=self.owner;}self.held_mode=false;self.target=None;self.active=None;self.pending=false;}
 fn target(&mut self,value:bool,s:Snapshot,effects:&mut Vec<Effect>){
  self.restore=None;
  if !s.has_mode{return;}
  self.target=Some(value);
  if value!=s.alt && self.active.is_some(){effects.push(Effect::Stop);self.pending=true;}
 }
 pub fn update(&mut self,snapshot:Option<Snapshot>,inputs:impl IntoIterator<Item=Input>,frame:u64)->Vec<Effect>{
  let Some(s)=snapshot else {self.suspend();return vec![]};let token=(s.player,s.weapon);
  if self.owner!=Some(token){let held=self.held_mode&&self.owner.is_some_and(|p|p.0==s.player);*self=Self{owner:Some(token),held_mode:held,target:held.then_some(true),..Self::new()};}
  if self.restore==Some(token){self.target=Some(false);}
  let mut out=Vec::new();
  for input in inputs {match input {
   Input::Set(value)=>self.target(value,s,&mut out),
   Input::Toggle=>self.target(!self.target.unwrap_or(s.alt),s,&mut out),
   Input::Hold(down)=>{let owe_primary=!down&&self.held_mode;self.held_mode=down&&s.has_mode;self.target(down,s,&mut out);if owe_primary{self.restore=Some(token);}},
   Input::Fire(source,mode)=>{self.active=Some(source);self.pending=true;if let Some(value)=mode {self.target(value,s,&mut out);}},
   Input::Release(source)=>{if self.active==Some(source){self.active=None;self.pending=false;out.push(Effect::Stop);}},
   Input::Cancel=>{self.suspend();out.push(Effect::Stop);if self.restore==Some(token){self.target=Some(false);}},
  }}
  if self.target.is_some_and(|target|target!=s.alt) && s.has_mode {
   if s.can_tilt && self.tilt_frame!=Some(frame){out.push(Effect::Tilt);self.tilt_frame=Some(frame);}
   return out;
  }
  // A completed toggle belongs to the native weapon again. Keeping the host
  // target latched would immediately undo X or another original mode action.
  // Hold-space deliberately retains its target until release.
  if s.can_fire && !self.held_mode {self.target=None;if !s.alt{self.restore=None;}}
  // glClear may run more than once per presented frame. The native update
  // must consume mode selection before a queued first shot can be dispatched.
  if self.pending && self.active.is_some() && s.can_fire && self.tilt_frame.is_none_or(|f|frame>f){out.push(Effect::Fire);self.pending=false;}
  out
 }
}
struct Shared {queue:VecDeque<Input>,state:State}
static SHARED:Mutex<Shared>=Mutex::new(Shared{queue:VecDeque::new(),state:State::new()});
pub fn enqueue(input:Input){let mut s=SHARED.lock().unwrap();if input==Input::Cancel{s.queue.clear();}if s.queue.len()<64{s.queue.push_back(input);}}
pub fn tick(snapshot:Option<Snapshot>,frame:u64)->Vec<Effect>{let mut s=SHARED.lock().unwrap();let inputs:Vec<_>=s.queue.drain(..).collect();s.state.update(snapshot,inputs,frame)}
#[cfg(test)] mod tests {
 use super::*;
 fn snap(alt:bool)->Snapshot{Snapshot{player:1,weapon:2,alt,has_mode:true,can_tilt:true,can_fire:true}}
 #[test]fn canceled_hold_restoration_survives_a_menu(){let mut p=State::new();p.update(Some(snap(false)),[Input::Hold(true)],1);let lowered=Snapshot{can_tilt:false,can_fire:false,..snap(true)};p.update(Some(lowered),[Input::Cancel],2);p.update(None,[],3);assert_eq!(p.update(Some(snap(true)),[],4),[Effect::Tilt]);assert!(p.update(Some(snap(false)),[],5).is_empty());}
 #[test]fn released_hold_restoration_survives_a_menu(){let mut p=State::new();p.update(Some(snap(false)),[Input::Hold(true)],1);let lowered=Snapshot{can_tilt:false,can_fire:false,..snap(true)};p.update(Some(lowered),[Input::Hold(false)],2);p.update(None,[],3);assert_eq!(p.update(Some(snap(true)),[],4),[Effect::Tilt]);}
 #[test]fn held_modifier_follows_weapon_without_transferring_fire(){let mut p=State::new();p.update(Some(snap(false)),[Input::Hold(true),Input::Fire(Source::Mouse,None)],1);assert_eq!(p.update(Some(Snapshot{weapon:3,..snap(false)}),[],2),[Effect::Tilt]);assert!(p.update(Some(Snapshot{weapon:3,..snap(true)}),[],3).is_empty());}
 #[test]fn native_rotation_can_override_a_completed_toggle(){let mut p=State::new();p.update(Some(snap(false)),[Input::Toggle],1);p.update(Some(snap(true)),[],2);assert!(p.update(Some(snap(false)),[],3).is_empty());}
 #[test]fn selection_never_fires(){let mut p=State::new();assert_eq!(p.update(Some(snap(false)),[Input::Toggle],1),[Effect::Tilt]);assert!(p.update(Some(snap(true)),[],2).is_empty());}
 #[test]fn split_fire_waits_for_native_mode_and_next_frame(){let mut p=State::new();assert_eq!(p.update(Some(snap(false)),[Input::Fire(Source::Secondary,Some(true))],1),[Effect::Stop,Effect::Tilt]);assert!(p.update(Some(snap(true)),[],1).is_empty());assert_eq!(p.update(Some(snap(true)),[],2),[Effect::Fire]);}
 #[test]fn release_during_rotation_cancels_delayed_shot(){let mut p=State::new();p.update(Some(snap(false)),[Input::Fire(Source::Mouse,Some(true))],1);p.update(Some(snap(false)),[Input::Release(Source::Mouse)],2);assert!(p.update(Some(snap(true)),[],3).is_empty());}
 #[test]fn quick_toggle_pair_cancels_unapplied_mode(){let mut p=State::new();assert!(p.update(Some(snap(false)),[Input::Toggle,Input::Toggle],1).is_empty());}
 #[test]fn holding_space_restores_primary(){let mut p=State::new();p.update(Some(snap(false)),[Input::Hold(true)],1);assert_eq!(p.update(Some(snap(true)),[Input::Hold(false)],2),[Effect::Tilt]);assert!(p.update(Some(snap(false)),[],3).is_empty());}
 #[test]fn mode_less_weapon_can_still_fire(){let mut p=State::new();let s=Snapshot{has_mode:false,..snap(false)};assert_eq!(p.update(Some(s),[Input::Toggle,Input::Fire(Source::Secondary,Some(true))],1),[Effect::Fire]);}
 #[test]fn releasing_an_older_source_does_not_stop_latest(){let mut p=State::new();p.update(Some(snap(false)),[Input::Fire(Source::Primary,None),Input::Fire(Source::Secondary,None)],1);assert!(p.update(Some(snap(false)),[Input::Release(Source::Primary)],2).is_empty());assert_eq!(p.update(Some(snap(false)),[Input::Release(Source::Secondary)],3),[Effect::Stop]);}
 #[test]fn new_weapon_does_not_inherit_delayed_fire(){let mut p=State::new();p.update(Some(snap(false)),[Input::Fire(Source::Mouse,Some(true))],1);assert!(p.update(Some(Snapshot{weapon:3,..snap(true)}),[],2).is_empty());}
 #[test]fn lost_focus_releases_space_without_firing_on_resume(){let mut p=State::new();p.update(Some(snap(false)),[Input::Hold(true)],1);p.update(None,[],2);assert_eq!(p.update(Some(snap(true)),[],3),[Effect::Tilt]);assert!(p.update(Some(snap(false)),[],4).is_empty());}
 #[test]fn reload_blocks_selection_and_shooting(){let mut p=State::new();let busy=Snapshot{can_tilt:false,can_fire:false,..snap(false)};assert_eq!(p.update(Some(busy),[Input::Fire(Source::Mouse,Some(true))],1),[Effect::Stop]);assert_eq!(p.update(Some(snap(false)),[],2),[Effect::Tilt]);assert_eq!(p.update(Some(snap(true)),[],3),[Effect::Fire]);}
 #[test]fn repeated_glclear_cannot_duplicate_tilt(){let mut p=State::new();p.update(Some(snap(false)),[Input::Toggle],1);assert!(p.update(Some(snap(false)),[],1).is_empty());}
}

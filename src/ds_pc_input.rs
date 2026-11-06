//! Native PC input policy; raw input is retained across aim transitions.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input { pub x: f32, pub y: f32, pub look_x: f32, pub look_y: f32 }
impl Input { pub const fn new() -> Self { Self { x: 0.0, y: 0.0, look_x: 0.0, look_y: 0.0 } } }

/// Original EnterStore/EnterBench use 19, station UI uses 20/21;
/// UpgradeRig enters 22 and ExitRigUpgrade enters 24. These native sequences
/// own both motion and camera, including when no modal widget is being drawn.
pub fn station_owns_input(state:u32)->bool { (19..=24).contains(&state) }

/// Native look vectors remain latched in the guest until explicitly neutralized.
#[derive(Default)]
pub struct NativeLook {
    last: Option<(u32, u32, u64)>, // player, event type, presented frame
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookEvent { pub kind: u32, pub x: f32, pub y: f32 }
impl NativeLook {
    pub fn cancel(&mut self, player: u32) -> Option<LookEvent> {
        self.last.take().filter(|last| last.0 == player)
            .map(|last| LookEvent { kind: last.1, x: 0.0, y: 0.0 })
    }
    pub fn events(&mut self, frame: u64, player: u32, kind: u32, x: f32, y: f32) -> [Option<LookEvent>; 2] {
        if self.last.is_some_and(|last| last.0 != player) { self.last = None; }
        let active = x.is_finite() && y.is_finite() && (x.abs() > 0.0001 || y.abs() > 0.0001);
        if active {
            let stop = if self.last.is_some_and(|last| last.1 != kind) { self.cancel(player) } else { None };
            self.last = Some((player, kind, frame));
            [stop, Some(LookEvent { kind, x: x.clamp(-4096.0, 4096.0), y: y.clamp(-4096.0, 4096.0) })]
        } else if self.last.is_some_and(|last| last.2 != frame) {
            [self.cancel(player), None]
        } else { [None, None] }
    }
}
/// Physical keys retain their own press order, including WASD/arrow aliases.
/// Later opposing presses win; releasing them resumes a still-held earlier key.
#[derive(Default)]
pub struct HeldDirections { order: [u64; 8], next: u64 }
impl HeldDirections {
    pub fn new() -> Self { Self::default() }
    pub fn update(&mut self, key: usize, down: bool) -> (f32, f32) {
        assert!(key < self.order.len());
        if down && self.order[key] == 0 {
            self.next += 1;
            self.order[key] = self.next;
        } else if !down { self.order[key] = 0; }
        self.vector()
    }
    pub fn vector(&self) -> (f32, f32) {
        let axis = |negative: [usize;2], positive: [usize;2]| {
            let n = self.order[negative[0]].max(self.order[negative[1]]);
            let p = self.order[positive[0]].max(self.order[positive[1]]);
            if n > p { -1.0 } else if p > n { 1.0 } else { 0.0 }
        };
        // W/A/S/D = 0..4, Up/Left/Down/Right = 4..8.
        (axis([1,5],[3,7]), axis([0,4],[2,6]))
    }
    pub fn clear(&mut self) { *self = Self::new(); }
}
pub const DEFAULT_AIM_WALK_LIMIT: f32 = 1.0;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Movement { pub x: f32, pub y: f32, pub sprint: bool, pub limited: bool }
pub fn movement(x: f32, y: f32, aiming: bool, limit: f32) -> Movement {
    if !x.is_finite() || !y.is_finite() { return Movement { x:0.0,y:0.0,sprint:false,limited:false }; }
    let length=x.hypot(y); let scale=1.0/length.max(1.0); let (mut x,mut y)=(x*scale,y*scale);
    let limited=aiming && y < 0.0;
    // Native TouchDPad normalizes a below-sprint drag to length 1 and sends
    // a separate sprint flag. Length 1 with sprint=false is maximum walking,
    // not sprinting. Retain small analogue input and allow a slower user cap.
    if limited { let cap=if limit.is_finite(){limit.clamp(0.05,1.0)}else{DEFAULT_AIM_WALK_LIMIT}; let norm=x.hypot(y); if norm>cap {x*=cap/norm;y*=cap/norm;} }
    Movement { x,y,sprint:!limited && y < -0.7,limited }
}
pub fn vector_event(kind:u32,x:f32,y:f32,sprint:bool) -> [u32;6] {
    [0x00a62e18,1006,kind,x.to_bits(),y.to_bits(),sprint as u32]
}
#[cfg(test)] mod look_tests {
    use super::*;
    fn motion(kind: u32, x: f32, y: f32) -> Option<LookEvent> { Some(LookEvent { kind, x, y }) }
    #[test] fn mouse_stop_or_capture_release_sends_one_neutral_event_on_the_next_frame() {
        for kind in [3, 5] {
            let mut look = NativeLook::default();
            assert_eq!(look.events(10, 42, kind, 12., -3.), [None, motion(kind, 12., -3.)]);
            assert_eq!(look.events(10, 42, kind, 0., 0.), [None, None]);
            assert_eq!(look.events(11, 42, kind, 0., 0.), [motion(kind, 0., 0.), None]);
            assert_eq!(look.events(12, 42, kind, 0., 0.), [None, None]);
        }
    }
    #[test] fn held_attack_transition_neutralizes_the_previous_native_camera_channel() {
        let mut look = NativeLook::default();
        look.events(1, 42, 3, 10., 0.);
        assert_eq!(look.events(2, 42, 5, 4., 1.), [motion(3, 0., 0.), motion(5, 4., 1.)]);
        assert_eq!(look.events(3, 42, 3, 0., 0.), [motion(5, 0., 0.), None]);
    }
    #[test] fn menu_cancellation_stops_look_without_sending_old_input_to_a_new_player() {
        let mut look = NativeLook::default();
        look.events(1, 42, 3, 10., 0.);
        assert_eq!(look.cancel(42), motion(3, 0., 0.));
        assert_eq!(look.cancel(42), None);
        look.events(2, 42, 5, 10., 0.);
        assert_eq!(look.events(3, 99, 3, 0., 0.), [None, None]);
        assert_eq!(look.cancel(99), None);
    }
    #[test] fn nonfinite_mouse_motion_cannot_leave_native_look_latched() {
        let mut look = NativeLook::default();
        look.events(1, 42, 3, 10., 0.);
        assert_eq!(look.events(2, 42, 3, f32::NAN, 1.), [motion(3, 0., 0.), None]);
        assert_eq!(look.events(3, 42, 3, f32::INFINITY, 0.), [None, None]);
    }
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn station_and_suit_lifecycle_keeps_ownership_until_native_exit_finishes(){
  for state in [19,20,21,22,23,24] {assert!(station_owns_input(state));}
  for state in [0,2,3,4,15,17,18,25,26] {assert!(!station_owns_input(state));}
 }
 #[test] fn opposite_keys_resume_held_direction_in_both_orders_on_both_axes(){
  for (first,last,first_vector,last_vector) in [(3,1,(1.,0.),(-1.,0.)),(1,3,(-1.,0.),(1.,0.)),(0,2,(0.,-1.),(0.,1.)),(2,0,(0.,1.),(0.,-1.))] {
   let mut keys=HeldDirections::new();assert_eq!(keys.update(first,true),first_vector);assert_eq!(keys.update(last,true),last_vector);
   assert_eq!(keys.update(last,false),first_vector);assert_eq!(keys.update(first,false),(0.,0.));
  }
 }
 #[test] fn releasing_older_opposite_key_preserves_newer_key(){
  for (first,last) in [(1,3),(3,1),(0,2),(2,0),(5,7),(4,6)] {
   let mut keys=HeldDirections::new();keys.update(first,true);let v=keys.update(last,true);assert_eq!(keys.update(first,false),v);
  }
 }
 #[test] fn aliases_and_diagonals_retain_every_physical_key(){
  let mut k=HeldDirections::new();k.update(0,true);k.update(4,true);assert_eq!(k.update(1,true),(-1.,-1.));
  assert_eq!(k.update(0,false),(-1.,-1.));assert_eq!(k.update(3,true),(1.,-1.));assert_eq!(k.update(3,false),(-1.,-1.));
  assert_eq!(k.update(4,false),(-1.,0.));assert_eq!(k.update(1,false),(0.,0.));
 }
 #[test] fn repeats_and_stale_releases_cannot_steal_priority_or_restore_cancelled_keys(){
  let mut k=HeldDirections::new();k.update(1,true);k.update(3,true);assert_eq!(k.update(1,true),(1.,0.));
  k.clear();assert_eq!(k.update(3,false),(0.,0.));assert_eq!(k.update(0,true),(0.,-1.));
 }
 #[test] fn all_physical_key_pairs_and_release_orders_preserve_survivors(){
  for a in 0..8 {for b in 0..8 {if a==b {continue;}for release_first in [a,b] {
   let mut k=HeldDirections::new();k.update(a,true);k.update(b,true);k.update(release_first,false);
   let survivor=if release_first==a {b}else{a};let mut expected=HeldDirections::new();expected.update(survivor,true);
   assert_eq!(k.vector(),expected.vector(),"keys {a}/{b}, release {release_first}");k.update(survivor,false);assert_eq!(k.vector(),(0.,0.));
  }}}
 }
 #[test] fn maximum_aimed_walk_preserves_raw_analogue_and_never_sets_sprint(){
  for (x,y) in [(0.,-1.),(-1.,-1.),(1.,-1.),(0.1,-0.2),(0.,-0.699),(0.,-0.701)] {
   let walk=movement(x,y,true,DEFAULT_AIM_WALK_LIMIT);assert!(!walk.sprint);let free=movement(x,y,false,DEFAULT_AIM_WALK_LIMIT);assert_eq!((walk.x,walk.y),(free.x,free.y));
  }
  assert!(movement(0.,-1.,false,DEFAULT_AIM_WALK_LIMIT).sprint);
 }
 #[test] fn held_keyboard_restores_full_forward_without_another_key_event(){ let raw=(0.0,-2.0);let full=movement(raw.0,raw.1,false,0.35);let aim=movement(raw.0,raw.1,true,0.35);assert_eq!(full.y,-1.0);assert!(full.sprint);assert!((aim.y+0.35).abs()<1e-6);assert!(!aim.sprint);assert_eq!(movement(raw.0,raw.1,false,0.35),full); }
 #[test] fn full_controller_stick_shrinks_and_recovers_while_physically_unchanged(){ let full=movement(0.0,-1.0,false,0.35);assert!((movement(0.0,-1.0,true,0.35).y+0.35).abs()<1e-6);assert_eq!(movement(0.0,-1.0,false,0.35),full); }
 #[test] fn forward_diagonals_limit_the_entire_vector_and_preserve_direction(){for x in [-2.0,2.0]{let m=movement(x,-2.0,true,0.35);assert!((m.x.hypot(m.y)-0.35).abs()<1e-6);assert!((m.x.abs()+m.y).abs()<1e-6);assert!(!m.sprint);} }
 #[test] fn side_and_backward_inputs_remain_full_and_partial_stick_is_not_amplified(){for (x,y) in [(1.0,0.0),(-1.0,0.0),(0.0,1.0)]{assert_eq!(movement(x,y,true,0.35),movement(x,y,false,0.35));}let m=movement(0.0,-0.1,true,0.35);assert_eq!(m.y,-0.1); }
 #[test] fn invalid_axes_cannot_start_motion(){for value in [f32::NAN,f32::INFINITY]{assert_eq!(movement(value,-1.0,true,0.35).y,0.0);} }
 #[test] fn native_vector_layout_uses_original_rtti_and_finite_float_bits(){let e=vector_event(3,4.5,-2.0,false);assert_eq!(&e[..3],&[0xa62e18,1006,3]);assert_eq!(f32::from_bits(e[3]),4.5);assert_eq!(f32::from_bits(e[4]),-2.0);assert_eq!(e[5],0);}
}

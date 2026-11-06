//! Edge-triggered weapon shortcuts. Never synthesize touch or write weapon state.
use super::ds_hud_commands::HudCommand;
use std::collections::VecDeque;

pub const HOLD_MS:u64=450;
pub const COOLDOWN_MS:u64=120;

pub fn enqueue(queue:&mut VecDeque<HudCommand>,command:HudCommand) {
    // Coalesce keyboard and pad together, including a request waiting for CD.
    if command.weapon_index().is_some() {queue.retain(|c|c.weapon_index().is_none());}
    if queue.len()<32 {queue.push_back(command);}
}
pub fn pop(queue:&mut VecDeque<HudCommand>,cooldown:&mut Cooldown,now:u64)->Option<HudCommand> {
    if queue.front()?.weapon_index().is_some() && !cooldown.take(now) {
        // Pause never waits behind weapon cooldown. Keep the newest weapon.
        return queue.iter().position(|c|c.weapon_index().is_none()).and_then(|i|queue.remove(i));
    }
    queue.pop_front()
}

pub struct Cooldown {last:Option<u64>}
impl Cooldown {
    pub const fn new()->Self {Self{last:None}}
    pub fn take(&mut self,now:u64)->bool {
        if self.last.is_some_and(|last|now.saturating_sub(last)<COOLDOWN_MS) {return false;}
        self.last=Some(now);true
    }
}

pub struct PadWeapons {
    held:[bool;4],
    latest:Option<(usize,u64)>,
    long_sent:bool,
}
impl PadWeapons {
    pub const fn new()->Self {Self{held:[false;4],latest:None,long_sent:false}}
    pub fn clear(&mut self) {*self=Self::new();}
    pub fn update(&mut self,direction:usize,down:bool,now:u64)->Option<HudCommand> {
        let held=self.held.get_mut(direction)?;
        if *held==down {return None;}
        *held=down;
        if !down {
            // Releasing the newest direction never reselects an older held
            // weapon, and cannot start a delayed long press on that old key.
            if self.latest.is_some_and(|(key,_)|key==direction) {self.latest=None;}
            return None;
        }
        self.latest=Some((direction,now));self.long_sent=false;
        Some([HudCommand::Weapon1,HudCommand::Weapon2,HudCommand::Weapon3,HudCommand::Weapon4][direction])
    }
    pub fn tick(&mut self,now:u64)->Option<HudCommand> {
        let (direction,start)=self.latest?;
        if self.held[direction] && !self.long_sent && now.saturating_sub(start)>=HOLD_MS {
            self.long_sent=true;Some(HudCommand::Weapon5)
        } else {None}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_direction_has_one_weapon_and_hold_selects_fifth_once() {
        for key in 0..4 {
            let mut pad=PadWeapons::new();
            assert_eq!(pad.update(key,true,0),Some([HudCommand::Weapon1,HudCommand::Weapon2,HudCommand::Weapon3,HudCommand::Weapon4][key]));
            assert_eq!(pad.tick(HOLD_MS-1),None);
            assert_eq!(pad.tick(HOLD_MS),Some(HudCommand::Weapon5));
            assert_eq!(pad.tick(HOLD_MS+1000),None);
            assert_eq!(pad.update(key,false,HOLD_MS+1001),None);
        }
    }
    #[test]
    fn newest_press_wins_and_release_does_not_resurrect_old_hold() {
        let mut pad=PadWeapons::new();pad.update(0,true,0);
        assert_eq!(pad.update(2,true,200),Some(HudCommand::Weapon3));
        assert_eq!(pad.tick(450),None);
        pad.update(2,false,500);
        assert_eq!(pad.tick(2000),None);
        assert_eq!(pad.update(0,true,2001),None); // old held key is not a new edge
        pad.update(0,false,2002);pad.update(0,true,2003);
        assert_eq!(pad.tick(2453),Some(HudCommand::Weapon5));
    }
    #[test]
    fn cancel_blocks_held_long_press_until_a_new_down_edge() {
        let mut pad=PadWeapons::new();pad.update(1,true,0);pad.clear();
        assert_eq!(pad.tick(2000),None);
        assert_eq!(pad.update(1,false,2001),None);
        assert_eq!(pad.update(1,true,2002),Some(HudCommand::Weapon2));
    }
    #[test]
    fn cooldown_retains_capacity_for_later_retry() {
        let mut cd=Cooldown::new();
        assert!(cd.take(0));assert!(!cd.take(119));assert!(cd.take(120));
        assert!(!cd.take(121));assert!(cd.take(240));
    }
    #[test]
    fn last_key_wins_both_in_one_poll_and_during_cd_without_delaying_pause() {
        let mut queue=VecDeque::new();let mut cd=Cooldown::new();
        enqueue(&mut queue,HudCommand::Weapon1);enqueue(&mut queue,HudCommand::Weapon3);
        assert_eq!(pop(&mut queue,&mut cd,0),Some(HudCommand::Weapon3));
        enqueue(&mut queue,HudCommand::Weapon2);enqueue(&mut queue,HudCommand::Weapon4);
        enqueue(&mut queue,HudCommand::Pause);
        assert_eq!(pop(&mut queue,&mut cd,20),Some(HudCommand::Pause));
        assert_eq!(pop(&mut queue,&mut cd,119),None);
        assert_eq!(pop(&mut queue,&mut cd,120),Some(HudCommand::Weapon4));
        assert!(queue.is_empty());
    }
}

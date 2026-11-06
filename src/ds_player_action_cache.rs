//! Ephemeral native-player action lookup. Guest fields are re-read at use.
pub const FORWARDER_VPTR: u32 = 0x00a7_d1e8;
pub const PLAYER_VPTR: u32 = 0x00a7_a4f8;

#[derive(Clone, Copy)]
pub struct Snapshot {
    pub vptr: u32,
    pub code: u32,
    pub enabled: u8,
    pub owner: u32,
    pub owner_vptr: u32,
}
impl Snapshot {
    pub fn belongs_to(self, owner: u32, code: u32) -> bool {
        owner != 0 && self.vptr == FORWARDER_VPTR && self.owner == owner
            && self.owner_vptr == PLAYER_VPTR && self.code == code
    }
    pub fn enabled_for(self, owner: u32, code: u32) -> bool {
        self.belongs_to(owner, code) && self.enabled != 0
    }
}
#[derive(Default)]
pub struct Cache { entry: Option<(u32, u32)> }
impl Cache {
    pub fn candidate(&mut self, owner: u32) -> Option<u32> {
        if self.entry.is_some_and(|entry| entry.0 != owner) { self.clear(); }
        self.entry.filter(|entry| owner != 0 && entry.0 == owner).map(|entry| entry.1)
    }
    pub fn remember(&mut self, owner: u32, forwarder: u32) {
        self.entry = (owner != 0 && forwarder != 0).then_some((owner, forwarder));
    }
    pub fn clear(&mut self) { self.entry = None; }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn live() -> Snapshot { Snapshot { vptr: FORWARDER_VPTR, code: 6, enabled: 1, owner: 100, owner_vptr: PLAYER_VPTR } }
    #[test]
    fn stale_actor_or_reused_object_cannot_dispatch() {
        assert!(live().enabled_for(100, 6));
        assert!(!live().enabled_for(101, 6));
        assert!(!Snapshot { vptr: 9, ..live() }.enabled_for(100, 6));
        assert!(!Snapshot { owner_vptr: 9, ..live() }.enabled_for(100, 6));
        assert!(!Snapshot { owner: 0, ..live() }.enabled_for(0, 6));
    }
    #[test]
    fn native_enable_and_action_code_are_checked_on_each_use() {
        assert!(!Snapshot { enabled: 0, ..live() }.enabled_for(100, 6));
        assert!(!Snapshot { code: 7, ..live() }.enabled_for(100, 6));
        assert!(Snapshot { enabled: 2, ..live() }.enabled_for(100, 6));
    }
    #[test]
    fn owner_change_invalidates_and_missing_lookup_is_not_remembered() {
        let mut cache = Cache::default();
        assert_eq!(cache.candidate(100), None);
        cache.remember(100, 200);
        assert_eq!(cache.candidate(100), Some(200));
        assert_eq!(cache.candidate(101), None);
        assert_eq!(cache.candidate(100), None);
        cache.remember(101, 300);
        assert_eq!(cache.candidate(101), Some(300));
        cache.remember(101, 0);
        assert_eq!(cache.candidate(101), None);
    }
}

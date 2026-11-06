//! Keyboard target policy; guest visibility/eligibility checks stay in the caller.
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub(crate) const PICKUP_VPTR: u32 = 0x00a7e158;
pub(crate) type PickupKey = (u32, u32, u32); // forwarder, owner, item type

pub(crate) fn rank(
    kinesis_key: bool,
    telekinetic: bool,
    pickup: bool,
    blocked: bool,
    distance: i64,
) -> Option<(u8, i64)> {
    if kinesis_key != telekinetic || (!kinesis_key && pickup && blocked) {
        return None;
    }
    Some((if pickup { 0 } else { 1 }, distance))
}

pub(crate) fn pickup_failed(before_quantity: u32, after_quantity: u32, after_state: u32) -> bool {
    // GameObjectPickup::OnOpen: successful complete pickup enters state 2;
    // partial pickup mutates Item::quantity (+0x1e0). A full bag does neither.
    after_state != 2 && before_quantity == after_quantity
}

#[derive(Default)]
pub(crate) struct PickupCooldowns(HashMap<PickupKey, Instant>);

impl PickupCooldowns {
    pub(crate) fn block(&mut self, key: PickupKey, now: Instant) {
        self.0.retain(|_, deadline| *deadline > now);
        self.0.insert(key, now + Duration::from_secs(3));
    }

    pub(crate) fn blocked(&mut self, key: PickupKey, now: Instant) -> bool {
        self.0.retain(|_, deadline| *deadline > now);
        self.0.contains_key(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickup_beats_nearer_bench_but_full_bag_allows_bench() {
        let bench = rank(false, false, false, false, 0);
        let drop = rank(false, false, true, false, 10000);
        assert!(drop.unwrap() < bench.unwrap());
        assert!(rank(false, false, true, true, 10000).is_none());
        assert!(bench.is_some());
    }

    #[test]
    fn e_cannot_grab_limb_and_f_cannot_pick_up_ammo_or_open_door() {
        assert!(rank(false, true, false, false, 0).is_none());
        assert!(rank(true, false, true, false, 0).is_none());
        assert!(rank(true, false, false, false, 0).is_none());
        assert!(rank(true, true, false, false, 0).is_some());
    }

    #[test]
    fn only_unsuccessful_pickups_enter_cooldown() {
        assert!(pickup_failed(10, 10, 0));
        assert!(!pickup_failed(10, 5, 0));
        assert!(!pickup_failed(10, 10, 2));
    }

    #[test]
    fn cooldown_is_per_item_and_expires_without_sleeping() {
        let now = Instant::now();
        let mut cooldowns = PickupCooldowns::default();
        cooldowns.block((1, 2, 3), now);
        assert!(cooldowns.blocked((1, 2, 3), now + Duration::from_secs(2)));
        assert!(!cooldowns.blocked((1, 2, 4), now));
        assert!(!cooldowns.blocked((5, 6, 3), now));
        assert!(!cooldowns.blocked((1, 2, 3), now + Duration::from_secs(3)));
    }
}

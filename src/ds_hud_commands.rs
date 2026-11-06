//! Borrowed string view for Hud::OnStringIdEvent in Dead Space iOS 1.3.34.
//! Not a general StringIdEvent constructor: call only the verified HUD handler,
//! synchronously, and never dispatch it through generic OnEvent/destructors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudCommand { Weapon1, Weapon2, Weapon3, Weapon4, Weapon5, Pause, Locator, Rig }

impl HudCommand {
    pub fn from_key(name: &str) -> Option<Self> {
        Some(match name { "1" => Self::Weapon1, "2" => Self::Weapon2, "3" => Self::Weapon3,
            "4" => Self::Weapon4, "5" => Self::Weapon5, "P" | "Escape" => Self::Pause,
            "G" => Self::Locator, "RIG" => Self::Rig, _ => return None })
    }
    pub fn spec(self) -> (&'static str, u32) {
        match self {
            Self::Weapon1 => ("BTN_LINEGUN", 0x8ee6e4),
            Self::Weapon2 => ("BTN_PLASMA", 0x8ee68c),
            Self::Weapon3 => ("BTN_RIPPER", 0x8ee6b8),
            Self::Weapon4 => ("BTN_CORER", 0x8ee714),
            Self::Weapon5 => ("BTN_BURST", 0x8ee73c),
            Self::Pause => ("BTN_PLAY_PAUSE", 0x8ee3a0),
            Self::Locator => ("BTN_MARKER", 0x8ee3dc),
            Self::Rig => ("BTN_RIG", 0x8ee380),
        }
    }
    pub fn allowed(self, hud_state: u32) -> bool {
        hud_state == 1 || (self == Self::Pause && matches!(hud_state, 2 | 3))
            || (self == Self::Rig && hud_state == 0)
    }
    pub fn needs_rig(self, hud_state: u32) -> bool {
        hud_state == 0 && self != Self::Rig
    }
    pub fn weapon_index(self) -> Option<u32> {
        match self {
            Self::Weapon1 => Some(2),
            Self::Weapon2 => Some(0),
            Self::Weapon3 => Some(1),
            Self::Weapon4 => Some(3),
            Self::Weapon5 => Some(4),
            Self::Pause | Self::Locator | Self::Rig => None,
        }
    }
    pub fn string_bytes(self) -> Vec<u8> {
        self.spec().0.chars().flat_map(|c| (c as u32).to_le_bytes()).collect()
    }
    pub fn borrowed_view(self) -> [u32; 6] {
        let (_, begin) = self.spec(); let end = begin + self.string_bytes().len() as u32;
        [0, 0, begin, end, end, 0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_existing_number_key_order_and_wchar32_extent() {
        assert_eq!(HudCommand::from_key("1").unwrap().spec().0, "BTN_LINEGUN");
        assert_eq!(HudCommand::from_key("2").unwrap().spec().0, "BTN_PLASMA");
        for key in ["1", "2", "3", "4", "5", "P", "G"] {
            let command = HudCommand::from_key(key).unwrap(); let view = command.borrowed_view();
            assert_eq!(view[3] - view[2], (command.spec().0.len() * 4) as u32);
            assert_eq!(view[4], view[3]); assert_eq!(command.string_bytes()[1], 0);
        }
    }
    #[test]
    fn pause_resume_only_and_unsupported_keys_do_not_dispatch() {
        assert!(HudCommand::Pause.allowed(3));
        assert!(HudCommand::Pause.allowed(2));
        for state in [0, 2, 3, 4, 13, 999] {
            assert!(!HudCommand::Weapon1.allowed(state)); assert!(!HudCommand::Locator.allowed(state));
        }
        assert!(HudCommand::from_key("Q").is_none());
    }
    #[test]
    fn closed_hud_requires_native_rig_preparation_not_a_state_override() {
        assert!(HudCommand::Weapon2.needs_rig(0));
        assert!(!HudCommand::Weapon2.allowed(0));
        assert!(HudCommand::Rig.allowed(0));
        assert!(!HudCommand::Rig.needs_rig(0));
        for state in [2,3,4,13,999] { assert!(!HudCommand::Weapon2.needs_rig(state)); }
        assert_eq!(HudCommand::Rig.spec(),("BTN_RIG",0x8ee380));
    }
    #[test]
    fn weapon_commands_keep_legacy_key_to_native_index_mapping() {
        assert_eq!((1..=5).map(|key| HudCommand::from_key(&key.to_string()).unwrap().weapon_index().unwrap()).collect::<Vec<_>>(), vec![2, 0, 1, 3, 4]);
        assert_eq!(HudCommand::Pause.weapon_index(), None);
        assert_eq!(HudCommand::Locator.weapon_index(), None);
    }
}

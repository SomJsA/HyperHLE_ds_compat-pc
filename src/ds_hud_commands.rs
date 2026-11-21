//! Borrowed wchar32 view for a verified native Hud::OnStringIdEvent handler.
//! The view is for synchronous calls to that handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudCommand { Weapon1, Weapon2, Weapon3, Weapon4, Weapon5, Pause, Locator, Rig }

pub(crate) struct Layout {
    pub handler: u32,
    stack_setup: u32,
    pub can_enter_rig: Option<u32>,
    legacy_strings: Option<[u32; 7]>,
}

impl Layout {
    pub fn for_get_hud(get_hud: u32) -> Option<Self> {
        // The string slots are Weapon1 through Weapon5, then Pause and Rig.
        let (handler, stack_setup, legacy_strings) = match get_hud {
            0x0003_0474 => (0x0019_4158, 0xe24d_df42, None),
            0x0002_1f18 => (0x0017_13ec, 0xe24d_df42, Some([
                0x0068_8890, 0x0068_88bc, 0x0068_88e8, 0x0068_8918, 0x0068_8940,
                0x0068_85a4, 0x0068_8584,
            ])),
            0x0002_a73c => (0x0017_920c, 0xe24d_df42, Some([
                0x0067_4880, 0x0067_48ac, 0x0067_48d8, 0x0067_4908, 0x0067_4930,
                0x0067_4594, 0x0067_4574,
            ])),
            0x0003_62f0 => (0x001e_bfc0, 0xe24d_d0e8, Some([
                0x0071_9804, 0x0071_9830, 0x0071_985c, 0x0071_988c, 0x0071_98b4,
                0x0071_92e8, 0x0071_96e8,
            ])),
            0x0006_1fa0 => (0x001e_1a00, 0xe24d_d0a0, Some([
                0x0068_2870, 0x0068_289c, 0x0068_28c8, 0x0068_28f8, 0,
                0x0068_22b0, 0x0068_2290,
            ])),
            _ => return None,
        };
        Some(Self {
            handler,
            stack_setup,
            can_enter_rig: legacy_strings.is_none().then_some(0x0018_b9ec),
            legacy_strings,
        })
    }

    pub fn handler_prefix(&self) -> [u32; 4] {
        [0xe92d_40f0, 0xe28d_700c, 0xe92d_0d00, self.stack_setup]
    }

    pub fn string_address(&self, command: HudCommand) -> Option<u32> {
        let Some(strings) = self.legacy_strings else {
            return Some(command.spec().1);
        };
        let slot = match command {
            HudCommand::Pause => 5,
            HudCommand::Rig => 6,
            _ => command.weapon_index()? as usize,
        };
        (strings[slot] != 0).then_some(strings[slot])
    }
}

impl HudCommand {
    pub fn from_key(name: &str) -> Option<Self> {
        Some(match name { "1" => Self::Weapon1, "2" => Self::Weapon2, "3" => Self::Weapon3,
            "4" => Self::Weapon4, "5" => Self::Weapon5, "P" | "Escape" => Self::Pause,
            "G" => Self::Locator, "RIG" => Self::Rig, _ => return None })
    }
    pub fn spec(self) -> (&'static str, u32) {
        match self {
            Self::Weapon1 => ("BTN_PLASMA", 0x8ee68c),
            Self::Weapon2 => ("BTN_RIPPER", 0x8ee6b8),
            Self::Weapon3 => ("BTN_LINEGUN", 0x8ee6e4),
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
            Self::Weapon1 => Some(0),
            Self::Weapon2 => Some(1),
            Self::Weapon3 => Some(2),
            Self::Weapon4 => Some(3),
            Self::Weapon5 => Some(4),
            Self::Pause | Self::Locator | Self::Rig => None,
        }
    }
    pub fn string_bytes(self) -> Vec<u8> {
        self.spec().0.chars().flat_map(|c| (c as u32).to_le_bytes()).collect()
    }
    pub fn borrowed_view(self, begin: u32) -> [u32; 6] {
        let end = begin + self.string_bytes().len() as u32;
        [0, 0, begin, end, end, 0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requested_weapon_order_and_wchar32_extent() {
        assert_eq!(HudCommand::from_key("1").unwrap().spec().0, "BTN_PLASMA");
        assert_eq!(HudCommand::from_key("2").unwrap().spec().0, "BTN_RIPPER");
        assert_eq!(HudCommand::from_key("3").unwrap().spec().0, "BTN_LINEGUN");
        for key in ["1", "2", "3", "4", "5", "P", "G"] {
            let command = HudCommand::from_key(key).unwrap(); let view = command.borrowed_view(command.spec().1);
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
    fn weapon_commands_match_display_order_and_native_indices() {
        assert_eq!((1..=5).map(|key| HudCommand::from_key(&key.to_string()).unwrap().weapon_index().unwrap()).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        assert_eq!(HudCommand::Pause.weapon_index(), None);
        assert_eq!(HudCommand::Locator.weapon_index(), None);
    }
    #[test]
    fn legacy_pause_uses_its_own_handler_and_utf32_range() {
        let iphone = Layout::for_get_hud(0x0002_a73c).unwrap();
        assert_eq!(iphone.handler, 0x0017_920c);
        assert_eq!(iphone.handler_prefix()[3], 0xe24d_df42);
        assert_eq!(iphone.can_enter_rig, None);
        let pause = HudCommand::Pause;
        let begin = iphone.string_address(pause).unwrap();
        assert_eq!(pause.borrowed_view(begin), [0, 0, 0x0067_4594, 0x0067_45cc, 0x0067_45cc, 0]);

        let world = Layout::for_get_hud(0x0006_1fa0).unwrap();
        assert!(world.string_address(HudCommand::Weapon5).is_none());
        let current = Layout::for_get_hud(0x0003_0474).unwrap();
        assert_eq!(current.can_enter_rig, Some(0x0018_b9ec));
        assert_eq!(current.string_address(pause), Some(pause.spec().1));
        assert!(Layout::for_get_hud(0x0002_a1dc).is_none());
    }
}

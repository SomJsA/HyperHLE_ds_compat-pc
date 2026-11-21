//! Strict native frontend profiles for each reverse-mapped Dead Space binary.
//!
//! Addresses in this file are only selected after the bundle identifier and
//! CFBundleVersion match a known executable layout.  Unknown/old binaries
//! must continue through the original touch path instead of receiving calls
//! into a different build.

use crate::Environment;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Binary {
    Current134,
    Ipad1311,
}

impl Binary {
    pub(crate) fn loading_wait_vptr(self) -> u32 {
        match self {
            Self::Current134 => 0x00a8_73b8,
            Self::Ipad1311 => 0x0077_d0d8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Native {
    pub get_active_rect: u32,
    pub layout_button_send_action: u32,
    pub set_selected: u32,
    pub select_difficulty: u32,
    pub survival_item_send_action: u32,
    pub chapter_item_send_action: u32,
}

const CURRENT: Native = Native {
    get_active_rect: 0x001e_0488,
    layout_button_send_action: 0x001d_0b80,
    set_selected: 0x001c_da9c,
    select_difficulty: 0x002f_c298,
    survival_item_send_action: 0x001d_66e4,
    chapter_item_send_action: 0x001d_7200,
};

const IPAD1311: Native = Native {
    get_active_rect: 0x001b_6ea0,
    layout_button_send_action: 0x001a_7a08,
    set_selected: 0x001a_4e04,
    // LayerDifficulty::SetDifficulty in the 1.3.11 layout.
    select_difficulty: 0x0018_5f7c,
    survival_item_send_action: 0x001a_d528,
    chapter_item_send_action: 0x001a_e03c,
};

pub(crate) fn identify(env: &Environment) -> Option<Binary> {
    let id = env.bundle.bundle_identifier();
    let version = env.bundle.bundle_version();
    if id == "com.ea.deadspace.bv" && version == "1.3.34" {
        Some(Binary::Current134)
    } else if id == "com.ea.deadspace.ipad.bv" && version == "1.3.11" {
        Some(Binary::Ipad1311)
    } else {
        None
    }
}

pub(crate) fn native(env: &Environment) -> Option<Native> {
    match identify(env) {
        Some(Binary::Current134) => Some(CURRENT),
        Some(Binary::Ipad1311) => Some(IPAD1311),
        None => None,
    }
}

pub(crate) fn is_current(env: &Environment) -> bool {
    identify(env) == Some(Binary::Current134)
}

pub(crate) fn is_ipad1311(env: &Environment) -> bool {
    identify(env) == Some(Binary::Ipad1311)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_addresses_stay_pinned() {
        assert_eq!(CURRENT.get_active_rect, 0x001e_0488);
        assert_eq!(CURRENT.chapter_item_send_action, 0x001d_7200);
    }

    #[test]
    fn ipad_addresses_match_reverse_map() {
        assert_eq!(IPAD1311.get_active_rect, 0x001b_6ea0);
        assert_eq!(IPAD1311.set_selected, 0x001a_4e04);
        assert_eq!(IPAD1311.chapter_item_send_action, 0x001a_e03c);
    }
}

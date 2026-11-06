//! Pure rules shared by the chapter-event hook and the Android display bridge.

pub(crate) const CALL_SITE: u32 = 0x0024_15c4;
pub(crate) const CALL_ORIGINAL: u32 = 0xebfd_2736;
pub(crate) const CALL_BREAKPOINT: u32 = 0xe120_0070;
pub(crate) const DRAW_SITE: u32 = 0x002f_6b2c;
pub(crate) const DRAW_ORIGINAL: u32 = 0xebff_2990;
pub(crate) const ARM_NOP: u32 = 0xe1a0_0000;

pub(crate) fn chapter_number(number_id: &str, title_id: &str) -> Option<u8> {
    (1..=12).find(|n| {
        number_id == format!("CHAPTER_{n:02}") && title_id == format!("CHAPTER_{n:02}_GAME_TITLE")
    })
}

/// Destination of the original ARM BL. Handling BKPT must perform exactly the
/// original BL's PC/LR change, leaving arguments, flags and other registers alone.
pub(crate) fn call_target() -> u32 {
    let signed_offset = ((CALL_ORIGINAL << 8) as i32) >> 6;
    CALL_SITE.wrapping_add(8).wrapping_add(signed_offset as u32)
}

pub(crate) fn visible(phase: u32, eligible: u8, hud_state: u32, input_state: u32) -> bool {
    matches!(phase, 1..=3) && eligible != 0 && !matches!(hud_state, 2..=14) && input_state == 0
}

// JNI contract: chapter bits 0..7, phase 8..15, visible bit 16,
// playback serial 32..63. Never publish a visible card without native suppression.
pub(crate) fn snapshot(chapter: u8, phase: u32, serial: u32) -> u64 {
    u64::from(chapter) | (u64::from(phase) << 8) | (1 << 16) | (u64::from(serial) << 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_all_twelve_exact_pairs() {
        for n in 1..=12 {
            assert_eq!(
                chapter_number(
                    &format!("CHAPTER_{n:02}"),
                    &format!("CHAPTER_{n:02}_GAME_TITLE")
                ),
                Some(n)
            );
        }
        for (a, b) in [
            ("CHAPTER_01", "CHAPTER_02_GAME_TITLE"),
            ("CHAPTER_01", "CHAPTER_01_TITLE"),
            ("CHAPTER_00", "CHAPTER_00_GAME_TITLE"),
            ("CHAPTER_13", "CHAPTER_13_GAME_TITLE"),
            ("CHAPTER_1", "CHAPTER_1_GAME_TITLE"),
            ("CHAPTER_01 ", "CHAPTER_01_GAME_TITLE"),
        ] {
            assert_eq!(chapter_number(a, b), None);
        }
    }

    #[test]
    fn intercept_preserves_original_branch_destination() {
        assert_eq!(call_target(), 0x0018_b2a4);
        assert_eq!(CALL_SITE + 4, 0x0024_15c8);
    }

    #[test]
    fn all_active_phases_obey_visibility_gates() {
        for phase in 0..=4 {
            assert_eq!(visible(phase, 1, 0, 0), (1..=3).contains(&phase));
            assert!(!visible(phase, 0, 0, 0));
            assert!(!visible(phase, 1, 0, 1));
            for hud in 2..=14 {
                assert!(!visible(phase, 1, hud, 0));
            }
        }
    }

    #[test]
    fn bridge_contract_retains_chapter_phase_and_replay_serial() {
        for chapter in 1..=12 {
            for phase in 1..=3 {
                let value = snapshot(chapter, phase, u32::MAX);
                assert_eq!(value & 255, u64::from(chapter));
                assert_eq!((value >> 8) & 255, u64::from(phase));
                assert_ne!(value & (1 << 16), 0);
                assert_eq!(value >> 32, u64::from(u32::MAX));
            }
        }
    }
}

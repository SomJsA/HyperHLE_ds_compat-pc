//! Objective notification call/draw sites for the checked Dead Space binary.
pub(crate) const CALL_SITE: u32 = 0x0018_b284;
pub(crate) const CALL_ORIGINAL: u32 = 0xeb05_b391;
pub(crate) const BREAKPOINT: u32 = 0xe120_0070;
pub(crate) const DRAW_SITE: u32 = 0x002f_8f00;
pub(crate) const DRAW_ORIGINAL: u32 = 0xebff_209b;
pub(crate) const NOP: u32 = 0xe1a0_0000;

pub(crate) fn visible(phase: u32, eligible: u8, hud_state: u32, input: u32, chapter: u32, tutorial: u32) -> bool {
    matches!(phase, 1..=3) && eligible != 0 && !matches!(hud_state, 2..=14)
        && input == 0 && chapter == 0 && (tutorial == 0 || (tutorial == 1 && phase >= 2))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn preserves_chapter_and_tutorial_priority() {
        for phase in 1..=3 {
            assert!(visible(phase, 1, 0, 0, 0, 0));
            assert!(!visible(phase, 1, 0, 0, 1, 0));
            assert!(!visible(phase, 1, 0, 0, 0, 2));
            assert_eq!(visible(phase, 1, 0, 0, 0, 1), phase >= 2);
            assert!(!visible(phase, 0, 0, 0, 0, 0));
            assert!(!visible(phase, 1, 2, 0, 0, 0));
            assert!(!visible(phase, 1, 0, 1, 0, 0));
        }
        assert!(!visible(0, 1, 0, 0, 0, 0));
    }
    #[test] fn original_call_destination_is_initializer() {
        let offset = ((CALL_ORIGINAL << 8) as i32) >> 6;
        assert_eq!(CALL_SITE.wrapping_add(8).wrapping_add(offset as u32), 0x002f_80d0);
    }
}

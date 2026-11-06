//! Binary-specific subtitle indices and all-or-nothing draw gate preflight.
pub(crate) const SITES: [(u32, u32, u32); 2] = [
    (0x0018_9d64, 0x0a00_0008, 0xea00_0008),
    (0x0018_a1f8, 0x0a00_0033, 0xea00_0033),
];

pub(crate) fn entry(id: &str) -> Option<u32> {
    include_str!("ds_subtitle_ids.txt")
        .lines()
        .position(|s| s == id)
        .map(|index| index as u32 + 1)
}

pub(crate) fn preflight(actual: [Option<u32>; 2], suppressed: bool) -> bool {
    SITES.iter().enumerate().all(|(i, (_, original, skip))| {
        actual[i] == Some(if suppressed { *skip } else { *original })
    })
}

pub(crate) fn valid_segment(begin: u32, end: u32, segment: u32) -> Option<u32> {
    let size = end.checked_sub(begin)?;
    if begin < 0x1000 || size == 0 || size % 20 != 0 || size > 20 * 512 || segment >= size / 20 {
        return None;
    }
    begin.checked_add(segment * 20)
}

/// Own blank gaps only when every caption in the track can be replaced.
/// Unknown or malformed captions keep the original fallback available.
pub(crate) fn replace_track<'a>(ids: impl IntoIterator<Item = Result<Option<&'a str>, ()>>) -> bool {
    let mut mapped = false;
    for id in ids {
        match id {
            Ok(None) => (),
            Ok(Some(id)) if entry(id).is_some() => mapped = true,
            _ => return false,
        }
    }
    mapped
}

/// RenderSubtitle checks the finished byte before drawing its active track.
pub(crate) fn visible_value(serial:u32,index:Option<u32>,finished:Option<u8>)->u64 {
    if finished!=Some(0){return 0;}
    index.map(|i|(u64::from(serial)<<32)|u64::from(i)).unwrap_or(0)
}
/// SubtitleTrack stores remaining milliseconds at +8 and per-cue durations
/// in its +0x24 vector (verified against SetTimes/OnUpdate/Reset).
pub(crate) fn timing_value(total:u32,remaining:i32)->u64 {
    timing_value_with_offset(total, remaining, 0)
}

/// Per-cue calibration against the shipped voice track. Positive values make
/// the caption appear earlier; negative values delay it.
pub(crate) fn timing_offset(id:&str)->i32 {
    match id {
        "Miner_02"|"Oper_01" => 2_000,
        // The two "third public sector" cues are already aligned by the
        // native clock and must not inherit the public-sector advance.
        "Oper_02"|"Oper_03" => 0,
        "Miner_01" => -3_000,
        _ => 0,
    }
}

pub(crate) fn timing_value_with_offset(total:u32,remaining:i32,offset:i32)->u64 {
    if !(100..=600_000).contains(&total){return 0;}
    let elapsed=(i64::from(total)-i64::from(remaining)+i64::from(offset))
        .clamp(0,i64::from(total)) as u32;
    (u64::from(total)<<32)|u64::from(elapsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finished_or_unreadable_track_cannot_leave_the_last_caption_visible(){
        assert_eq!(visible_value(7,Some(10),Some(0)),(7u64<<32)|10);
        assert_eq!(visible_value(7,Some(10),Some(1)),0);
        assert_eq!(visible_value(7,Some(10),None),0);
    }
    #[test]
    fn native_remaining_time_is_bounded_and_retains_pause_progress(){
        assert_eq!(timing_value(10_000,7500),(10_000u64<<32)|2500);
        assert_eq!(timing_value(10_000,7500),timing_value(10_000,7500));
        assert_eq!(timing_value(10_000,-100),(10_000u64<<32)|10_000);
        assert_eq!(timing_value(0,0),0);assert_eq!(timing_value(u32::MAX,1),0);
    }
    #[test]
    fn per_cue_offsets_advance_or_delay_only_named_voice_lines(){
        assert_eq!(timing_value_with_offset(10_000,7500,2_000),(10_000u64<<32)|4500);
        assert_eq!(timing_value_with_offset(10_000,7500,-3_000),(10_000u64<<32)|0);
        assert_eq!(timing_offset("Miner_02"),2_000);
        assert_eq!(timing_offset("Oper_02"),0);
        assert_eq!(timing_offset("Oper_03"),0);
        assert_eq!(timing_offset("Miner_01"),-3_000);
        assert_eq!(timing_offset("Van_47"),0);
    }
    #[test]
    fn known_dialogue_indices_and_missing_fallback() {
        assert!(entry("Van_47").is_some());
        assert!(entry("Anchor_01").is_some());
        assert_ne!(entry("Van_47"), entry("Anchor_01"));
        assert_eq!(entry("Anchor_01\0"), None);
        assert_eq!(entry("unknown dialogue"), None);
    }
    #[test]
    fn both_draw_sites_must_match_before_any_patch() {
        let originals = [Some(SITES[0].1), Some(SITES[1].1)];
        let skips = [Some(SITES[0].2), Some(SITES[1].2)];
        assert!(preflight(originals, false));
        assert!(preflight(skips, true));
        assert!(!preflight([originals[0], skips[1]], false));
        assert!(!preflight([skips[0], originals[1]], true));
        assert!(!preflight([originals[0], None], false));
    }
    #[test]
    fn track_boundaries_reject_sentinels_and_bad_segments() {
        assert_eq!(valid_segment(0x2000, 0x2028, 1), Some(0x2014));
        for (a, b, i) in [
            (0, 0, 0),
            (1, 21, 0),
            (0x2000, 0x2000, 0),
            (0x2000, 0x2001, 0),
            (0x2000, 0x2028, 2),
            (0x2028, 0x2000, 0),
            (u32::MAX - 5, u32::MAX, 1),
        ] {
            assert_eq!(valid_segment(a, b, i), None);
        }
    }

    #[test]
    fn known_tracks_keep_takeover_across_blank_gaps_without_hiding_unknown_text() {
        assert!(replace_track([Ok(None), Ok(Some("Anchor_01")), Ok(None), Ok(Some("Van_47")), Ok(None)]));
        assert!(!replace_track([Ok(Some("Anchor_01")), Ok(Some("unknown dialogue"))]));
        assert!(!replace_track([Ok(Some("Anchor_01")), Err(())]));
        assert!(!replace_track([Ok(None), Ok(None)]));
    }
}

// Pure checked guest spans for native math snapshots. No Environment or mutation.
pub(crate) fn span(base: u32, offset: u64, len: usize) -> Result<u32, &'static str> {
    let at = u64::from(base)
        .checked_add(offset)
        .ok_or("address-overflow")?;
    if base < 0x1000
        || at < 0x1000
        || len == 0
        || at
            .checked_add(len as u64)
            .is_none_or(|end| end > 1u64 << 32)
    {
        return Err("address-range");
    }
    at.try_into().map_err(|_| "address-overflow")
}
pub(crate) fn overlap(a: (u32, usize), b: (u32, usize)) -> bool {
    u64::from(a.0) < u64::from(b.0) + b.1 as u64 && u64::from(b.0) < u64::from(a.0) + a.1 as u64
}
#[cfg(test)]
mod tests {
    use super::{overlap, span};
    #[test]
    fn snapshot_addressing_rejects_null_wrap_and_oversized_ranges() {
        assert!(span(0, 0x1000, 4).is_err());
        assert!(span(0x1000, u64::MAX, 12).is_err());
        assert!(span(0xffff_fffc, 1, 4).is_err());
        assert!(span(0x1000, 0, usize::MAX).is_err());
        assert_eq!(span(0xffff_fffc, 0, 4), Ok(0xffff_fffc));
    }
    #[test]
    fn snapshot_alias_checks_cover_partial_overlap_and_adjacent_buffers() {
        assert!(overlap((0x1000, 12), (0x1008, 16)));
        assert!(overlap((0x1008, 16), (0x1000, 12)));
        assert!(!overlap((0x1000, 12), (0x100c, 16)));
    }
}

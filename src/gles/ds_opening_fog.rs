//! Repair the observed Dead Space cinematic 10..25 -> 0..0 -> 10..25 ramp.
//! A short, context-local transition guard, not a minimum fog-distance clamp.

#[derive(Default, Debug)]
pub(crate) struct OpeningFog {
    has_baseline: bool,
    zero_gap: bool,
    active_frame: Option<u64>,
}

impl OpeningFog {
    pub(crate) fn range(&mut self, frame: u64, start: f32, end: f32) -> Option<(f32, f32)> {
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end < 0.0 {
            self.active_frame = None;
            return None;
        }
        let signature = (start - end * 0.4).abs() < 0.002;
        if signature && (start - 10.0).abs() < 0.01 && (end - 25.0).abs() < 0.01 {
            self.has_baseline = true;
            self.zero_gap = false;
            self.active_frame = None;
            return None;
        }
        if !signature || end >= 25.0 {
            self.has_baseline = false;
            self.zero_gap = false;
            self.active_frame = None;
            return None;
        }
        // The camera can spend thousands of frames with fog disabled by its
        // zero range. This is a waiting phase, not the recovery itself.
        if end == 0.0 {
            self.zero_gap = self.has_baseline;
            self.active_frame = None;
            return None;
        }
        if self.active_frame.is_none() && end < 0.25 && self.has_baseline && self.zero_gap {
            self.active_frame = Some(frame);
            self.zero_gap = false;
        }
        if let Some(first) = self.active_frame {
            if frame.saturating_sub(first) <= 240 {
                return Some((10.0, 25.0));
            }
            self.active_frame = None;
            self.has_baseline = false;
            self.zero_gap = false;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_collapse_and_recovery_keep_normal_atmosphere() {
        let mut fog = OpeningFog::default();
        assert_eq!(fog.range(100, 10.0, 24.999994), None);
        assert_eq!(fog.range(101, 0.0, 0.0), None);
        for (frame, end) in [
            (102, 0.004155642),
            (110, 1.0168812),
            (130, 13.17033),
            (150, 24.9),
        ] {
            assert_eq!(fog.range(frame, end * 0.4, end), Some((10.0, 25.0)));
        }
        assert_eq!(fog.range(151, 10.0, 25.0), None);
    }

    #[test]
    fn long_zero_range_cinematic_does_not_expire_before_recovery() {
        let mut fog = OpeningFog::default();
        fog.range(270, 10.0, 25.0);
        for frame in 271..3870 {
            assert_eq!(fog.range(frame, 0.0, 0.0), None);
        }
        assert_eq!(
            fog.range(3870, 0.0016622568, 0.004155642),
            Some((10.0, 25.0))
        );
        assert_eq!(fog.range(3900, 5.698911, 14.247277), Some((10.0, 25.0)));
        assert_eq!(fog.range(3940, 10.0, 25.0), None);
    }

    #[test]
    fn ordinary_or_unrelated_fog_is_not_clamped() {
        let mut fog = OpeningFog::default();
        assert_eq!(fog.range(1, 0.04, 0.1), None);
        fog.range(2, 10.0, 25.0);
        assert_eq!(fog.range(3, 5.0, 12.5), None);
        assert_eq!(fog.range(4, 0.1, 0.2), None);
        assert_eq!(fog.range(20, 0.04, 0.1), None);
    }

    #[test]
    fn stalled_ramp_expires_and_contexts_are_independent() {
        let mut a = OpeningFog::default();
        let mut b = OpeningFog::default();
        a.range(100, 10.0, 25.0);
        assert_eq!(a.range(101, 0.0, 0.0), None);
        assert_eq!(a.range(102, 0.04, 0.1), Some((10.0, 25.0)));
        assert_eq!(b.range(101, 0.0, 0.0), None);
        assert_eq!(a.range(343, 0.04, 0.1), None);
    }
}

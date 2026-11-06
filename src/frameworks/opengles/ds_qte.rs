//! Dead Space 1.3.34 GestureEvent ABI, verified at 0x40cd0 and 0xe191c.
pub(crate) fn grapple_swipe(state: u32) -> Option<[i32; 7]> {
    let points = match state {
        1 => [120, 80, 220, 180],
        7 => [340, 160, 140, 160],
        _ => return None,
    };
    Some([0x00a62e58, 1007, 15, points[0], points[1], points[2], points[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_angle(event: [i32; 7]) -> f32 {
        // The actual ARM handler subtracts signed words BEFORE conversion,
        // then calls atan2f(dx, dy), with the unusual argument order.
        ((event[5] - event[3]) as f32)
            .atan2((event[6] - event[4]) as f32).to_degrees()
    }

    #[test]
    fn both_swipes_pass_native_angle_guards() {
        assert!((12.0..78.0).contains(&native_angle(grapple_swipe(1).unwrap())));
        assert!((-123.0..-57.0).contains(&native_angle(grapple_swipe(7).unwrap())));
    }

    #[test]
    fn old_upward_swipe_fails_second_stage() {
        let old = [0, 0, 0, 0, 100.0f32.to_bits() as i32, 0, 0];
        assert!(!(-123.0..-57.0).contains(&native_angle(old)));
    }

    #[test]
    fn no_swipe_during_tap_or_transition_states() {
        for state in [0, 2, 3, 4, 5, 6, 8, 9, u32::MAX] {
            assert!(grapple_swipe(state).is_none());
        }
    }
}

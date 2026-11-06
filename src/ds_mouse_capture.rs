//! Mouse capture transitions shared by the SDL window and isolated VM tests.

pub trait CaptureBackend {
    fn set_relative(&mut self, enabled: bool) -> Result<(), String>;
    fn show_cursor(&mut self, visible: bool);
}

/// Reject motion queued before a capture transition, including cursor recentering.
#[derive(Default)]
pub struct MotionGate(Option<u32>);
impl MotionGate {
    pub fn reset(&mut self, timestamp: u32) { self.0 = Some(timestamp); }
    pub fn accepts(&self, timestamp: u32) -> bool {
        self.0.is_none_or(|start| timestamp != start && timestamp.wrapping_sub(start) < 0x8000_0000)
    }
}

/// Limit fresh aim presses; release handling never goes through this gate.
#[derive(Default)]
pub struct RightPressGate(Option<u32>);
impl RightPressGate {
    pub fn accepts(&mut self, timestamp:u32)->bool {
        if let Some(last)=self.0 {
            let elapsed=timestamp.wrapping_sub(last);
            if elapsed<500 || elapsed>=0x8000_0000 { return false; }
        }
        self.0=Some(timestamp); true
    }
}

/// Releasing the mouse must not activate the legacy stick cursor in native mode.
pub fn gamepad_cursor_active(native: bool, connected: bool, allowed: bool, explicit: bool, legacy_release: bool) -> bool {
    connected && allowed && (explicit || (!native && legacy_release))
}

#[derive(Default)]
pub struct CaptureState {
    pub captured: bool,
    pub forced_off: bool,
    pub recapture_on_focus: bool,
}

impl CaptureState {
    /// F8/Alt/Win take effect immediately, before the next SDL capture update.
    pub fn gameplay_pointer(&self, suspended: bool, alt: bool, cursor: bool) -> bool {
        self.captured && !self.forced_off && !suspended && !alt && !cursor
    }
    /// Failed enable requests latch click mode until an explicit retry.
    pub fn should_change(&self, wanted: bool) -> bool {
        (wanted && !self.forced_off) != self.captured
    }

    pub fn set(&mut self, wanted: bool, backend: &mut impl CaptureBackend) -> Result<(), String> {
        let wanted = wanted && !self.forced_off;
        let result = backend.set_relative(wanted);
        self.captured = wanted && result.is_ok();
        if wanted && result.is_err() {
            self.forced_off = true;
            self.recapture_on_focus = false;
        }
        backend.show_cursor(!self.captured);
        result
    }

    /// Use the completed capture result, never the request from before SDL ran.
    pub fn look_active(&self, focused: bool, controller_pending: bool) -> bool {
        self.captured || (focused && controller_pending)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LeftPress {
    Pointer((f32, f32)),
    Fire((f32, f32)),
}

/// Latch the destination on Down: capture changes cannot redirect the matching Up.
#[derive(Default)]
pub struct LeftButton(Option<LeftPress>);
impl LeftButton {
    pub fn down(&mut self, window: (f32, f32), aim: Option<(f32, f32)>, gameplay: bool) -> LeftPress {
        let press = match aim.filter(|_| gameplay) {
            Some(point) => LeftPress::Fire(point),
            None => LeftPress::Pointer(window),
        };
        self.0 = Some(press);
        press
    }
    pub fn pointer_drag(&self) -> bool { matches!(self.0, Some(LeftPress::Pointer(_))) }
    pub fn up(&mut self) -> Option<LeftPress> { self.0.take() }
    pub fn cancel(&mut self) { self.0 = None; }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_transition_rejects_queued_and_same_tick_warp_motion_but_accepts_new_motion() {
        let mut gate = MotionGate::default();
        assert!(gate.accepts(99));
        gate.reset(100);
        assert!(!gate.accepts(99));
        assert!(!gate.accepts(100));
        assert!(gate.accepts(101));
        gate.reset(200);
        assert!(!gate.accepts(150));
        assert!(gate.accepts(201));
    }

    #[test]
    fn capture_motion_timestamps_handle_sdl_tick_rollover() {
        let mut gate = MotionGate::default();
        gate.reset(u32::MAX - 1);
        assert!(gate.accepts(1));
        gate.reset(1);
        assert!(!gate.accepts(u32::MAX));
        assert!(!gate.accepts(1));
        assert!(gate.accepts(2));
    }

    #[derive(Default)]
    struct Backend {
        reject: bool,
        requests: Vec<bool>,
        cursor: Vec<bool>,
    }
    impl CaptureBackend for Backend {
        fn set_relative(&mut self, enabled: bool) -> Result<(), String> {
            self.requests.push(enabled);
            if self.reject {
                Err("injected SDL failure".into())
            } else {
                Ok(())
            }
        }
        fn show_cursor(&mut self, visible: bool) {
            self.cursor.push(visible);
        }
    }

    #[test]
    fn rejected_capture_keeps_pointer_and_stops_automatic_retries() {
        let mut state = CaptureState {
            recapture_on_focus: true,
            ..Default::default()
        };
        let mut backend = Backend {
            reject: true,
            ..Default::default()
        };
        assert!(state.set(true, &mut backend).is_err());
        assert!(!state.captured);
        assert!(state.forced_off);
        assert!(!state.recapture_on_focus);
        assert!(
            !state.look_active(true, false),
            "no stale mouse drag in the failed frame"
        );
        for _ in 0..240 {
            if state.should_change(true) {
                state.set(true, &mut backend).unwrap();
            }
        }
        assert_eq!(backend.requests, [true]);
        assert_eq!(backend.cursor, [true]);
    }

    #[test]
    fn explicit_retry_and_release_restore_consistent_state() {
        let mut state = CaptureState::default();
        let mut backend = Backend {
            reject: true,
            ..Default::default()
        };
        assert!(state.set(true, &mut backend).is_err());
        state.forced_off = false; // F8 / explicit LOOK selection
        backend.reject = false;
        assert!(state.should_change(true));
        state.set(true, &mut backend).unwrap();
        assert!(state.look_active(true, false));
        state.set(false, &mut backend).unwrap();
        assert!(!state.look_active(true, false));
        assert_eq!(backend.requests, [true, true, false]);
        assert_eq!(backend.cursor, [true, false, true]);
    }

    #[test]
    fn controller_look_stays_independent_of_mouse_capture() {
        let state = CaptureState {
            forced_off: true,
            ..Default::default()
        };
        assert!(state.look_active(true, true));
        assert!(!state.look_active(false, true));
        assert!(!state.look_active(true, false));
    }

    #[test]
    fn click_modes_keep_menu_origin_even_if_capture_resumes_before_up() {
        let mut capture = CaptureState { captured: true, ..Default::default() };
        let mut left = LeftButton::default();
        for mode in 0..4 {
            capture.forced_off = mode == 0; // F8, Alt, Win, controller cursor
            let gameplay = capture.gameplay_pointer(mode == 2, mode == 1, mode == 3);
            assert_eq!(left.down((80.0, 286.0), Some((360.0, 200.0)), gameplay), LeftPress::Pointer((80.0, 286.0)));
            assert!(left.pointer_drag());
            capture.forced_off = false;
            assert!(capture.gameplay_pointer(false, false, false));
            assert_eq!(left.up(), Some(LeftPress::Pointer((80.0, 286.0))));
            assert_eq!(left.up(), None);
        }
    }

    #[test]
    fn failed_capture_cannot_route_a_menu_click_to_fire() {
        let mut capture = CaptureState::default();
        let mut backend = Backend { reject: true, ..Default::default() };
        assert!(capture.set(true, &mut backend).is_err());
        let mut left = LeftButton::default();
        assert_eq!(left.down((20.0, 30.0), Some((360.0, 200.0)), capture.gameplay_pointer(false, false, false)), LeftPress::Pointer((20.0, 30.0)));
        left.cancel(); // focus loss releases delivered touches separately
        assert!(!left.pointer_drag());
        assert_eq!(left.up(), None);
    }

    #[test]
    fn fire_release_keeps_its_finger_and_coordinates_across_mode_changes() {
        let mut left = LeftButton::default();
        assert_eq!(left.down((20.0, 30.0), Some((360.0, 200.0)), true), LeftPress::Fire((360.0, 200.0)));
        assert!(!left.pointer_drag());
        assert_eq!(left.up(), Some(LeftPress::Fire((360.0, 200.0))));
        left.down((20.0, 30.0), Some((360.0, 200.0)), true);
        left.cancel();
        assert_eq!(left.up(), None);
    }
}

#[cfg(test)]
mod right_press_tests {
    use super::*;
    #[test] fn caps_new_presses_at_two_per_second_without_sliding_on_rejections() {
        let mut gate=RightPressGate::default();
        assert!(gate.accepts(100));
        for t in [101,200,400,599] {assert!(!gate.accepts(t));}
        assert!(gate.accepts(600));
        assert!(!gate.accepts(1099)); assert!(gate.accepts(1100));
    }
    #[test] fn handles_sdl_timestamp_rollover() {
        let mut gate=RightPressGate::default();
        assert!(gate.accepts(u32::MAX-249));
        assert!(!gate.accepts(249)); assert!(gate.accepts(250));
    }
}

#[cfg(test)]
mod cursor_mode_tests {
    use super::gamepad_cursor_active;
    #[test]
    fn native_mouse_release_never_reveals_centered_stick_cursor() {
        // F8, Alt, focus suspension and look-off all feed legacy_release.
        for released in [false, true] {
            assert!(!gamepad_cursor_active(true, true, true, false, released));
            assert!(gamepad_cursor_active(true, true, true, true, released));
        }
    }
    #[test]
    fn preserves_legacy_cursor_but_requires_allowed_connected_gamepad() {
        assert!(gamepad_cursor_active(false, true, true, false, true));
        assert!(!gamepad_cursor_active(false, true, true, false, false));
        assert!(!gamepad_cursor_active(true, true, false, true, true));
        assert!(!gamepad_cursor_active(true, false, true, true, true));
    }
}

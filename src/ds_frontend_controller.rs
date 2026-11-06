//! Verified native UI navigation, pointer focus, and HUD confirmation routing.
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
#[path = "ds_frontend_native.rs"]
mod native;
#[path="ds_loading_ui.rs"]
mod loading;

pub(crate) const UNKNOWN: u32 = u32::MAX;
static STATE: AtomicU32 = AtomicU32::new(UNKNOWN);
static SUPPORTED: AtomicBool = AtomicBool::new(false);
static MENU_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static HUD_STATE: AtomicU32 = AtomicU32::new(UNKNOWN);
// Input ownership may deliberately hide the HUD while a pause/modal handler
// draws. That transient absence must not turn in-game text into frontend text.
static STORY_TEXT: AtomicBool = AtomicBool::new(false);
static HUD_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static PAUSE_PENDING: AtomicBool = AtomicBool::new(false);
static SCENE_ACTION: AtomicU32 = AtomicU32::new(0);
static SCENE_DIALOG_FRAME: AtomicU64 = AtomicU64::new(0);
static INPUT_DEVICE: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Cancel,
    Delete,
}

pub(crate) fn keyboard_action(key: sdl2::keyboard::Keycode) -> Option<Action> {
    use sdl2::keyboard::Keycode as K;
    match key {
        K::Up => Some(Action::Up),
        K::Down => Some(Action::Down),
        K::Left => Some(Action::Left),
        K::Right => Some(Action::Right),
        K::E | K::Return | K::KpEnter => Some(Action::Confirm),
        K::Q | K::X | K::Escape => Some(Action::Cancel),
        K::Delete => Some(Action::Delete),
        _ => None,
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

pub(crate) fn configured() -> bool {
    std::env::var("TOUCHHLE_DS_FRONTEND_NAV").as_deref() != Ok("0")
}
pub(crate) fn native_modal_verified(env: &crate::Environment) -> bool {
    native::modal_verified(env)
}
pub(crate) fn enabled() -> bool {
    SUPPORTED.load(Ordering::Relaxed) && configured()
}
pub(crate) fn deactivate() {
    SUPPORTED.store(false, Ordering::Relaxed);
    STATE.store(UNKNOWN, Ordering::Relaxed);
    MENU_FRAME.store(u64::MAX, Ordering::Relaxed);
    HUD_STATE.store(UNKNOWN, Ordering::Relaxed);
    STORY_TEXT.store(false, Ordering::Relaxed);
    HUD_FRAME.store(u64::MAX, Ordering::Relaxed);
    PAUSE_PENDING.store(false, Ordering::Relaxed);
    INPUT_DEVICE.store(0, Ordering::Relaxed);
    SCENE_ACTION.store(0, Ordering::Relaxed);
    native::reset();
    loading::reset();
}
pub(crate) fn activate() {
    deactivate();
    SUPPORTED.store(true, Ordering::Relaxed);
}
pub(crate) fn note_input(keyboard: bool) {
    let device=if keyboard {1}else{2};
    let changed=INPUT_DEVICE.swap(device,Ordering::Relaxed)!=device;
    if changed && std::env::var("TOUCHHLE_DS_FRONTEND_TRACE").as_deref() == Ok("1") {
        echo!(
            "[DSfrontendInput] note_input device={} state={} menu={}",
            if keyboard { "keyboard" } else { "gamepad" },
            STATE.load(Ordering::Relaxed),
            capture_menu()
        );
    }
}
pub(crate) fn keyboard_hints() -> bool {
    match std::env::var("TOUCHHLE_DS_INPUT_MODE").as_deref() {
        Ok("keyboard")=>true,Ok("gamepad")=>false,
        _=>INPUT_DEVICE.load(Ordering::Relaxed)!=2,
    }
}
pub(crate) fn observe_frame(frame: u64) {
    SCENE_DIALOG_FRAME.store(frame, Ordering::Relaxed);
}
fn menu_recent(frame: u64, seen: u64) -> bool {
    seen != u64::MAX && frame.saturating_sub(seen) <= 2
}
pub(crate) fn set_state(state: u32) {
    let next = if (2..=32).contains(&state) {
        state
    } else {
        UNKNOWN
    };
    if next != UNKNOWN {
        STORY_TEXT.store(false, Ordering::Relaxed);
    }
    if STATE.swap(next, Ordering::Relaxed) != next {
        echo!("[DSfrontendInput] page state={next}");
    }
}
pub(crate) fn observe_menu(state: u32) {
    set_state(state);
    MENU_FRAME.store(
        SCENE_DIALOG_FRAME.load(Ordering::Relaxed),
        Ordering::Relaxed,
    );
}
pub(crate) fn state() -> u32 {
    if enabled() {
        STATE.load(Ordering::Relaxed)
    } else {
        UNKNOWN
    }
}
/// Frontend layouts remain titles; gameplay/pause layouts use the story face.
/// Keep the last verified scene across modal HUD unavailability. Only a real
/// frontend state transition changes it back; bootstrap starts in frontend.
pub(crate) fn native_text_in_story() -> bool {
    enabled() && STORY_TEXT.load(Ordering::Relaxed)
}
pub(crate) fn capture_menu() -> bool {
    loading::active() || native::modal_active()
        || (state() != UNKNOWN
            && menu_recent(
                SCENE_DIALOG_FRAME.load(Ordering::Relaxed),
                MENU_FRAME.load(Ordering::Relaxed),
            ))
}
pub(crate) fn observe_ui_handler(env: &crate::Environment, handler: u32) {
    native::observe_handler(env, handler);
}
pub(crate) fn set_hud_state(state: u32) {
    let previous = HUD_STATE.swap(state, Ordering::Relaxed);
    // HUD state 0/1 is live gameplay. It supersedes a static frontend page;
    // do not keep the menu capture alive merely because no new menu draw
    // occurred.
    if matches!(state, 0 | 1) {
        STATE.store(UNKNOWN, Ordering::Relaxed);
        STORY_TEXT.store(true, Ordering::Relaxed);
    }
    if previous != state && std::env::var("TOUCHHLE_DS_FRONTEND_TRACE").as_deref() == Ok("1") {
        echo!("[DShudState] state={state}");
    }
    HUD_FRAME.store(
        SCENE_DIALOG_FRAME.load(Ordering::Relaxed),
        Ordering::Relaxed,
    );
    if !matches!(state, 0 | 1) {
        PAUSE_PENDING.store(false, Ordering::Relaxed);
    }
}
fn hud_in_confirm(state: u32) -> bool {
    (8..=13).contains(&state)
}
pub(crate) fn capture_scene_dialog() -> bool {
    enabled()
        && hud_in_confirm(HUD_STATE.load(Ordering::Relaxed))
        && menu_recent(
            SCENE_DIALOG_FRAME.load(Ordering::Relaxed),
            HUD_FRAME.load(Ordering::Relaxed),
        )
}
pub(crate) fn request_native_pause() -> bool {
    if !enabled()
        || capture_menu()
        || !matches!(HUD_STATE.load(Ordering::Relaxed), 0 | 1)
        || !menu_recent(
            SCENE_DIALOG_FRAME.load(Ordering::Relaxed),
            HUD_FRAME.load(Ordering::Relaxed),
        )
    {
        return false;
    }
    if !PAUSE_PENDING.swap(true, Ordering::Relaxed) {
        request_pause_command();
    }
    true
}
fn request_pause_command() {
    crate::window::DS_HUD_COMMANDS
        .lock()
        .unwrap()
        .push_back(crate::window::ds_hud_commands::HudCommand::Pause);
}
pub(crate) fn request_native_resume() {
    if enabled() && matches!(HUD_STATE.load(Ordering::Relaxed), 2 | 3) {
        request_pause_command();
    }
}
pub(crate) fn queue_scene_action(action: Action) {
    let bit = match action {
        Action::Confirm => 1,
        Action::Cancel => 2,
        _ => return,
    };
    SCENE_ACTION.fetch_or(bit, Ordering::Relaxed);
}
pub(crate) fn take_scene_action() -> u32 {
    let bits = SCENE_ACTION.swap(0, Ordering::Relaxed);
    if bits & 2 != 0 {
        2
    } else if bits & 1 != 0 {
        1
    } else {
        0
    }
}
pub(crate) fn pause_command_finished() {
    PAUSE_PENDING.store(false, Ordering::Relaxed);
}
pub(crate) fn in_game() -> bool {
    enabled()
        && HUD_STATE.load(Ordering::Relaxed) != UNKNOWN
        && menu_recent(
            SCENE_DIALOG_FRAME.load(Ordering::Relaxed),
            HUD_FRAME.load(Ordering::Relaxed),
        )
}
pub(crate) fn cancel_pending_pause() {
    if PAUSE_PENDING.swap(false, Ordering::Relaxed) {
        crate::window::DS_HUD_COMMANDS
            .lock()
            .unwrap()
            .retain(|c| *c != crate::window::ds_hud_commands::HudCommand::Pause);
    }
}
pub(crate) fn observe_widgets(env: &crate::Environment, owner: u32, page: u32, difficulty: bool) {
    native::observe(env, owner, page, difficulty);
}
pub(crate) fn queue_menu_action(action: Action) {
    if std::env::var("TOUCHHLE_DS_FRONTEND_TRACE").as_deref() == Ok("1") {
        echo!(
            "[DSfrontendInput] queue menu action={:?} state={}",
            action,
            STATE.load(Ordering::Relaxed)
        );
    }
    if loading::active() {
        if action==Action::Confirm {loading::queue();}
    } else {native::queue(action);}
}
pub(crate) fn tick_native(env: &mut crate::Environment) {
    loading::tick(env);
    if !loading::active() {native::tick(env);}
}
pub(crate) fn menu_cursor() -> Option<Point> {
    if loading::active(){return None;}
    native::cursor()
}
pub(crate) fn hover(point: Point) {
    if loading::active(){return;}
    native::hover(point);
}
pub(crate) fn hint_text() -> Option<&'static str> {
    if (!capture_menu() && !capture_scene_dialog())
        || (!cfg!(target_os = "windows") && INPUT_DEVICE.load(Ordering::Relaxed) == 0)
    {
        return None;
    }
    let cancel = if capture_menu() {
        native::can_cancel()
    } else {
        true
    };
    Some(match (keyboard_hints(), cancel) {
        (true, true) => "KEYBOARD  E OK  Q BACK",
        (true, false) => "KEYBOARD  E OK",
        (false, true) => "GAMEPAD  A OK  B/X BACK",
        (false, false) => "GAMEPAD  A OK",
    })
}
/// A small focus bracket complements the original widget selection.
/// Button legends are sampled from the shared prompt texture atlas.
pub(crate) fn overlay() -> Vec<([f32; 12], [f32; 4])> {
    if hint_text().is_none() || loading::active() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(r) = native::focus_rect().filter(|_|native::modal_active()) {
        let x = (r[0] - 1.).clamp(2., 478.);
        let y = (r[1] - 1.).clamp(2., 318.);
        let w = (r[2] + 2.).min(478. - x).max(0.);
        let h = (r[3] + 2.).min(318. - y).max(0.);
        if w >= 8. && h >= 8. {
            let c = 5.;
            let t = 0.7;
            let color = [0.42, 0.79, 0.84, 1.];
            for (px, py, pw, ph) in [
                (x, y, c, t),
                (x, y, t, c),
                (x + w - c, y, c, t),
                (x + w - t, y, t, c),
                (x, y + h - t, c, t),
                (x, y + h - c, t, c),
                (x + w - c, y + h - t, c, t),
                (x + w - t, y + h - c, t, c),
            ] {
                push_rect(&mut out, px, py, pw, ph, color);
            }
        }
    }
    out
}

pub(crate) fn observe_loading(env:&crate::Environment, owner:u32){loading::observe(env,owner);}
pub(crate) fn loading_ready()->bool {loading::ready()}
pub(crate) fn cancel_loading_input(){loading::cancel();}

pub(crate) fn modal_active()->bool { native::modal_active() }
pub(crate) fn home_page()->bool {capture_menu() && state()==5 && !native::modal_active()}
pub(crate) fn delete_rect()->Option<[f32;4]> {native::delete_rect()}

pub(crate) fn prompt_controls() -> (Vec<([f32; 4], bool)>, Option<[f32; 4]>) {
    if hint_text().is_none() || loading::active() {
        return (Vec::new(), None);
    }
    (native::action_rects(), native::navigation_rect())
}

pub(crate) fn key_row_anchor(rect: [f32; 4], cancel: bool) -> Point {
    key_row_anchor_width(rect,cancel,if cancel {35.}else{52.})
}
pub(crate) fn key_row_anchor_width(rect:[f32;4],cancel:bool,width:f32)->Point {
    let height = 14.;
    // Adjacent main-menu buttons have no side gap: put the legend above
    // its own button instead of drawing it over the neighbouring Options icon.
    let (x, y) = if rect[1]>=250. && (rect[2]>=85. || (!cancel && rect[0]>=350.)) {
        (rect[0]+rect[2]/2.-width/2., rect[1]-height-4.)
    } else if rect[0] >= width + 8. {
        (rect[0] - width - 6., rect[1] + rect[3] / 2. - height / 2.)
    } else if rect[0] + rect[2] + width + 8. <= 480. {
        (rect[0] + rect[2] + 6., rect[1] + rect[3] / 2. - height / 2.)
    } else {
        (rect[0] + rect[2] / 2. - width / 2., rect[1] - height - 4.)
    };
    Point::new(x.clamp(3., 477. - width), y.clamp(3., 303.))
}

fn push_rect(
    out: &mut Vec<([f32; 12], [f32; 4])>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
) {
    let (l, r) = (x / 240. - 1., (x + w) / 240. - 1.);
    let (t, b) = (1. - y / 160., 1. - (y + h) / 160.);
    out.push(([l, t, r, t, r, b, l, t, r, b, l, b], color));
}

#[cfg(test)]
mod tests {
    use super::*;
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    #[test]
    fn native_text_roles_follow_bootstrap_gameplay_pause_and_return_to_menu() {
        let _guard = TEST_LOCK.lock().unwrap();
        activate();
        assert!(!native_text_in_story());
        observe_menu(5);
        assert!(!native_text_in_story());
        set_hud_state(0);
        assert!(native_text_in_story());
        set_hud_state(2);
        assert!(native_text_in_story());
        // ds_current_hud_player intentionally returns no player while the
        // native pause handler owns input. Its fonts still belong to gameplay.
        set_hud_state(UNKNOWN);
        assert!(native_text_in_story());
        set_hud_state(2);
        assert!(native_text_in_story());
        observe_menu(5);
        assert!(!native_text_in_story());
        set_hud_state(UNKNOWN);
        assert!(!native_text_in_story());
        deactivate();
    }
    #[test]
    fn stale_or_unknown_pages_never_capture_gameplay() {
        let _guard = TEST_LOCK.lock().unwrap();
        deactivate();
        observe_frame(100);
        assert!(!capture_menu());
        activate();
        assert_eq!(state(), UNKNOWN);
        observe_menu(5);
        assert!(capture_menu());
        observe_frame(102);
        assert!(capture_menu());
        observe_frame(103);
        assert!(!capture_menu());
        set_hud_state(0);
        assert!(!capture_menu());
        assert!(hint_text().is_none());
        observe_menu(33);
        assert!(!capture_menu());
        deactivate();
    }
    #[test]
    fn confirmation_eligibility_matches_native_hud_predicate() {
        let _guard = TEST_LOCK.lock().unwrap();
        for state in 0..20 {
            assert_eq!(hud_in_confirm(state), state >= 8 && state <= 13);
        }
        assert!(!hud_in_confirm(UNKNOWN));
    }
    #[test]
    fn enter_keys_confirm_without_capturing_gameplay_movement() {
        use sdl2::keyboard::Keycode as K;
        for key in [K::E, K::Return, K::KpEnter] {
            assert_eq!(keyboard_action(key), Some(Action::Confirm));
        }
        for key in [K::Q, K::X] {
            assert_eq!(keyboard_action(key), Some(Action::Cancel));
        }
        assert_eq!(keyboard_action(K::W), None);
    }
    #[test]
    fn control_key_rows_stay_beside_actual_controls_and_inside_the_viewport() {
        for rect in [
            [391., 271., 60., 49.],
            [30., 271., 60., 49.],
            [0., 0., 50., 15.],
            [450., 305., 30., 15.],
        ] {
            for cancel in [false, true] {
                let at = key_row_anchor(rect, cancel);
                let width = if cancel { 35. } else { 52. };
                assert!(at.x >= 0. && at.x + width <= 480.);
                assert!(at.y >= 0. && at.y + 14. <= 320.);
                assert!((at.y - rect[1]).abs() < 40.);
            }
        }
    }
    #[test]
    fn scene_cancel_wins_over_confirm_before_consumption() {
        let _guard = TEST_LOCK.lock().unwrap();
        deactivate();
        activate();
        queue_scene_action(Action::Confirm);
        queue_scene_action(Action::Cancel);
        assert_eq!(take_scene_action(), 2);
        assert_eq!(take_scene_action(), 0);
        queue_scene_action(Action::Confirm);
        assert_eq!(take_scene_action(), 1);
        deactivate();
    }
    #[test]
    fn overlay_geometry_is_finite_for_both_input_devices() {
        let _guard = TEST_LOCK.lock().unwrap();
        deactivate();
        activate();
        observe_frame(1);
        observe_menu(5);
        INPUT_DEVICE.store(1, Ordering::Relaxed);
        assert!(overlay()
            .iter()
            .all(|(v, _)| v.iter().all(|x| x.is_finite())));
        assert!(overlay()
            .iter()
            .all(|(v, _)| v.iter().all(|x| *x >= -1.0 && *x <= 1.0)));
        INPUT_DEVICE.store(2, Ordering::Relaxed);
        assert!(overlay()
            .iter()
            .all(|(v, _)| v.iter().all(|x| x.is_finite())));
        assert!(overlay()
            .iter()
            .all(|(v, _)| v.iter().all(|x| *x >= -1.0 && *x <= 1.0)));
        deactivate();
    }
}

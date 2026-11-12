/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Abstraction of window setup, OpenGL context creation and event handling.
//!
//! Implemented using the sdl2 crate (a Rust wrapper for SDL2). All usage of
//! SDL should be confined to this module.
//!
//! There is currently no separation of concerns between a single window and
//! window system interaction in general, because it is assumed only one window
//! will be needed for the runtime of the app.

use crate::gles::present::present_frame;
#[path = "ds_key_bindings.rs"]
mod ds_key_bindings;
#[path = "ds_gamepad_bindings.rs"]
mod ds_gamepad_bindings;
#[path = "ds_held_inputs.rs"]
mod ds_held_inputs;
#[path = "ds_pc_input.rs"]
pub(crate) mod ds_pc_input;
#[path = "ds_frame_journal.rs"]
mod ds_frame_journal;
#[path = "ds_mouse_capture.rs"]
mod ds_mouse_capture;
#[path = "ds_control_inbox.rs"]
mod ds_control_inbox;
#[path = "ds_hud_commands.rs"]
pub(crate) mod ds_hud_commands;
#[path = "ds_weapon_input.rs"]
mod ds_weapon_input;

const DS_GAMEPAD_START_HOLD: Duration = Duration::from_millis(650);

pub(crate) static DS_HUD_COMMANDS: std::sync::Mutex<VecDeque<ds_hud_commands::HudCommand>> = std::sync::Mutex::new(VecDeque::new());
fn ds_native_hud_enabled() -> bool { std::env::var("TOUCHHLE_DS_NATIVE_HUD").as_deref() == Ok("1") }
fn ds_queue_hud(command: ds_hud_commands::HudCommand) {
    let mut queue = DS_HUD_COMMANDS.lock().unwrap();
    ds_weapon_input::enqueue(&mut queue,command);
}
static DS_WEAPON_COOLDOWN:std::sync::Mutex<ds_weapon_input::Cooldown>=std::sync::Mutex::new(ds_weapon_input::Cooldown::new());
fn ds_weapon_ms()->u64 {
    static CLOCK:std::sync::OnceLock<Instant>=std::sync::OnceLock::new();
    CLOCK.get_or_init(Instant::now).elapsed().as_millis().min(u64::MAX as u128) as u64
}
pub(crate) fn ds_take_hud_command()->Option<ds_hud_commands::HudCommand> {
    let mut queue=DS_HUD_COMMANDS.lock().unwrap();
    ds_weapon_input::pop(&mut queue,&mut DS_WEAPON_COOLDOWN.lock().unwrap(),ds_weapon_ms())
}

fn ds_input_trace_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("TOUCHHLE_DS_INPUT_TRACE").is_some())
}

fn ds_input_mode() -> ds_gamepad_bindings::InputMode {
    static MODE: std::sync::OnceLock<ds_gamepad_bindings::InputMode> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| ds_gamepad_bindings::InputMode::parse(
        &std::env::var("TOUCHHLE_DS_INPUT_MODE").unwrap_or_else(|_| "auto".into()))
        .unwrap_or_else(|error| { log!("[DSinput] {error}; using auto"); ds_gamepad_bindings::InputMode::Auto }))
}

#[path="ds_test_pad.rs"]
mod ds_test_pad;

#[path="ds_fire_control.rs"]
pub(crate) mod ds_fire_control;
pub(crate) fn ds_pad_mode_toggle()->bool {std::env::var("TOUCHHLE_DS_CONTROLLER_FIRE_MODE").as_deref()!=Ok("split")}
pub(crate) fn ds_keyboard_alt_hold()->bool {std::env::var("TOUCHHLE_DS_KEYBOARD_ALT_MODE").as_deref()!=Ok("toggle")}

fn ds_gamepad_semantic()->bool {
    std::env::var("TOUCHHLE_DS_GAMEPAD_SEMANTIC").map(|v|v!="0").unwrap_or(true)
}

fn ds_pad_map() -> &'static HashMap<String, String> {
    static MAP: std::sync::OnceLock<HashMap<String, String>> = std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        let loaded = std::env::var("TOUCHHLE_DS_GAMEPAD_BINDINGS").ok().map(|path|
            std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|text| ds_gamepad_bindings::parse(&text)));
        match loaded {
            Some(Ok(map)) => map,
            Some(Err(error)) => { log!("[DSpad] rejected bindings: {error}; using defaults"); ds_gamepad_bindings::parse("").unwrap() }
            None => ds_gamepad_bindings::parse("").unwrap(),
        }
    })
}

fn ds_pad_button(name: &str) -> Option<sdl2::controller::Button> {
    use sdl2::controller::Button as B;
    let name = ds_gamepad_bindings::action_button(name).unwrap_or(name);
    Some(match name { "A" => B::A, "B" => B::B, "X" => B::X, "Y" => B::Y,
        "LB" => B::LeftShoulder, "RB" => B::RightShoulder, "START" => B::Start, "BACK" => B::Back, "LS" => B::LeftStick, "RS" => B::RightStick, _ => return None })
}

fn ds_remap_pad(button: sdl2::controller::Button, reverse: bool) -> sdl2::controller::Button {
    for (physical, semantic) in ds_pad_map() {
        // `parse` returns physical -> semantic. Resolve the semantic side
        // through the canonical button table instead of treating action names
        // such as `interact` as SDL button names.
        let from = if reverse { semantic.as_str() } else { physical.as_str() };
        let to = if reverse { physical.as_str() } else { semantic.as_str() };
        if ds_pad_button(from) == Some(button) {
            return ds_pad_button(to).unwrap_or(button);
        }
    }
    button
}

fn ds_frontend_pad_action(
    button: sdl2::controller::Button,
) -> Option<crate::ds_frontend_controller::Action> {
    use crate::ds_frontend_controller::Action;
    use sdl2::controller::Button as B;
    Some(match button {
        B::DPadUp => Action::Up,
        B::DPadDown => Action::Down,
        B::DPadLeft => Action::Left,
        B::DPadRight => Action::Right,
        B::A => Action::Confirm,
        B::B => Action::Cancel,
        B::X => Action::Cancel,
        B::Back => Action::Cancel,
        B::Start => Action::Delete,
        _ => return None,
    })
}

fn ds_key_map() -> &'static Option<HashMap<String,String>> {
    static MAP: std::sync::OnceLock<Option<HashMap<String, String>>> = std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        let path = std::env::var("TOUCHHLE_DS_KEY_BINDINGS").ok()?;
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| ds_key_bindings::parse(&text))
        {
            Ok(map) => Some(map),
            Err(error) => {
                log!("[DSbindings] rejected {}: {}; using defaults", path, error);
                None
            }
        }
    })
}
pub(crate) fn ds_prompt_key(action:&str)->String {
    let semantic=ds_key_bindings::ACTIONS.iter().find(|(a,_)| *a==action).map(|(_,k)|*k).unwrap_or(action);
    ds_key_map().as_ref().and_then(|map|map.iter().find(|(_,s)|s.as_str()==semantic).map(|(p,_)|p.clone())).unwrap_or_else(||semantic.to_owned())
}
pub(crate) fn ds_locator_prompt_keys()->Vec<String> {
    ds_key_bindings::locator_keys(ds_key_map().as_ref())
}
pub(crate) fn ds_prompt_button(action:&str)->String {
    let semantic=ds_gamepad_bindings::action_button(action).unwrap_or(action);
    ds_pad_map().iter().find(|(_,s)|s.as_str()==semantic).map(|(p,_)|p.clone()).unwrap_or_else(||semantic.to_owned())
}
fn ds_remap_key(key:sdl2::keyboard::Keycode)->Option<sdl2::keyboard::Keycode> {
    use sdl2::keyboard::Keycode;
    let name = if matches!(key, Keycode::LCtrl | Keycode::RCtrl) { "CTRL".into() } else { key.name().to_ascii_uppercase() };
    let semantic = ds_key_bindings::resolve(ds_key_map().as_ref(), &name)?;
    if semantic == "CTRL" || semantic == "zero_gravity_jump" { Some(Keycode::LCtrl) }
    else if semantic == name { Some(key) }
    else { Keycode::from_name(semantic) }
}
use crate::gles::{
    create_gles1_ctx_no_parent_stack, create_gles2_ctx_no_parent_stack, GLESContext, GLES,
};
use crate::image::Image;
use crate::matrix::Matrix;
use crate::options::Options;
use crate::Environment;
use sdl2::mouse::MouseButton;
use sdl2::pixels::PixelFormatEnum;
use sdl2::surface::Surface;
use sdl2_sys::SDL_PowerState;
use std::collections::{HashMap, VecDeque};
use std::env;
use std::f32::consts::{FRAC_PI_2, PI};
use std::num::NonZeroU32;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

#[cfg(windows)]
fn allow_foreground_process(process_id: u32) {
    #[link(name = "user32")]
    extern "system" { fn AllowSetForegroundWindow(process_id: u32) -> i32; }
    unsafe { let _ = AllowSetForegroundWindow(process_id); }
}
#[cfg(not(windows))]
fn allow_foreground_process(_process_id: u32) {}

#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DeviceFamily {
    iPhone,
    iPhone3G,
    iPhone3GS,
    iPhone4,
    iPhone4s,
    iPhone5,
    iPhone5c,
    iPad,
    iPad2,
    iPad3,
    iPad4,
    iPad5,
    iPadMini,
    iPadMini2,
    iPadMini3,
    iPodTouch,
    iPodTouch2,
    iPodTouch3,
    iPodTouch4,
    iPodTouch5,
}
impl std::fmt::Display for DeviceFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}
impl DeviceFamily {
    pub fn display_name(&self) -> &'static str {
        match self {
            DeviceFamily::iPhone => "iPhone",
            DeviceFamily::iPhone3G => "iPhone 3G",
            DeviceFamily::iPhone3GS => "iPhone 3GS",
            DeviceFamily::iPhone4 => "iPhone 4",
            DeviceFamily::iPhone4s => "iPhone 4s",
            DeviceFamily::iPhone5 => "iPhone 5",
            DeviceFamily::iPhone5c => "iPhone 5c",
            DeviceFamily::iPad => "iPad",
            DeviceFamily::iPad2 => "iPad 2",
            DeviceFamily::iPad3 => "iPad 3",
            DeviceFamily::iPad4 => "iPad 4",
            DeviceFamily::iPad5 => "iPad 5",
            DeviceFamily::iPadMini => "iPad mini",
            DeviceFamily::iPadMini2 => "iPad mini 2",
            DeviceFamily::iPadMini3 => "iPad mini 3",
            DeviceFamily::iPodTouch => "iPod touch",
            DeviceFamily::iPodTouch2 => "iPod touch 2",
            DeviceFamily::iPodTouch3 => "iPod touch 3",
            DeviceFamily::iPodTouch4 => "iPod touch 4",
            DeviceFamily::iPodTouch5 => "iPod touch 5",
        }
    }

    pub fn is_ipad(&self) -> bool {
        matches!(
            self,
            DeviceFamily::iPad
                | DeviceFamily::iPad2
                | DeviceFamily::iPad3
                | DeviceFamily::iPad4
                | DeviceFamily::iPad5
                | DeviceFamily::iPadMini
                | DeviceFamily::iPadMini2
                | DeviceFamily::iPadMini3
        )
    }

    pub fn is_ipod_touch(&self) -> bool {
        matches!(
            self,
            DeviceFamily::iPodTouch
                | DeviceFamily::iPodTouch2
                | DeviceFamily::iPodTouch3
                | DeviceFamily::iPodTouch4
                | DeviceFamily::iPodTouch5
        )
    }

    pub fn is_phone_568(&self) -> bool {
        matches!(
            self,
            DeviceFamily::iPhone5 | DeviceFamily::iPhone5c | DeviceFamily::iPodTouch5
        )
    }

    pub fn is_retina(&self) -> bool {
        matches!(
            self,
            DeviceFamily::iPhone4
                | DeviceFamily::iPhone4s
                | DeviceFamily::iPhone5
                | DeviceFamily::iPhone5c
                | DeviceFamily::iPodTouch4
                | DeviceFamily::iPodTouch5
                | DeviceFamily::iPad3
                | DeviceFamily::iPad4
                | DeviceFamily::iPad5
                | DeviceFamily::iPadMini2
                | DeviceFamily::iPadMini3
        )
    }

    /// Portrait (width, height) in logical points.
    pub fn portrait_size(&self) -> (u32, u32) {
        if self.is_ipad() {
            (768, 1024)
        } else if self.is_phone_568() {
            (320, 568)
        } else {
            (320, 480)
        }
    }

    /// UIScreen.scale — retina multiplier.
    pub fn scale_factor(&self) -> f32 {
        if self.is_retina() {
            2.0
        } else {
            1.0
        }
    }

    /// sysctl hw.machine / uname machine string.
    pub fn machine_name(&self) -> &'static str {
        match self {
            DeviceFamily::iPhone => "iPhone1,1",
            DeviceFamily::iPhone3G => "iPhone1,2",
            DeviceFamily::iPhone3GS => "iPhone2,1",
            DeviceFamily::iPhone4 => "iPhone3,1",
            DeviceFamily::iPhone4s => "iPhone4,1",
            DeviceFamily::iPhone5 => "iPhone5,1",
            DeviceFamily::iPhone5c => "iPhone5,3",
            DeviceFamily::iPad => "iPad1,1",
            DeviceFamily::iPad2 => "iPad2,1",
            DeviceFamily::iPad3 => "iPad3,1",
            DeviceFamily::iPad4 => "iPad3,4",
            DeviceFamily::iPad5 => "iPad6,11",
            DeviceFamily::iPadMini => "iPad2,5",
            DeviceFamily::iPadMini2 => "iPad4,4",
            DeviceFamily::iPadMini3 => "iPad4,7",
            DeviceFamily::iPodTouch => "iPod1,1",
            DeviceFamily::iPodTouch2 => "iPod2,1",
            DeviceFamily::iPodTouch3 => "iPod3,1",
            DeviceFamily::iPodTouch4 => "iPod4,1",
            DeviceFamily::iPodTouch5 => "iPod5,1",
        }
    }

    /// Amount of physical RAM (in bytes) the modelled device shipped with.
    ///
    /// Used by `NSProcessInfo.physicalMemory`, the `host_statistics`
    /// VM page counts and the `hw.memsize` / `hw.physmem` sysctls so that
    /// every memory-reporting API agrees with the chosen device family.
    /// Reporting too little (the old code reported ~50 MiB regardless of
    /// device) makes memory-budgeting engines such as Unreal Engine 3
    /// (UDKGame / Batman: Arkham City Lockdown) compute a bogus pool size and
    /// then attempt absurd allocations that fail, while reporting too much on
    /// a 32-bit address space risks overflow.
    /// Values come from Apple's published hardware specifications for each
    /// model.
    pub fn physical_memory(&self) -> u64 {
        const MIB: u64 = 1024 * 1024;
        match self {
            // Early devices shipped with 128 MiB of RAM.
            DeviceFamily::iPhone
            | DeviceFamily::iPhone3G
            | DeviceFamily::iPodTouch
            | DeviceFamily::iPodTouch2
            | DeviceFamily::iPodTouch3 => 128 * MIB,
            // iPhone 3GS / 4, iPad 1, iPod touch 4 shipped with 256 MiB.
            DeviceFamily::iPhone3GS
            | DeviceFamily::iPhone4
            | DeviceFamily::iPad
            | DeviceFamily::iPodTouch4 => 256 * MIB,
            // iPhone 4S / 5 / 5c, iPad 2/3/4/mini, iPod touch 5 shipped with
            // 512 MiB.
            DeviceFamily::iPhone4s
            | DeviceFamily::iPhone5
            | DeviceFamily::iPhone5c
            | DeviceFamily::iPad2
            | DeviceFamily::iPad3
            | DeviceFamily::iPad4
            | DeviceFamily::iPadMini
            | DeviceFamily::iPodTouch5 => 512 * MIB,
            // iPad 5 (2017) and the Retina iPad minis shipped with 1 GiB. We
            // intentionally cap this at 1 GiB even though the address space is
            // 4 GiB, leaving headroom for the guest heap, stacks and mapped
            // libraries.
            DeviceFamily::iPad5 | DeviceFamily::iPadMini2 | DeviceFamily::iPadMini3 => 1024 * MIB,
        }
    }

    /// Heuristically pick the emulated device family whose screen most closely
    /// matches an arbitrary host screen of `(width, height)` physical pixels.
    pub fn pick_for_screen(width: u32, height: u32) -> DeviceFamily {
        if width == 0 || height == 0 {
            return DeviceFamily::iPhone3GS;
        }
        let (short, long) = if width <= height {
            (width as f32, height as f32)
        } else {
            (height as f32, width as f32)
        };
        let host_ratio = short / long;
        const CANDIDATES: [DeviceFamily; 5] = [
            DeviceFamily::iPhone3GS,
            DeviceFamily::iPhone4,
            DeviceFamily::iPhone5,
            DeviceFamily::iPad2,
            DeviceFamily::iPad3,
        ];
        let mut best = DeviceFamily::iPhone3GS;
        let mut best_dist = f32::INFINITY;
        for family in CANDIDATES {
            let (w, h) = family.portrait_size();
            let ratio = w as f32 / h as f32;
            let dist = (ratio - host_ratio).abs();
            if dist < best_dist {
                best_dist = dist;
                best = family;
            }
        }
        best
    }

    /// CLI/option canonical name accepted by `--device-family=` and emitted by
    /// the app picker. Round-trips through `TryFrom<&str>`.
    pub fn option_name(&self) -> &'static str {
        match self {
            DeviceFamily::iPhone => "iphone-2g",
            DeviceFamily::iPhone3G => "iphone-3g",
            DeviceFamily::iPhone3GS => "iphone-3gs",
            DeviceFamily::iPhone4 => "iphone-4",
            DeviceFamily::iPhone4s => "iphone-4s",
            DeviceFamily::iPhone5 => "iphone-5",
            DeviceFamily::iPhone5c => "iphone-5c",
            DeviceFamily::iPad => "ipad-1",
            DeviceFamily::iPad2 => "ipad-2",
            DeviceFamily::iPad3 => "ipad-3",
            DeviceFamily::iPad4 => "ipad-4",
            DeviceFamily::iPad5 => "ipad-5",
            DeviceFamily::iPadMini => "ipad-mini",
            DeviceFamily::iPadMini2 => "ipad-mini-2",
            DeviceFamily::iPadMini3 => "ipad-mini-3",
            DeviceFamily::iPodTouch => "ipod-touch",
            DeviceFamily::iPodTouch2 => "ipod-touch-2",
            DeviceFamily::iPodTouch3 => "ipod-touch-3",
            DeviceFamily::iPodTouch4 => "ipod-touch-4",
            DeviceFamily::iPodTouch5 => "ipod-touch-5",
        }
    }

    /// Every device model the user can pick, in menu/display order. Used by the
    /// app picker's Quick Options "Device model" selector and by docs.
    pub const ALL_SELECTABLE: &'static [DeviceFamily] = &[
        DeviceFamily::iPhone,
        DeviceFamily::iPhone3G,
        DeviceFamily::iPhone3GS,
        DeviceFamily::iPhone4,
        DeviceFamily::iPhone4s,
        DeviceFamily::iPhone5,
        DeviceFamily::iPhone5c,
        DeviceFamily::iPad,
        DeviceFamily::iPad2,
        DeviceFamily::iPad3,
        DeviceFamily::iPad4,
        DeviceFamily::iPad5,
        DeviceFamily::iPadMini,
        DeviceFamily::iPadMini2,
        DeviceFamily::iPadMini3,
        DeviceFamily::iPodTouch,
        DeviceFamily::iPodTouch2,
        DeviceFamily::iPodTouch3,
        DeviceFamily::iPodTouch4,
        DeviceFamily::iPodTouch5,
    ];
}
impl TryFrom<u64> for DeviceFamily {
    type Error = ();
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(DeviceFamily::iPhone),
            2 => Ok(DeviceFamily::iPad),
            _ => Err(()),
        }
    }
}
impl TryFrom<&str> for DeviceFamily {
    type Error = ();
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.to_ascii_lowercase().as_str() {
            "iphone" => Ok(DeviceFamily::iPhone3GS),
            "iphone-2g" | "iphone1,1" => Ok(DeviceFamily::iPhone),
            "iphone-3g" | "iphone1,2" => Ok(DeviceFamily::iPhone3G),
            "iphone-3gs" | "iphone2,1" => Ok(DeviceFamily::iPhone3GS),
            "iphone-4" | "iphone3,1" => Ok(DeviceFamily::iPhone4),
            "iphone-4s" | "iphone4,1" => Ok(DeviceFamily::iPhone4s),
            "iphone-5" | "iphone5,1" => Ok(DeviceFamily::iPhone5),
            "iphone-5c" | "iphone5,3" => Ok(DeviceFamily::iPhone5c),
            "ipad" => Ok(DeviceFamily::iPad2),
            "ipad-1" | "ipad1,1" => Ok(DeviceFamily::iPad),
            "ipad-2" | "ipad2,1" => Ok(DeviceFamily::iPad2),
            "ipad-3" | "ipad3,1" => Ok(DeviceFamily::iPad3),
            "ipad-4" | "ipad3,4" => Ok(DeviceFamily::iPad4),
            "ipad-5" | "ipad6,11" => Ok(DeviceFamily::iPad5),
            "ipad-mini" | "ipad2,5" => Ok(DeviceFamily::iPadMini),
            "ipad-mini-2" | "ipad4,4" => Ok(DeviceFamily::iPadMini2),
            "ipad-mini-3" | "ipad4,7" => Ok(DeviceFamily::iPadMini3),
            "ipod-touch" | "ipod1,1" => Ok(DeviceFamily::iPodTouch),
            "ipod-touch-2" | "ipod2,1" => Ok(DeviceFamily::iPodTouch2),
            "ipod-touch-3" | "ipod3,1" => Ok(DeviceFamily::iPodTouch3),
            "ipod-touch-4" | "ipod4,1" => Ok(DeviceFamily::iPodTouch4),
            "ipod-touch-5" | "ipod5,1" => Ok(DeviceFamily::iPodTouch5),
            _ => Err(()),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum DeviceOrientation {
    Portrait,
    PortraitUpsideDown,
    LandscapeLeft,
    LandscapeRight,
}
fn normalize_portrait_size(size: (u32, u32)) -> (u32, u32) {
    if size.0 <= size.1 {
        size
    } else {
        (size.1, size.0)
    }
}

fn size_for_orientation_from_size(
    size: (u32, u32),
    orientation: DeviceOrientation,
    scale_hack: NonZeroU32,
) -> (u32, u32) {
    let (width, height) = size;
    let scale_hack = scale_hack.get();
    match orientation {
        DeviceOrientation::Portrait => (width * scale_hack, height * scale_hack),
        DeviceOrientation::PortraitUpsideDown => (width * scale_hack, height * scale_hack),
        DeviceOrientation::LandscapeLeft => (height * scale_hack, width * scale_hack),
        DeviceOrientation::LandscapeRight => (height * scale_hack, width * scale_hack),
    }
}

fn size_for_orientation(
    family: DeviceFamily,
    orientation: DeviceOrientation,
    scale_hack: NonZeroU32,
) -> (u32, u32) {
    size_for_orientation_from_size(family.portrait_size(), orientation, scale_hack)
}
fn rotate_fullscreen_size(orientation: DeviceOrientation, screen_size: (u32, u32)) -> (u32, u32) {
    let (short_side, long_side) = if screen_size.0 < screen_size.1 {
        (screen_size.0, screen_size.1)
    } else {
        (screen_size.1, screen_size.0)
    };
    match orientation {
        DeviceOrientation::Portrait | DeviceOrientation::PortraitUpsideDown => {
            (short_side, long_side)
        }
        DeviceOrientation::LandscapeLeft | DeviceOrientation::LandscapeRight => {
            (long_side, short_side)
        }
    }
}
/// Tell SDL2 what orientation we want. Only useful on Android.
fn set_sdl2_orientation(orientation: DeviceOrientation) {
    // Despite the name, this hint works on Android too.
    sdl2::hint::set(
        "SDL_IOS_ORIENTATIONS",
        match orientation {
            DeviceOrientation::Portrait => "Portrait",
            // The inversion is deliberate. These probably correspond to
            // iPhone OS content orientations?
            DeviceOrientation::PortraitUpsideDown => "PortraitUpsideDown",
            DeviceOrientation::LandscapeLeft => "LandscapeRight",
            DeviceOrientation::LandscapeRight => "LandscapeLeft",
        },
    );
}

/// A tap requested by the *emulator itself* (see `TOUCHHLE_DS_TRACK_TAP`),
/// packed as `(x << 16) | y` in the same 480x320 space as `TOUCHHLE_DS_SCRIPT`;
/// 0 = nothing pending. The HUD state tracker sets it the moment the byte it
/// watches turns non-zero - which is exactly when the game starts treating an
/// object as interactive (DECOMPILE.md D-004). The script-action loop below
/// turns it into an ordinary tap, so the click lands at the right frame without
/// any host-side timing guesswork.
pub(crate) static AUTO_TAP: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum FingerId {
    Mouse,
    /// Held finger used for PC-style right-button aiming (TOUCHHLE_DS_AIM_XY).
    MouseAim,
    /// Transient finger used for PC-style left-button firing while aiming.
    MouseFire,
    /// Held finger used by captured relative-mouse camera look.
    Look,
    Touch(i64),
    VirtualCursor,
    ButtonToTouch(crate::options::Button),
    StickToTouch,
    DpadToTouch,
}
pub type Coords = (f32, f32);

struct DpadState {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    active: bool,
}

#[derive(Debug)]
pub enum TextInputEvent {
    Text(String),
    Backspace,
    Return,
}

#[derive(Debug)]
pub enum Event {
    /// User requested quit.
    Quit,
    /// OS has informed touchHLE it will soon become inactive.
    /// (iOS `applicationWillResignActive:`, Android `onPause()`)
    AppWillResignActive,
    /// The app has fully entered the background.
    AppDidEnterBackground,
    /// The app is about to return to the foreground.
    AppWillEnterForeground,
    /// The app is active again and the guest run loop can continue.
    AppDidBecomeActive,
    /// OS has informed touchHLE it will soon terminate.
    /// (iOS `applicationWillTerminate:`, Android `onDestroy()`)
    AppWillTerminate,
    TouchesDown(HashMap<FingerId, Coords>),
    TouchesMove(HashMap<FingerId, Coords>),
    TouchesUp(HashMap<FingerId, Coords>),
    TouchesCancelled(HashMap<FingerId, Coords>),
    /// User pressed F12, requesting that execution be paused and the debugger
    /// take over.
    EnterDebugger,
    /// User pressed Shift+F5, requesting a guest memory + CPU save-state dump.
    SaveState,
    /// User pressed Shift+F9, requesting a restore of the most recent save-state.
    LoadState,
    TextInput(TextInputEvent),
}

pub enum BatteryState {
    Unknown,
    OnBattery,
    NoBattery,
    Charging,
    Full,
}

pub enum GLVersion {
    /// OpenGL ES 1.1
    GLES11,
    /// OpenGL ES 2.0
    GLES20,
    /// OpenGL ES 3.0
    GLES30,
    /// OpenGL 2.1 compatibility profile
    GL21Compat,
    /// OpenGL 3.3 Core profile
    GL33Core,
}

pub struct GLContext(sdl2::video::GLContext);

impl GLContext {
    pub fn is_current(&self) -> bool {
        self.0.is_current()
    }
}

fn surface_from_image(image: &Image) -> Surface<'_> {
    let src_pixels = image.pixels();
    let (width, height) = image.dimensions();

    let mut surface = Surface::new(width, height, PixelFormatEnum::RGBA32).unwrap();
    let (width, height) = (width as usize, height as usize);
    let pitch = surface.pitch() as usize;
    surface.with_lock_mut(|dst_pixels| {
        for y in 0..height {
            for x in 0..width {
                for channel in 0..4 {
                    let src_idx = y * width * 4 + x * 4 + channel;
                    let dst_idx = y * pitch + x * 4 + channel;
                    dst_pixels[dst_idx] = src_pixels[src_idx];
                }
            }
        }
    });
    surface
}

/// Query the host's primary display size in physical pixels, if possible.
///
/// Used by `--device-family=auto` to pick the closest emulated device before
/// the real [Window] is created. Spins up a throwaway SDL video subsystem; on
/// any failure (e.g. headless host, no display) returns [None] so the caller
/// can fall back to a sensible default.
pub fn host_screen_size() -> Option<(u32, u32)> {
    let sdl_ctx = sdl2::init().ok()?;
    let video_ctx = sdl_ctx.video().ok()?;
    let bounds = video_ctx.display_bounds(0).ok()?;
    let (w, h) = bounds.size();
    if w == 0 || h == 0 {
        None
    } else {
        Some((w, h))
    }
}

/// Frames presented so far (bumped in the EAGL present path). Used as the clock
/// for the scripted-input executor below.
pub static DS_FRAME_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct DsFrameHistory {
    last_present: Option<std::time::Instant>,
    intervals_us: std::collections::VecDeque<u32>,
}

static DS_FRAME_HISTORY: std::sync::OnceLock<std::sync::Mutex<DsFrameHistory>> =
    std::sync::OnceLock::new();

/// Record one actual `presentRenderbuffer` event. Android's FPS overlay and
/// optional present diagnostics use this instead of compositor/vsync callbacks.
pub fn ds_record_present_frame() {
    let now = std::time::Instant::now();
    let history = DS_FRAME_HISTORY.get_or_init(|| {
        std::sync::Mutex::new(DsFrameHistory {
            last_present: None,
            intervals_us: std::collections::VecDeque::with_capacity(1024),
        })
    });
    let Ok(mut history) = history.lock() else {
        return;
    };
    if let Some(last) = history.last_present.replace(now) {
        let dt_us = now.duration_since(last).as_micros();
        // Keep the complete opt-in capture separate from the HUD's rolling,
        // filtered history: loading/suspend gaps are real measured intervals.
        static JOURNAL: std::sync::OnceLock<Option<std::sync::Mutex<ds_frame_journal::Journal>>> =
            std::sync::OnceLock::new();
        if let Some(journal) = JOURNAL.get_or_init(|| {
            let path = std::env::var_os("TOUCHHLE_DS_FRAME_TIMES")?;
            match ds_frame_journal::Journal::open(std::path::Path::new(&path)) {
                Ok(journal) => {
                    log!("[DSframe-times] recording actual guest presents to {:?}", path);
                    Some(std::sync::Mutex::new(journal))
                }
                Err(error) => {
                    log!("[DSframe-times] capture disabled: {}", error);
                    None
                }
            }
        }) {
            if let Ok(mut journal) = journal.lock() {
                let frame = DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
                if let Err(error) = journal.record(frame, dt_us) {
                    log!("[DSframe-times] capture stopped: {}", error);
                }
            }
        }
        if (1_000..=5_000_000).contains(&dt_us) {
            history.intervals_us.push_back(dt_us as u32);
            if history.intervals_us.len() > 1024 {
                history.intervals_us.pop_front();
            }
        }
    }
}

/// Pack average FPS, 1% low FPS and minimum FPS into 16-bit tenths:
/// bits 0..15 average, 16..31 1% low, 32..47 minimum.
pub fn ds_present_stats_packed() -> u64 {
    let mut intervals: Vec<u32> = match DS_FRAME_HISTORY.get() {
        Some(history) => match history.lock() {
            Ok(history) => history.intervals_us.iter().copied().collect(),
            Err(_) => return 0,
        },
        None => return 0,
    };
    if intervals.len() < 2 {
        return 0;
    }
    intervals.sort_unstable();
    let total: u64 = intervals.iter().map(|value| *value as u64).sum();
    let average_us = (total / intervals.len() as u64).max(1);
    let worst_count = (intervals.len() / 100).max(1);
    let worst_total: u64 = intervals[intervals.len() - worst_count..]
        .iter()
        .map(|value| *value as u64)
        .sum();
    let one_percent_us = (worst_total / worst_count as u64).max(1);
    let minimum_us = (*intervals.last().unwrap_or(&1)).max(1) as u64;

    fn fps_tenths(frame_us: u64) -> u64 {
        ((1_000_000.0 / frame_us as f64) * 10.0)
            .round()
            .clamp(0.0, 65535.0) as u64
    }

    let average = fps_tenths(average_us);
    let one_percent = fps_tenths(one_percent_us);
    let minimum = fps_tenths(minimum_us);
    average | (one_percent << 16) | (minimum << 32)
}

/// Actual EAGL frame intervals for the in-game monitor, never compositor FPS.
pub(crate) fn ds_monitor_frame_stats() -> [f64; 6] {
    let Some(history)=DS_FRAME_HISTORY.get() else{return [0.0;6]};
    let Ok(history)=history.lock() else{return [0.0;6]};
    let mut times:Vec<u32>=history.intervals_us.iter().rev().take(240).copied().collect();
    if times.is_empty(){return [0.0;6]};
    let mean=times.iter().map(|v|*v as f64).sum::<f64>()/times.len() as f64;
    let hitches=times.iter().filter(|v|**v>50_000).count();
    times.sort_unstable();
    let percentile=|p:f64|times[((times.len() as f64*p).ceil() as usize).saturating_sub(1).min(times.len()-1)] as f64/1000.0;
    [1_000_000.0/mean,mean/1000.0,percentile(0.95),percentile(0.99),*times.last().unwrap() as f64/1000.0,hitches as f64]
}

/// Set by the E key; consumed at the next frame boundary in `gles_guest::glClear`,
/// where the colour buffer is readable. There the pale-cyan interaction ring
/// (door / console / kinesis prompt) is located and a tap is queued at it.
pub static DS_EKEY_INTERACT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Data-coordinate replacement for the E key's colour-blob ring scan.
/// `TOUCHHLE_DS_PROMPT_UV="tex,u0,u1,v0,v1"` makes the draw tracer record where
/// that prompt sprite was drawn (see `ds_set_prompt_xy`); the E key then taps
/// that point instead of guessing from pixels. The frame stamp lets the E key
/// reject a hit that is older than the sprite it is looking for.
pub static DS_PROMPT_XY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static DS_PROMPT_FRAME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Record the screen position (landscape iOS points) of the prompt sprite the
/// draw tracer just saw, together with the frame it was drawn on.
/// Two f32 bit patterns are packed into one AtomicU64 so the draw path stays
/// lock-free; `ds_prompt_xy()` unpacks them.
pub fn ds_set_prompt_xy(x: f32, y: f32) {
    use std::sync::atomic::Ordering;
    let packed = ((x.to_bits() as u64) << 32) | y.to_bits() as u64;
    DS_PROMPT_XY.store(packed, Ordering::Relaxed);
    DS_PROMPT_FRAME.store(DS_FRAME_COUNTER.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// Unpack `DS_PROMPT_XY`; `None` until something has been recorded.
pub fn ds_prompt_xy() -> Option<(f32, f32)> {
    use std::sync::atomic::Ordering;
    let p = DS_PROMPT_XY.load(Ordering::Relaxed);
    if p == 0 {
        return None;
    }
    Some((
        f32::from_bits((p >> 32) as u32),
        f32::from_bits((p & 0xffff_ffff) as u32),
    ))
}

/// Frame stamp of the most recent `ds_set_prompt_xy`.
pub fn ds_prompt_frame() -> u64 {
    DS_PROMPT_FRAME.load(std::sync::atomic::Ordering::Relaxed)
}

/// Set by the F key; kinesis (magnet icon) prompt is located and a tap queued.
pub static DS_FKEY_KINESIS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// RT launches a held telekinesis object without requiring LT first.
pub static DS_RT_KINESIS_LAUNCH_REQUEST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Left click launches a held telekinesis object before ordinary firing.
pub static DS_MOUSE_KINESIS_LAUNCH_REQUEST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Q/B on the standard death choice targets the cancel button.
pub static DS_DEATH_CANCEL_REQUEST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Set by the C key; stasis is a fixed on-screen button (aim mode).
pub static DS_CKEY_STASIS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Set by the **X** key; rotates the plasma cutter (action code `0x07`).
///
/// `0x07` was pinned to the engine's own sound `deadspace/plasma_cutter/rotate`
/// (DECOMPILE.md D-036), i.e. it is the aiming-stance "rotate the cutter 90°"
/// button, not a fire action. The engine gates it on aiming + idle + weapon
/// available, so pressing X when none of that holds does nothing.
pub static DS_XKEY_ROTATE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Set by the R key; consumed at the next frame boundary in `gles_guest::glClear`,
/// where the colour buffer is readable. There the cyan ammo/weapon box the game
/// draws next to the weapon (aim mode) is located and a tap is queued on it.
///
/// This cannot be a plain macro tap: the box **tracks the weapon pose**, so its
/// centre moves over ~120 landscape points between stances (measured
/// 2026-10-08, see TODO-SPECIAL-CONTROLS.md). Tapping a wrong point in aim mode
/// fires the weapon, so when no box is found we deliberately do nothing.
pub static DS_RKEY_RELOAD: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Gamepad RB: run the engine's stomp action directly.
pub(crate) static DS_PC_INPUT: std::sync::Mutex<ds_pc_input::Input> = std::sync::Mutex::new(ds_pc_input::Input::new());
pub static DS_AIM_REQUEST: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
pub static DS_PC_SKIN_HOST_REQUEST: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(-1);
pub static DS_PC_SKIN_INPUT_REQUEST: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(-1);
pub static DS_PC_TRANSFORM_HOST_REQUEST: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(-1);
pub static DS_FIRE_MODE_REQUEST: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
pub static DS_FIRE_RELEASE_REQUEST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub static DS_CONTEXT_COMBAT_REQUEST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Direct native player shortcuts, independent of experimental RIG/HUD routing.
/// Consumed once at a guest frame boundary; never synthesize double taps.
pub static DS_LOCATOR_REQUEST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static DS_QUICK_TURN_REQUEST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set by a diagnostic script: fire the *next* code from `TOUCHHLE_DS_SEND_ACTION`
/// (see `gles_guest::ds_scripted_actions`). Used to confirm the
/// code -> action table with exact timing from an input script, instead of
/// guessing the frame at which the level is up.
pub static DS_NEXT_ACTION: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Set by the N key: log the player's gauges (stasis / ammo / state, read
/// through the engine's own getters) and dump this frame. Used to measure what
/// one engine action did, without guessing from a screenshot.
pub static DS_PROBE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set by the M key (or F7); consumed by the frame dumper, which then writes a
/// full-resolution copy of that frame named `mark_<n>_f<frame>.ppm` and logs it.
///
/// Purpose: while playing, the user cannot see a shot number. Pressing a single
/// key at the interesting moment leaves a numbered, full-quality frame behind,
/// so afterwards it is enough to say "the 3rd mark".
pub static DS_MARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Sticky diagnostic arming flag. Normal play is unaffected; the pixel tracer
/// consults this only with TOUCHHLE_DS_PIXEL_TRACE_WAIT_FOR_MARK=1.
pub static DS_TRACE_MARKED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// SDL keycode for a name written in a macro file. Accepts a raw number, a
/// single alphanumeric character (SDL uses the lowercase ASCII code for those),
/// or a few common names.
fn macro_keycode_for(name: &str) -> Option<i64> {
    // A single alphanumeric character is the SDL keycode for that key; SDL
    // (like most APIs) uses the lowercase ASCII code. Check this *before* the
    // numeric parse so macro names like "1".."4" mean the number keys (49..52)
    // rather than raw keycodes 1..4.
    let mut chars = name.chars();
    if let Some(c) = chars.next() {
        if chars.next().is_none() && c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_lowercase() as i64);
        }
    }
    if let Ok(v) = name.parse::<i64>() {
        return Some(v);
    }
    match name {
        "Space" => return Some(32),
        "Return" | "Enter" => return Some(13),
        "Escape" | "Esc" => return Some(27),
        "Tab" => return Some(9),
        _ => {}
    }
    None
}

/// The raw SDL keycode as a number. rust-sdl2 in this tree prints `Keycode` as
/// a newtype (`Keycode(1073741893)`), not as a named variant, so the number has
/// to be recovered from the debug form.
fn keycode_number(k: sdl2::keyboard::Keycode) -> Option<i64> {
    let dbg = format!("{:?}", k);
    if let Some(rest) = dbg.strip_prefix("Keycode(") {
        return rest.trim_end_matches(')').parse::<i64>().ok();
    }
    None
}

/// Does the macro file's key name refer to `k`?
///
/// `ESC` is an alias for the macro key `p` (pause): the game's pause menu is a
/// touch UI, so "pause" is a macro that taps it, and ESC is the key people reach
/// for (user request, 2026-10-16).
fn macro_alias(k: sdl2::keyboard::Keycode) -> sdl2::keyboard::Keycode {
    if k == sdl2::keyboard::Keycode::Escape {
        sdl2::keyboard::Keycode::P
    } else {
        k
    }
}

fn macro_key_matches(name: &str, k: sdl2::keyboard::Keycode) -> bool {
    let k = macro_alias(k);
    if let (Some(want), Some(have)) = (macro_keycode_for(name), keycode_number(k)) {
        if want == have {
            return true;
        }
    }
    format!("{:?}", k) == name
}

/// PC-style aim point (`TOUCHHLE_DS_AIM_XY="x,y"`, landscape iOS points).
/// When set, the right mouse button holds a touch there (aim) and the left
/// button taps the same spot (fire); the legacy right-button tilt handling
/// is disabled so the two do not fight. Unset = original behaviour.
pub(crate) fn ds_native_inputs() -> bool { std::env::var("TOUCHHLE_DS_NATIVE_INPUTS").as_deref() == Ok("1") }
pub(crate) fn ds_native_pointer_enabled() -> bool {
    cfg!(target_os="windows") && ds_native_inputs()
        && std::env::var("TOUCHHLE_DS_SOFTWARE_CURSOR").as_deref() != Ok("0")
}
pub(crate) static DS_GAMEPLAY_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn ds_direction_key(key: sdl2::keyboard::Keycode) -> usize {
    use sdl2::keyboard::Keycode as K;
    match key { K::W=>0,K::A=>1,K::S=>2,K::D=>3,K::Up=>4,K::Left=>5,K::Down=>6,K::Right=>7,_=>unreachable!("non-direction key") }
}

fn ds_aim_toggle(device: &str) -> bool {
    std::env::var(format!("TOUCHHLE_DS_{device}_AIM_MODE")).as_deref() == Ok("toggle")
}

fn ds_take_aim_release(physical_down: &mut bool, logical_down: bool) -> bool {
    let release = *physical_down || logical_down;
    *physical_down = false;
    release
}

fn ds_release_key_enabled(key: &str) -> bool {
    std::env::var(format!("TOUCHHLE_DS_RELEASE_{key}")).as_deref() != Ok("0")
}

fn ds_aim_point() -> Option<(f32, f32)> {
    let s = std::env::var("TOUCHHLE_DS_AIM_XY").ok()?;
    let (a, b) = s.split_once(',')?;
    let ax = a.trim().parse::<f32>().ok()?;
    let ay = b.trim().parse::<f32>().ok()?;
    Some((ax, ay))
}

/// Set by the guest side every frame: `GameObjectPlayable+0x1e1 != 0`, i.e.
/// "the player is aiming right now" (`SetAiming`, 0x0003f884). Used by
/// `TOUCHHLE_DS_MOUSE_LOOK=aim` to decide when to capture the mouse.
pub static DS_AIM_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Physical LT state survives native control states so hold-to-aim can replay
/// its edge after stasis/kinesis releases the player.
pub static DS_CONTROLLER_LT_HELD: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Relative-mouse look mode (`TOUCHHLE_DS_MOUSE_LOOK`):
///
/// * unset / `0`  - off (the original absolute mouse mapping is untouched);
/// * `1` / `all`  - capture the mouse like an exclusive-fullscreen game: hide
///   the cursor, use SDL relative mode, and turn mouse motion into look drags;
/// * `aim`        - capture only while the guest reports the player aiming.
///
/// The delta is turned into the same drag the touch UI uses (a swipe in the
/// look area), so no game-side hook is needed - `TOUCHHLE_DS_LOOK_XY` picks the
/// anchor (default right-middle of the 480x320 landscape screen) and
/// `TOUCHHLE_DS_MOUSE_LOOK_GAIN` scales it (default 1.0).
#[derive(PartialEq, Clone, Copy)]
enum DsLookMode {
    Off,
    Always,
    WhileAiming,
}

fn ds_look_mode() -> DsLookMode {
    match std::env::var("TOUCHHLE_DS_MOUSE_LOOK") {
        Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
            "" | "0" | "off" | "false" => DsLookMode::Off,
            "aim" | "aiming" => DsLookMode::WhileAiming,
            _ => DsLookMode::Always,
        },
        Err(_) => DsLookMode::Off,
    }
}

#[derive(PartialEq, Clone, Copy)]
enum DsWindowMode {
    Windowed,
    Borderless,
    Exclusive,
}

fn ds_window_mode() -> DsWindowMode {
    match std::env::var("TOUCHHLE_DS_WINDOW_MODE")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "borderless" => DsWindowMode::Borderless,
        "exclusive" => DsWindowMode::Exclusive,
        _ => DsWindowMode::Windowed,
    }
}

fn ds_output_size() -> Option<(u32, u32)> {
    let width = std::env::var("TOUCHHLE_DS_OUTPUT_WIDTH")
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()?;
    let height = std::env::var("TOUCHHLE_DS_OUTPUT_HEIGHT")
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()?;
    if width == 0 || height == 0 {
        None
    } else {
        Some((width, height))
    }
}

fn ds_look_gain() -> f32 {
    std::env::var("TOUCHHLE_DS_MOUSE_LOOK_GAIN")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(1.0)
}

/// Mouse-look axis signs. The touch gesture used by Dead Space is opposite to
/// raw relative-mouse motion on both axes, so the safe game default is
/// `both`. Users can select `none`, `x` or `y` for other layouts.
fn ds_look_invert() -> (f32, f32) {
    let setting = std::env::var("TOUCHHLE_DS_MOUSE_LOOK_INVERT")
        .unwrap_or_else(|_| "both".to_string())
        .trim()
        .to_ascii_lowercase();
    match setting.as_str() {
        "0" | "off" | "none" | "false" => (1.0, 1.0),
        "x" | "horizontal" => (-1.0, 1.0),
        "y" | "vertical" => (1.0, -1.0),
        _ => (-1.0, -1.0),
    }
}

/// Is the **right mouse button** free to switch between look mode (captured) and
/// click mode (absolute pointer)?
///
/// Only when the look feature is on at all and PC-style aiming (`TOUCHHLE_DS_AIM_XY`)
/// is not already using the right button for the aim trigger. Users who want the
/// right button to stay the virtual accelerometer / aim trigger set
/// `TOUCHHLE_DS_MOUSE_LOOK_RMB=0`; F8 always works as the switch.
fn ds_look_capture_switch_armed() -> bool {
    if ds_look_mode() == DsLookMode::Off {
        return false;
    }
    if ds_aim_point().is_some() {
        return false;
    }
    !matches!(
        std::env::var("TOUCHHLE_DS_MOUSE_LOOK_RMB")
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref(),
        Ok("0") | Ok("off") | Ok("false") | Ok("")
    )
}

fn ds_look_anchor() -> (f32, f32) {
    std::env::var("TOUCHHLE_DS_LOOK_XY")
        .ok()
        .and_then(|s| {
            let (a, b) = s.split_once(',')?;
            Some((a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?))
        })
        .unwrap_or((240.0, 160.0))
}

/// `TOUCHHLE_DS_FPS_CAP=<fps>`: upper bound on the presented frame rate
/// (e.g. `120` on a 165 Hz display, `60` to keep the original feel, or unset for
/// "as fast as the machine can", which is what `TOUCHHLE_DS_VSYNC=0` is for).
/// Values below 5 or above 1000 are ignored so a typo cannot freeze the guest.
fn ds_fps_cap() -> Option<f32> {
    let v = std::env::var("TOUCHHLE_DS_FPS_CAP").ok()?;
    let cap: f32 = v.trim().parse().ok()?;
    if cap.is_finite() && (5.0..=1000.0).contains(&cap) {
        Some(cap)
    } else {
        None
    }
}

/// How many frames an injected tap stays pressed (`TOUCHHLE_DS_TAP_HOLD`,
/// default 1; 0 and 1 mean the same thing).
///
/// The VM input tool's click (`C:\dsh\input.ps1` -> `DoClick`) does
/// `SetCursorPos` / 130 ms / `LD` / **85 ms** / `LU`, i.e. the button is held
/// for several frames, and that is the *only* tap path ever measured to move
/// the game's state (DECOMPILE.md D-014/D-016/D-017). Every emulator-injected
/// tap so far went down and up **one frame apart** and did nothing - including
/// on the exact frame the game marked the object interactive and with the lift
/// moved to the press point (D-013, and the take-33 run recorded in D-018).
/// A hold is therefore the next thing to vary; keep the default at 1 frame so
/// nothing changes unless the switch is set.
fn ds_tap_hold() -> u64 {
    std::env::var("TOUCHHLE_DS_TAP_HOLD")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(1)
        .max(1)
}

pub struct Window {
    ds_held_touches: ds_held_inputs::HeldTouches<FingerId>,
    _sdl_ctx: sdl2::Sdl,
    video_ctx: sdl2::VideoSubsystem,
    window: sdl2::video::Window,
    event_pump: sdl2::EventPump,
    event_queue: VecDeque<Event>,
    last_polled: Instant,
    /// Separate queue for extremely high-priority events (e.g. app about to
    /// terminate).
    high_priority_event: Option<Event>,
    enable_event_polling: bool,
    backgrounded: bool,
    #[cfg(target_os = "macos")]
    max_height: u32,
    #[cfg(target_os = "macos")]
    viewport_y_offset: u32,
    /// Copy of `fullscreen` on [Options]. Note that this is meaningless when
    /// [Self::rotatable_fullscreen] returns [true].
    fullscreen: bool,
    /// Guest/EAGL renderbuffer scale (`--scale-hack`).
    scale_hack: NonZeroU32,
    /// Host window/backbuffer scale relative to the guest's logical screen.
    /// Unlike `scale_hack`, this does not enlarge the game's renderbuffer.
    /// Keeping them separate lets the present layer upscale a lower-resolution
    /// frame (FSR1 EASU/RCAS) without forcing the expensive guest renderer to
    /// allocate the output resolution.
    present_scale: NonZeroU32,
    /// Explicit host output size selected in the launcher. `None` keeps the
    /// legacy scale-derived window size.
    windowed_output_size: Option<(u32, u32)>,
    host_screen_size: Option<(u32, u32)>,
    internal_gl_ins: Option<Box<dyn GLESContext>>,
    pub(crate) internal_present_state: std::rc::Rc<std::cell::RefCell<crate::gles::present::PresentState>>,
    splash_image: Option<Image>,
    /// Whether the selected image already targets the startup orientation.
    splash_image_is_orientation_specific: bool,
    device_family: DeviceFamily,
    device_orientation: DeviceOrientation,
    controller_ctx: sdl2::GameControllerSubsystem,
    controllers: Vec<sdl2::controller::GameController>,
    dpad_state: DpadState,
    stick_active: bool,
    controller_lt_down: bool,
    controller_rt_down: bool,
    controller_rb_down: bool,
    controller_look_pending: bool,
    /// Edge-latch for the physical E interaction key. Some Windows/SDL paths
    /// still emit repeated KeyDown events without SDL's repeat flag; holding E
    /// must not reopen the same interaction dialog.
    ds_e_held: bool,
    ds_space_held: bool,
    ds_ui_key_held: std::collections::HashSet<sdl2::keyboard::Keycode>,
    ds_ui_capture_previous: bool,
    ds_test_pad: Option<ds_test_pad::TestPad>,
    /// Keyboard-driven virtual stick (WASD / arrow keys). Games that only
    /// understand a virtual analogue stick (Dead Space) are otherwise
    /// unplayable without a gamepad.
    key_stick: (f32, f32),
    key_directions: ds_pc_input::HeldDirections,
    pad_directions: ds_pc_input::HeldDirections,
    pad_weapons: ds_weapon_input::PadWeapons,
    /// Device-space position of the last key-stick touch, so direction changes
    /// and the release can be *slid* instead of teleporting (a teleport reads as
    /// a flick/turn to this game - see the key-stick arms below).
    key_stick_last: (f32, f32),
    _sensor_ctx: sdl2::SensorSubsystem,
    accelerometer: Option<sdl2::sensor::Sensor>,
    virtual_cursor_last: Option<(f32, f32, bool, bool)>,
    virtual_cursor_last_unsticky: Option<(f32, f32, Instant)>,
    /// Window position where the left mouse button went down, so the matching
    /// `TouchesUp` can be delivered at that spot instead of at the current
    /// cursor position. Dead Space's virtual stick is *origin relative*: a drag
    /// that ends outside the stick area (our walk gesture ends well above the
    /// rect) otherwise leaves the stick deflected and the character keeps
    /// walking after the player lets go (measured 2026-10-10: no input at all
    /// and the camera still advanced through the door, mean|diff| 57 over 10 s).
    mouse_left_button: ds_mouse_capture::LeftButton,
    ds_native_input_tick: Instant,
    ds_diag_native_stick: Option<(f32,f32)>,
    ds_diag_native_look: (f32,f32),
    virtual_accelerometer_last: Option<(f32, f32, bool)>,
    /// PC-style mouse aiming: true while the right button is held and
    /// TOUCHHLE_DS_AIM_XY is configured.
    ds_aim_active: bool,
    /// Physical edges survive temporary logical-input cancellation so their
    /// matching release can still revoke a native-held aim lease.
    ds_mouse_right_down: bool,
    controller_lt_physical_down: bool,
    /// Zero-g jump: wall-clock start of the synthetic shake. Real time is used
    /// rather than the presented-frame counter so the shake keeps its frequency
    /// even when the emulator runs at ~12 fps (a frame-counter-driven square
    /// wave at 12 fps cannot exceed ~2 Hz, and the guest samples the
    /// accelerometer far more often than that).
    ds_shake_start: Option<std::time::Instant>,
    /// Fixed parameters from the tested zero-g gesture. No runtime parameter
    /// probe is exposed.
    ds_shake_amp: f32,
    ds_shake_dur: f32,
    ds_shake_freq: f32,
    ds_shake_axis: u8,
    /// TAS-lite scripted input (`TOUCHHLE_DS_SCRIPT`). Each entry is
    /// (frame, rest-of-line), e.g. (120, "tap 240 270").
    /// Relative-mouse look (`TOUCHHLE_DS_MOUSE_LOOK`): motion deltas accumulate
    /// here and are flushed as ONE look-drag per frame, so several motion events
    /// inside one frame cannot queue up a backlog of swipes.
    ds_look_accum: (f32, f32),
    /// Whether the relative-look gesture currently has a held guest finger.
    ds_look_dragging: bool,
    /// Last guest-space position of that held look finger.
    ds_look_position: (f32, f32),
    /// Relative capture, persistent click mode, and focus-recapture policy.
    ds_mouse_capture: ds_mouse_capture::CaptureState,
    ds_mouse_motion_gate: ds_mouse_capture::MotionGate,
    ds_right_press_gate: ds_mouse_capture::RightPressGate,
    /// Temporary capture suspension after Win/focus loss. Unlike click mode,
    /// this ends automatically when the game window regains focus.
    ds_capture_suspended: bool,
    /// Holding Alt temporarily exposes the absolute pointer. Releasing Alt
    /// restores capture without changing the persistent click-mode toggle.
    ds_alt_released: bool,
    /// Gamepad cursor mode, toggled by Start. Right stick moves the
    /// rendered cursor, A clicks and B exits without changing camera input in
    /// normal gameplay.
    ds_gamepad_cursor_mode: bool,
    ds_gamepad_start_down: Option<Instant>,
    ds_gamepad_start_long: bool,
    /// A pause-resume press must consume its matching release. Otherwise a
    /// Start press can resume the native pause screen on ButtonDown and then
    /// queue a second pause when ButtonUp arrives after the HUD has returned
    /// to gameplay.
    ds_gamepad_resume_latched: Option<sdl2::controller::Button>,
    ds_script: Vec<(u64, String)>,
    ds_script_loaded: bool,
    ds_control_inbox: ds_control_inbox::Inbox,
    ds_control_ready_logged: bool,
    /// Key macros (`TOUCHHLE_DS_MACROS`): key name -> action text. A key press
    /// schedules the action at the current frame, so a single key can perform a
    /// QTE swipe, a rapid-tap burst, or a single tap.
    ds_macros: Vec<(String, String)>,
    ds_macros_loaded: bool,
    /// TAS-lite recorder sink (`TOUCHHLE_DS_RECORD`).
    ds_record_file: Option<std::fs::File>,
    /// F8 toggles this; while false, real touches are not recorded.
    ds_record_enabled: bool,
    /// Whether or not we are on the "main" environment stack (rather than
    /// a coroutine stack). Checked in various functions to make sure that
    /// certain SDL functions (that call JNI functions) are on the main
    /// stack on Android.
    pub(super) on_main_stack: bool,
}

// Shared contract with translation branch a2af13e: hidden at creation only
// for an explicit private control-inbox test; ordinary launches are unchanged.
fn ds_background_test() -> bool {
    static ENABLED:std::sync::OnceLock<bool>=std::sync::OnceLock::new();
    *ENABLED.get_or_init(||cfg!(target_os="windows")
        && std::env::var("TOUCHHLE_DS_BACKGROUND_TEST").as_deref()==Ok("1")
        && std::env::var_os("TOUCHHLE_DS_CONTROL_INBOX").is_some())
}

impl Window {
    fn ds_poll_control_inbox(&mut self) {
        use ds_control_inbox::Command;
        use std::sync::atomic::Ordering::Relaxed;
        let result=self.ds_control_inbox.poll();
        if self.ds_control_inbox.configured() && !self.ds_control_ready_logged {
            self.ds_control_ready_logged=true;
            echo!("[DScontrol] ready; existing command ignored; publish a new sequence");
        }
        match result {
            Ok(Some((sequence,command))) => {
                echo!("[DScontrol] sequence={} queued {:?}",sequence,command);
                match command {
                    Command::Touch(action)=>self.ds_queue_action(action),
                    Command::Action(action)=>match action.as_str() {
                        "interact"=>DS_EKEY_INTERACT.store(true,Relaxed),
                        "kinesis"=>DS_FKEY_KINESIS.store(true,Relaxed),
                        "stasis"=>DS_CKEY_STASIS.store(true,Relaxed),
                        "reload"=>DS_RKEY_RELOAD.store(true,Relaxed),
                        "rotate"=>DS_XKEY_ROTATE.store(true,Relaxed),
                        "locator"=>DS_LOCATOR_REQUEST.store(true,Relaxed),
                        "quick-turn"=>DS_QUICK_TURN_REQUEST.store(true,Relaxed),
                        "combat"=>DS_CONTEXT_COMBAT_REQUEST.store(true,Relaxed),
                        _=>unreachable!(),
                    },
                    Command::Hud(key)=>{
                        if key == "G" { DS_LOCATOR_REQUEST.store(true,Relaxed); }
                        else if ds_native_hud_enabled() {
                            if let Some(command)=ds_hud_commands::HudCommand::from_key(&key) { ds_queue_hud(command); }
                        } else { echo!("[DScontrol] sequence={} rejected: native HUD is disabled",sequence); }
                    },
                    Command::Aim(on)=>DS_AIM_REQUEST.store(on as i32,Relaxed),
                    Command::AimToggle=>DS_AIM_REQUEST.store(2,Relaxed),
                    Command::NativeStick(x,y)=>self.ds_diag_native_stick=Some((x,y)),
                    Command::NativeLook(x,y)=>{self.ds_diag_native_look.0+=x;self.ds_diag_native_look.1+=y;},
                    Command::Ui(action)=>{
                        use sdl2::keyboard::{Keycode as K,Mod,Scancode};
                        let key=match action.as_str(){
                            "up"=>K::Up,"down"=>K::Down,"left"=>K::Left,"right"=>K::Right,
                            "confirm"=>K::E,"cancel"=>K::X,"delete"=>K::Delete,_=>return,
                        };
                        let (timestamp,window_id,keycode,scancode,keymod,repeat)=(0,self.window.id(),Some(key),Scancode::from_keycode(key),Mod::NOMOD,false);
                        for event in [
                            sdl2::event::Event::KeyDown{timestamp,window_id,keycode,scancode,keymod,repeat},
                            sdl2::event::Event::KeyUp{timestamp,window_id,keycode,scancode,keymod,repeat},
                        ] {
                            if let Err(error)=self._sdl_ctx.event().and_then(|events|events.push_event(event)){echo!("[DScontrol] ui enqueue failed: {}",error);}
                        }
                    },
                    Command::DirectionKey {slot,down}=>{
                        // Queue only into this emulator's SDL event queue. This
                        // is not host keyboard injection and is opt-in via inbox.
                        use sdl2::keyboard::{Keycode as K,Mod,Scancode};
                        let key=[K::W,K::A,K::S,K::D,K::Up,K::Left,K::Down,K::Right,K::Space,K::E,K::G,K::B,K::LCtrl,K::RCtrl,K::Z][slot];
                        let (timestamp,window_id,keycode,scancode,keymod,repeat)=(0,self.window.id(),Some(key),Scancode::from_keycode(key),Mod::NOMOD,false);
                        let event=if down {sdl2::event::Event::KeyDown {timestamp,window_id,keycode,scancode,keymod,repeat}}
                            else {sdl2::event::Event::KeyUp {timestamp,window_id,keycode,scancode,keymod,repeat}};
                        if slot<8 {self.ds_diag_native_stick=None;}
                        if let Err(error)=self._sdl_ctx.event().and_then(|events|events.push_event(event)) {echo!("[DScontrol] direction enqueue failed: {}",error);}
                    },
                    Command::Fire(mode)=>ds_fire_control::enqueue(ds_fire_control::Input::Fire(ds_fire_control::Source::Diagnostic,match mode {1=>Some(false),2=>Some(true),_=>None})),
                    Command::Release=>ds_fire_control::enqueue(ds_fire_control::Input::Release(ds_fire_control::Source::Diagnostic)),
                    Command::TestPadButton(name,down)=>self.ds_test_pad_input(&name,Some(down),None),
                    Command::TestPadAxis(name,value)=>self.ds_test_pad_input(&name,None,Some(value)),
                    Command::Cancel=>self.ds_cancel_inputs(),
                    Command::Mark=>DS_MARK.store(true,Relaxed),
                    Command::ReportProblem=>crate::ds_diagnostics::mark_problem(),
                    Command::Probe=>{DS_PROBE.store(true,Relaxed);DS_MARK.store(true,Relaxed);},
                    Command::TestAssist(enabled)=>{
                        let accepted=crate::frameworks::opengles::ds_test_assist::request(enabled);
                        echo!("[DScontrol] test-assist enabled={} accepted={} lease_ms=5000",enabled,accepted);
                    },
                    Command::Checkpoint=>echo!("[DSverify] control checkpoint sequence={} frame={}",sequence,DS_FRAME_COUNTER.load(Relaxed)),
                    Command::PerfGpuCopy(enabled)=>crate::ds_perf_control::set_gpu_present_copy(enabled),
                    Command::PerfTextOverlay(enabled)=>crate::ds_perf_control::set_gpu_text_overlay(enabled),
                    Command::PerfReadbackFinish(enabled)=>crate::ds_perf_control::set_readback_finish(enabled),
                    Command::PerfFpsLimit(fps)=>crate::ds_perf_control::set_fps_limit(fps),
                    Command::PerfTextureSync(mode)=>{
                        let mode=match mode.as_str(){
                            "flush"=>crate::ds_perf_control::TextureSyncMode::Flush,
                            "off"=>crate::ds_perf_control::TextureSyncMode::Off,
                            _=>crate::ds_perf_control::TextureSyncMode::Finish,
                        };
                        crate::ds_perf_control::set_texture_upload_sync(mode);
                    },
                    Command::PerfSkinHost(enabled)=>DS_PC_SKIN_HOST_REQUEST.store(enabled as i32,Relaxed),
                    Command::PerfSkinInput(strided)=>DS_PC_SKIN_INPUT_REQUEST.store(strided as i32,Relaxed),
                    Command::PerfTransformHost(enabled)=>DS_PC_TRANSFORM_HOST_REQUEST.store(enabled as i32,Relaxed),
                    Command::PerfMatrixFormat(eager)=>{
                        crate::gles::gles1_on_gl2::set_matrix_trace_format_eager(eager);
                        echo!("[DSmatrix-format] private runtime mode={}",if eager {"eager"} else {"lazy"});
                    },
                }
            },
            Ok(None)=>{},
            Err(error)=>echo!("[DScontrol] rejected: {}",error),
        }
    }
    /// Returns [true] if touchHLE is running on a device where we should always
    /// display fullscreen, but SDL2 will let us control the orientation, i.e.
    /// Android devices.
    pub fn rotatable_fullscreen() -> bool {
        env::consts::OS == "android"
    }

    /// Confirm normal desktop window close. Never intercept OS force termination.
    fn ds_confirm_quit(&mut self) -> bool {
        if env::consts::OS == "android" || std::env::var("TOUCHHLE_DS_CONFIRM_QUIT").as_deref() == Ok("0") { return true; }
        use sdl2::messagebox;
        let recapture = self.ds_mouse_capture.gameplay_pointer(self.ds_capture_suspended, self.ds_alt_released, self.ds_gamepad_cursor_mode);
        self.ds_cancel_inputs();
        self.ds_set_mouse_capture(false, "quit confirmation");
        let fullscreen = self.window.fullscreen_state();
        if fullscreen == sdl2::video::FullscreenType::True {
            // Exclusive scanout can cover even an owned native dialog. Keep
            // the same fullscreen desktop while the confirmation is open.
            if let Err(error) = self.window.set_fullscreen(sdl2::video::FullscreenType::Desktop) {
                echo!("[DSquit] cannot release exclusive fullscreen: {error}");
                if recapture { self.ds_set_mouse_capture(true, "quit retained"); }
                return false;
            }
            // Let the compositor finish leaving exclusive scanout before the
            // owned WinForms confirmation is created.
            std::thread::sleep(Duration::from_millis(180));
            self.event_pump.pump_events();
        }
        self.window.raise();
        let launcher = std::env::var_os("TOUCHHLE_DS_LAUNCHER_PATH").map(std::path::PathBuf::from)
            .or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.join("DeadSpace Launcher.exe"))));
        let styled_result = launcher.filter(|p| p.is_file() && p.with_file_name("DeadSpace-quit-dialog-v1.txt").is_file()).and_then(|path| {
            match std::process::Command::new(path).arg("--confirm-quit").arg(std::process::id().to_string()).spawn() {
                Ok(mut child) => {
                    allow_foreground_process(child.id());
                    loop {
                    // ShowDialog disables its cross-process owner with native
                    // window messages. A blocking child wait prevents SDL's
                    // window thread from servicing them, stranding the modal.
                    // Pump only the host here; game input stays suspended.
                    self.event_pump.pump_events();
                    while self.event_pump.poll_event().is_some() {}
                    match child.try_wait() {
                        Ok(Some(status)) => break Some(status.code() == Some(10)),
                        Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                        Err(error) => { echo!("[DSquit] themed dialog wait failed; game retained: {error}"); break Some(false); }
                    }
                }
                }
                ,
                Err(error) => { echo!("[DSquit] themed dialog unavailable: {error}"); None }
            }
        });
        let buttons = [
            messagebox::ButtonData { flags: messagebox::MessageBoxButtonFlag::ESCAPEKEY_DEFAULT | messagebox::MessageBoxButtonFlag::RETURNKEY_DEFAULT, button_id: 0, text: "取消" },
            messagebox::ButtonData { flags: messagebox::MessageBoxButtonFlag::NOTHING, button_id: 1, text: "退出游戏" },
        ];
        echo!("[DSquit] confirmation opened; input released");
        let confirmed = styled_result.unwrap_or_else(|| match messagebox::show_message_box(messagebox::MessageBoxFlag::WARNING, &buttons,
            "退出游戏？", "将关闭当前游戏进程。\n进度以最近一次游戏内自动保存为准。\n是否退出？", Some(&self.window), None) {
            Ok(messagebox::ClickedButton::CustomButton(button)) => button.button_id == 1,
            Ok(messagebox::ClickedButton::CloseButton) => false,
            Err(error) => { echo!("[DSquit] confirmation failed; game retained: {error}"); false }
        });
        if fullscreen == sdl2::video::FullscreenType::True {
            if let Err(error) = self.window.set_fullscreen(fullscreen) {
                echo!("[DSquit] exclusive fullscreen restore failed; desktop fullscreen retained: {error}");
            }
        }
        if confirmed { echo!("[DSquit] confirmed; normal guest termination queued"); }
        else {
            echo!("[DSquit] cancelled; game retained");
            if recapture { self.ds_set_mouse_capture(true, "quit cancelled"); }
        }
        confirmed
    }

    fn ds_sample_native_inputs(&mut self, options: &Options, focused: bool) {
        let now=Instant::now();let dt=now.saturating_duration_since(self.ds_native_input_tick).as_secs_f32().min(0.05);self.ds_native_input_tick=now;
        let background=ds_background_test();
        let blocked=(!background && (!focused || self.ds_capture_suspended || self.ds_mouse_capture.forced_off || self.ds_alt_released || self.ds_gamepad_cursor_mode))
            || crate::ds_frontend_controller::capture_menu() || crate::ds_frontend_controller::capture_scene_dialog();
        let mut x=0.0;let mut y=0.0;let mut pad_x=0.0;let mut pad_y=0.0;
        if !blocked {
            let gain: f32=std::env::var("TOUCHHLE_DS_KEY_STICK_GAIN").ok().and_then(|v|v.parse().ok()).unwrap_or(2.0);
            if self.key_stick != (0.0,0.0) {x=self.key_stick.0*gain;y=self.key_stick.1*gain;}
            else if !background && ds_input_mode()!=ds_gamepad_bindings::InputMode::Keyboard {
                let (sx,sy,_)=self.get_controller_stick(options,true);x=sx;y=sy;
                if self.dpad_state.left || self.dpad_state.right || self.dpad_state.up || self.dpad_state.down {
                    (x,y)=self.pad_directions.vector();
                }
            }
            if !background && ds_input_mode()!=ds_gamepad_bindings::InputMode::Keyboard {
                use sdl2::controller::Axis;
                let deadzone:f32=std::env::var("TOUCHHLE_DS_CONTROLLER_LOOK_DEADZONE").ok().and_then(|v|v.parse().ok()).unwrap_or(0.14);
                let gain:f32=std::env::var("TOUCHHLE_DS_CONTROLLER_LOOK_GAIN").ok().and_then(|v|v.parse().ok()).unwrap_or(18.0);
                let inv=std::env::var("TOUCHHLE_DS_CONTROLLER_LOOK_INVERT").unwrap_or_default();
                for controller in &self.controllers {
                    let axis=|value:i16| { let v=(value as f32/32768.0).clamp(-1.0,1.0);if v.abs()<=deadzone {0.0}else{((v.abs()-deadzone)/(1.0-deadzone).max(0.01)).copysign(v)} };
                    pad_x+=axis(controller.axis(Axis::RightX))*gain*30.0*dt*if inv=="x"||inv=="both"{-1.0}else{1.0};
                    pad_y+=axis(controller.axis(Axis::RightY))*gain*30.0*dt*if inv=="y"||inv=="both"{-1.0}else{1.0};
                }
            }
        }
        // Explicit inbox vectors must work in hidden/VM verification just like
        // inbox taps/actions. Host keys/mouse remain focus-gated. Focus loss
        // and UI ownership still cancel everything; only NEW inbox commands
        // can start background movement, and cannot operate behind a modal.
        let diagnostic=self.ds_control_inbox.configured()
            && !crate::ds_frontend_controller::capture_menu()
            && !crate::ds_frontend_controller::capture_scene_dialog();
        if diagnostic {if let Some(raw)=self.ds_diag_native_stick {x=raw.0;y=raw.1;}}
        let mut pending=DS_PC_INPUT.lock().unwrap();pending.x=x;pending.y=y;
        if !blocked {pending.look_x+=self.ds_look_accum.0+pad_x;pending.look_y+=self.ds_look_accum.1+pad_y;}
        else {pending.look_x=0.0;pending.look_y=0.0;}
        if diagnostic {pending.look_x+=self.ds_diag_native_look.0;pending.look_y+=self.ds_diag_native_look.1;}
        self.ds_diag_native_look=(0.0,0.0);
        self.ds_look_accum=(0.0,0.0);self.controller_look_pending=false;
    }

    fn ds_start_zero_g_jump(&mut self, source: &str) {
        self.ds_shake_start = Some(std::time::Instant::now());
        echo!(
            "[DSjump] {source}: tested gesture amp={}g dur={}s freq={}Hz axis={}",
            self.ds_shake_amp,
            self.ds_shake_dur,
            self.ds_shake_freq,
            ["faithful", "x", "y", "z", "all", "tumble"]
                [(self.ds_shake_axis % 6) as usize]
        );
    }

    fn ds_controller_lt_pressed(&self) -> bool {
        self.controller_lt_down
            || self
                .controllers
                .iter()
                .any(|controller| controller.axis(sdl2::controller::Axis::TriggerLeft) > 16000)
    }

    /// Refresh the physical LT level once per input turn. Native control/QTE
    /// can consume the SDL axis edge, leaving the old logical aim latch set;
    /// sampling the controller itself makes release-after-control authoritative
    /// instead of restoring aim merely because it was active before the grab.
    fn ds_refresh_controller_lt_state(&mut self) {
        let physical = self
            .controllers
            .iter()
            .any(|controller| controller.axis(sdl2::controller::Axis::TriggerLeft) > 16000);
        if physical {
            self.controller_lt_physical_down = true;
            DS_CONTROLLER_LT_HELD.store(true, std::sync::atomic::Ordering::Relaxed);
            return;
        }
        if !self.controller_lt_physical_down && !self.controller_lt_down {
            return;
        }
        let was_logical = self.controller_lt_down;
        self.controller_lt_physical_down = false;
        self.controller_lt_down = false;
        self.ds_aim_active = false;
        DS_CONTROLLER_LT_HELD.store(false, std::sync::atomic::Ordering::Relaxed);
        if was_logical && !ds_aim_toggle("CONTROLLER") {
            DS_AIM_REQUEST.store(0, std::sync::atomic::Ordering::Relaxed);
        }
        if self.controller_rt_down || self.controller_rb_down {
            DS_FIRE_RELEASE_REQUEST.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        echo!("[DSpad] LT physical release sampled -> aim cleared");
    }

    fn ds_set_gamepad_cursor_mode(&mut self, enabled: bool) {
        if self.ds_gamepad_cursor_mode == enabled {
            return;
        }
        self.ds_gamepad_cursor_mode = enabled;
        if enabled {
            self.ds_cancel_inputs();
            if self.controller_lt_down {
                crate::window::DS_AIM_REQUEST
                    .store(0, std::sync::atomic::Ordering::Relaxed);
            }
            if self.controller_rt_down || self.controller_rb_down {
                crate::window::DS_FIRE_RELEASE_REQUEST
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.controller_lt_down = false;
            DS_CONTROLLER_LT_HELD.store(false, std::sync::atomic::Ordering::Relaxed);
            self.controller_rt_down = false;
            self.controller_rb_down = false;
            self.ds_aim_active = false;
            self.ds_mouse_right_down = false;
            self.controller_lt_physical_down = false;
        }
        self.ds_look_accum = (0.0, 0.0);
        self.controller_look_pending = false;
        echo!(
            "[DSpad] Start long-press -> gamepad cursor {}",
            if enabled {
                "ON (right stick moves, A clicks, B exits)"
            } else {
                "OFF (right stick returns to camera)"
            }
        );
    }

    fn ds_gamepad_start_tick(&mut self) {
        let Some(started) = self.ds_gamepad_start_down else {
            return;
        };
        if !self.ds_gamepad_start_long && started.elapsed() >= DS_GAMEPAD_START_HOLD {
            self.ds_gamepad_start_long = true;
            let enabled = !self.ds_gamepad_cursor_mode;
            self.ds_set_gamepad_cursor_mode(enabled);
        }
    }

    fn ds_cursor_mode_active(&self) -> bool {
        if crate::ds_frontend_controller::capture_menu() && !self.controllers.is_empty()
            && ds_input_mode() != ds_gamepad_bindings::InputMode::Keyboard { return true; }
        ds_mouse_capture::gamepad_cursor_active(
            ds_native_inputs(),
            !self.controllers.is_empty(),
            ds_input_mode() != ds_gamepad_bindings::InputMode::Keyboard,
            self.ds_gamepad_cursor_mode,
            self.ds_mouse_capture.forced_off
                || self.ds_alt_released
                || self.ds_capture_suspended
                || ds_look_mode() == DsLookMode::Off,
        )
    }

    /// Apply mouse capture and keep emulator state consistent with SDL's
    /// result. On a rejected capture, fall back to click mode so the pointer
    /// remains usable instead of trapping the user behind a hidden cursor.
    fn ds_set_mouse_capture(&mut self, want: bool, context: &str) -> bool {
        struct SdlCapture;
        impl ds_mouse_capture::CaptureBackend for SdlCapture {
            fn set_relative(&mut self, enabled: bool) -> Result<(), String> {
                let result = unsafe {
                    sdl2_sys::SDL_SetRelativeMouseMode(if enabled {
                        sdl2_sys::SDL_bool::SDL_TRUE
                    } else {
                        sdl2_sys::SDL_bool::SDL_FALSE
                    })
                };
                if result == 0 { return Ok(()); }
                let error = unsafe { sdl2_sys::SDL_GetError() };
                Err(if error.is_null() { "unknown SDL error".into() } else {
                    unsafe { std::ffi::CStr::from_ptr(error) }.to_string_lossy().into_owned()
                })
            }
            fn show_cursor(&mut self, visible: bool) {
                unsafe { sdl2_sys::SDL_ShowCursor(if visible { 1 } else { 0 }); }
            }
        }
        // A capture transition can warp the OS cursor. Discard its motion and
        // the old look delta without discarding button/key release events.
        self.ds_look_accum = (0.0, 0.0);
        {
            let mut pending = DS_PC_INPUT.lock().unwrap();
            pending.look_x = 0.0;
            pending.look_y = 0.0;
        }
        let result = self.ds_mouse_capture.set(want, &mut SdlCapture);
        unsafe {
            sdl2_sys::SDL_PumpEvents();
            sdl2_sys::SDL_FlushEvent(sdl2_sys::SDL_EventType::SDL_MOUSEMOTION as u32);
            sdl2_sys::SDL_GetRelativeMouseState(std::ptr::null_mut(), std::ptr::null_mut());
            self.ds_mouse_motion_gate.reset(sdl2_sys::SDL_GetTicks());
        }
        match result {
            Ok(()) => true,
            Err(message) => {
                echo!("[DScapture] {}: SDL failed to {} mouse capture: {}; click mode retained",
                    context, if want { "enable" } else { "release" }, message);
                false
            }
        }
    }

    fn toggle_fullscreen(&mut self) {
        if Self::rotatable_fullscreen() {
            return;
        }

        let new_fullscreen = !self.fullscreen;
        let mode = if new_fullscreen {
            match ds_window_mode() {
                DsWindowMode::Exclusive => sdl2::video::FullscreenType::True,
                _ => sdl2::video::FullscreenType::Desktop,
            }
        } else {
            sdl2::video::FullscreenType::Off
        };

        match self.window.set_fullscreen(mode) {
            Ok(()) => {
                self.fullscreen = new_fullscreen;
                log!(
                    "Switched to {} mode.",
                    if self.fullscreen {
                        "fullscreen"
                    } else {
                        "windowed"
                    }
                );
            }
            Err(error) => {
                log!("Could not toggle fullscreen mode: {error}");
            }
        }
    }

    pub fn new(
        title: &str,
        icon: Option<Image>,
        launch_image: Option<(Image, bool)>,
        options: &Options,
    ) -> Window {
        let sdl_ctx = sdl2::init().unwrap();
        let video_ctx = sdl_ctx.video().unwrap();

        // The "hidapi" feature of rust-sdl2 is enabled so that sdl2::sensor
        // is available, but we don't want to enable SDL's HIDAPI controller
        // drivers because they cause duplicated controllers on macOS
        // (https://github.com/libsdl-org/SDL/issues/7479). Once that's fixed,
        // remove this (https://github.com/touchHLE/touchHLE/issues/85).
        // Keep the duplicate-device workaround scoped to macOS. Windows needs
        // SDL's HIDAPI drivers for PlayStation and Switch controllers.
        #[cfg(target_os = "macos")]
        sdl2::hint::set("SDL_JOYSTICK_HIDAPI", "0");

        if env::consts::OS == "android" {
            // It's important to set context version BEFORE window creation
            // ref. https://wiki.libsdl.org/SDL2/SDL_GLattr
            let attr = video_ctx.gl_attr();
            attr.set_context_version(1, 1);
            attr.set_context_profile(sdl2::video::GLProfile::GLES);

            // Disable blocking of event loop when app is paused.
            sdl2::hint::set("SDL_ANDROID_BLOCK_ON_PAUSE", "0");
        }

        // Separate mouse and touch events in both SDL synthesis directions.
        // SDL_TOUCH_MOUSE_EVENTS=0 stops touch input from also generating mouse input.
        // SDL_MOUSE_TOUCH_EVENTS=0 stops mouse input from also generating touch input.
        // Without both, one physical press can become two UIKit touches on some hosts.
        sdl2::hint::set("SDL_TOUCH_MOUSE_EVENTS", "0");
        sdl2::hint::set("SDL_MOUSE_TOUCH_EVENTS", "0");

        // SDL2 disables the screen saver by default, but iPhone OS enables
        // the idle timer that triggers sleep by default, so we turn it back on
        // here, and then the app can disable it if it wants to.
        video_ctx.enable_screen_saver();

        let scale_hack = options.scale_hack;
        let present_scale = std::env::var("TOUCHHLE_DS_PRESENT_SCALE")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .and_then(NonZeroU32::new)
            .unwrap_or(scale_hack);
        let host_screen_size = options.host_screen_size.map(normalize_portrait_size);
        // TODO: some apps specify their orientation in Info.plist, we could use
        // that here.
        let device_family = options.device_family.unwrap_or(DeviceFamily::iPhone);
        let device_orientation = options.initial_orientation;
        let output_size = ds_output_size();
        let requested_window_mode = match ds_window_mode() {
            DsWindowMode::Windowed if options.fullscreen => DsWindowMode::Borderless,
            mode => mode,
        };
        let fullscreen = requested_window_mode != DsWindowMode::Windowed;
        let portrait_screen_size =
            host_screen_size.unwrap_or_else(|| device_family.portrait_size());
        log!(
            "[DSwindow] mode={} output={} scale_hack={} present_scale={}",
            match requested_window_mode {
                DsWindowMode::Windowed => "windowed",
                DsWindowMode::Borderless => "borderless-fullscreen",
                DsWindowMode::Exclusive => "exclusive-fullscreen",
            },
            output_size
                .map(|(w, h)| format!("{w}x{h}"))
                .unwrap_or_else(|| "follow-internal".to_string()),
            scale_hack,
            present_scale
        );

        // iPhone OS gives apps a 24-bit depth buffer (PowerVR SGX advertises
        // GL_OES_depth24) plus 8 bits of stencil, and Dead Space's renderer
        // relies on that precision: poster decals and the layered sky quads sit
        // at nearly identical depths. SDL2's default for SDL_GL_DEPTH_SIZE is
        // only **16** bits, which turns exactly those surfaces into z-fighting -
        // the "textures fight over priority" / flicker reported against the sky
        // and wall posters. These attributes must be set before the GL context
        // is created, hence the position here rather than next to the other
        // per-platform settings.
        //
        // `TOUCHHLE_DS_DEPTH_SIZE=<bits>` overrides it, which is how the 16 vs
        // 24 comparison above was measured without two builds.
        {
            let attr = video_ctx.gl_attr();
            let depth_bits: u8 = std::env::var("TOUCHHLE_DS_DEPTH_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(24);
            attr.set_depth_size(depth_bits);
            attr.set_stencil_size(8);
            log!(
                "[DSgl] requesting SDL_GL_DEPTH_SIZE={} STENCIL_SIZE=8",
                depth_bits
            );
        }

        let mut window = if ds_background_test() {
            let (width,height)=output_size.unwrap_or((960,640));
            // Create hidden from the outset: neither show nor acquire focus.
            video_ctx.window(title,width,height).hidden().opengl().build().unwrap()
        } else if Self::rotatable_fullscreen() {
            // Without this, SDL will force fullscreen mode to be portrait.
            set_sdl2_orientation(device_orientation);
            let screen_size = video_ctx.display_bounds(0).unwrap().size();
            let (width, height) = rotate_fullscreen_size(device_orientation, screen_size);
            let window = video_ctx
                .window(title, width, height)
                .fullscreen()
                .opengl()
                .build()
                .unwrap();
            window
        } else if requested_window_mode == DsWindowMode::Borderless {
            let (width, height) = video_ctx.display_bounds(0).unwrap().size();
            let window = video_ctx
                .window(title, width, height)
                .fullscreen_desktop()
                .opengl()
                .build()
                .unwrap();
            window
        } else if requested_window_mode == DsWindowMode::Exclusive {
            let (width, height) =
                output_size.unwrap_or_else(|| video_ctx.display_bounds(0).unwrap().size());
            let window = video_ctx
                .window(title, width, height)
                .fullscreen()
                .opengl()
                .build()
                .unwrap();
            window
        } else {
            let (width, height) = output_size.unwrap_or_else(|| {
                size_for_orientation_from_size(
                    portrait_screen_size,
                    device_orientation,
                    present_scale,
                )
            });
            let window = video_ctx
                .window(title, width, height)
                .position_centered()
                .opengl()
                .build()
                .unwrap();
            window
        };

        if env::consts::OS == "android" {
            // Sanity check
            let gl_attr = video_ctx.gl_attr();
            debug_assert_eq!(gl_attr.context_profile(), sdl2::video::GLProfile::GLES);
            debug_assert_eq!(gl_attr.context_version(), (1, 1));
        }

        if let Some(icon) = icon {
            window.set_icon(surface_from_image(&icon));
        }

        let event_pump = sdl_ctx.event_pump().unwrap();

        let controller_ctx = sdl_ctx.game_controller().unwrap();
        log!("Controller discovery: HIDAPI={} (SDL default=1)",
            sdl2::hint::get("SDL_JOYSTICK_HIDAPI").unwrap_or_else(|| "default".into()));

        let sensor_ctx = sdl_ctx.sensor().unwrap();
        let mut accelerometer: Option<sdl2::sensor::Sensor> = None;
        if let Ok(num_sensors) = sensor_ctx.num_sensors() {
            for sensor_idx in 0..num_sensors {
                if let Ok(sensor) = sensor_ctx.open(sensor_idx) {
                    if sensor.sensor_type() == sdl2::sensor::SensorType::Accelerometer {
                        log!("Accelerometer detected: {}.", sensor.name());
                        accelerometer = Some(sensor);
                        break;
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        let max_height = window.size().1;

        let (splash_image, splash_image_is_orientation_specific) = launch_image
            .map(|(image, orientation_specific)| (Some(image), orientation_specific))
            .unwrap_or((None, false));

        let mut window = Window {
            _sdl_ctx: sdl_ctx,
            video_ctx,
            window,
            event_pump,
            event_queue: VecDeque::new(),
            last_polled: Instant::now() - Duration::from_secs(1),
            high_priority_event: None,
            enable_event_polling: true,
            backgrounded: false,
            #[cfg(target_os = "macos")]
            max_height,
            #[cfg(target_os = "macos")]
            viewport_y_offset: 0,
            fullscreen,
            scale_hack,
            present_scale,
            windowed_output_size: output_size,
            host_screen_size,
            internal_gl_ins: None,
            internal_present_state: Default::default(),
            splash_image,
            splash_image_is_orientation_specific,
            device_family,
            device_orientation,
            controller_ctx,
            controllers: Vec::new(),
            ds_held_touches: ds_held_inputs::HeldTouches::new(),
            dpad_state: DpadState {
                left: false,
                right: false,
                up: false,
                down: false,
                active: false,
            },
            stick_active: false,
            controller_lt_down: false,
            controller_rt_down: false,
            controller_rb_down: false,
            controller_look_pending: false,
            ds_e_held: false,
            ds_space_held: false,
            ds_ui_key_held: std::collections::HashSet::new(),
            ds_ui_capture_previous: false,
            ds_test_pad: None,
            key_stick: (0.0, 0.0),
            key_directions: ds_pc_input::HeldDirections::new(),
            pad_directions: ds_pc_input::HeldDirections::new(),
            pad_weapons: ds_weapon_input::PadWeapons::new(),
            key_stick_last: (0.0, 0.0),
            _sensor_ctx: sensor_ctx,
            accelerometer,
            virtual_cursor_last: None,
            virtual_cursor_last_unsticky: None,
            mouse_left_button: ds_mouse_capture::LeftButton::default(),
            ds_native_input_tick: Instant::now(),
            ds_diag_native_stick: None,
            ds_diag_native_look: (0.0,0.0),
            virtual_accelerometer_last: None,
            ds_aim_active: false,
            ds_mouse_right_down: false,
            controller_lt_physical_down: false,
            ds_shake_start: None,
            ds_shake_amp: 6.0,
            ds_shake_dur: 3.0,
            ds_shake_freq: 4.0,
            ds_shake_axis: 3,
            ds_look_accum: (0.0, 0.0),
            ds_look_dragging: false,
            ds_look_position: (240.0, 160.0),
            ds_mouse_capture: ds_mouse_capture::CaptureState::default(),
            ds_mouse_motion_gate: ds_mouse_capture::MotionGate::default(),
            ds_right_press_gate: ds_mouse_capture::RightPressGate::default(),
            ds_capture_suspended: false,
            ds_alt_released: false,
            ds_gamepad_cursor_mode: false,
            ds_gamepad_start_down: None,
            ds_gamepad_start_long: false,
            ds_gamepad_resume_latched: None,
            ds_script: Vec::new(),
            ds_script_loaded: false,
            ds_control_inbox: ds_control_inbox::Inbox::default(),
            ds_control_ready_logged: false,
            ds_macros: Vec::new(),
            ds_macros_loaded: false,
            ds_record_file: None,
            ds_record_enabled: true,
            on_main_stack: true,
        };

        // Set up OpenGL ES context used for splash screen and app UI rendering
        // (see src/frameworks/core_animation/composition.rs). OpenGL ES is used
        // because SDL2 won't let us use more than one graphics API in the same
        // window, and we also need OpenGL ES for the app's own rendering.
        let mut gl_ins = if options.prefer_gles2_context {
            create_gles2_ctx_no_parent_stack(&mut window)
        } else {
            create_gles1_ctx_no_parent_stack(&mut window, options)
        };
        {
            let gl_ctx = gl_ins.make_current(&mut window);
            log!("Driver info: {}", unsafe { gl_ctx.driver_description() });
        }
        window.internal_gl_ins = Some(gl_ins);

        if window.splash_image.is_some() {
            window.display_splash();
        }

        window
    }

    /// Poll for events from the OS. This needs to be done reasonably often
    /// (60Hz is probably fine) so that the host OS doesn't consider touchHLE
    /// to be unresponsive. Note that events are not returned by this function,
    /// since we often need to defer actually handling them.
    ///
    /// Since polling can be quite expensive, this function will skip it if it
    /// was called too recently.
    pub fn poll_for_events(&mut self, options: &Options) {
        if !self.on_main_stack {
            log!("Warning: poll_for_events called off main stack, skipping");
            return;
        }
        if self.backgrounded && self.event_queue.is_empty() && self.high_priority_event.is_none() {
            self.wait_until_foreground();
            return;
        }
        let now = Instant::now();
        // poll roughly twice per frame to try to avoid missing frames sometimes
        if now.duration_since(self.last_polled) < Duration::from_secs_f64(1.0 / 120.0) {
            return;
        }
        self.last_polled = now;
        self.ds_poll_control_inbox();

        fn transform_input_coords(
            window: &Window,
            (in_x, in_y): (f32, f32),
            independent_of_viewport: bool,
        ) -> (f32, f32) {
            let (vx, vy, vw, vh) = if independent_of_viewport {
                let (width, height) = size_for_orientation(
                    window.device_family,
                    window.device_orientation,
                    NonZeroU32::new(1).unwrap(),
                );
                (0, 0, width, height)
            } else {
                window.viewport()
            };
            // Clamp into the viewport. On hosts (Android, large desktops) the
            // SDL drawable is bigger than the iPhone's virtual screen and is
            // letterboxed inside the viewport. Touches landing in the
            // letterbox bars used to produce out-of-window iOS coordinates
            // (e.g. y == -91 or y == 570 for a 320x460 portrait window),
            // which made -[UIWindow hitTest:withEvent:] return nil for every
            // such touch. The "SUPER HACK" fallback in ui_touch then forced
            // the touch directly into the window object, bypassing all
            // subviews — so taps near the very top/bottom of a landscape
            // screen never reached overlay UI like CreateNewWorld dialogs
            // or the in-game chat field. Clamping to the viewport keeps the
            // touch on the nearest visible edge instead.
            let in_x = in_x.clamp(vx as f32, (vx + vw) as f32);
            let in_y = in_y.clamp(vy as f32, (vy + vh) as f32);
            // normalize to unit square centred on origin
            let x = (in_x - vx as f32) / vw as f32 - 0.5;
            let y = (in_y - vy as f32) / vh as f32 - 0.5;
            // rotate
            //
            // If the final EAGL presentation is not being rotated, the touch
            // path should not rotate either. Keep hit-testing in the normal
            // 320x480 UIKit space so EAGLView still receives the event; a
            // separate UITouch locationInView compatibility path can remap
            // the coordinates returned to the game.
            let [x, y] = if std::env::var_os("TOUCHHLE_DISABLE_PRESENT_ROTATION").is_some()
                || std::env::var_os("TOUCHHLE_DISABLE_TOUCH_ROTATION").is_some()
            {
                log_once!(
                    "TOUCHHLE_DISABLE_TOUCH_ROTATION: not rotating touch hit-test coordinates [this log will only be shown once]"
                );
                [x, y]
            } else {
                let matrix = window.rotation_matrix().inverse().unwrap();
                matrix.transform([x, y])
            };

            // back to pixels
            let (out_w, out_h) = window.size_unrotated_unscaled();
            let mut out_x = (x + 0.5) * out_w as f32;
            let mut out_y = (y + 0.5) * out_h as f32;

            // Optional hit-test tuning only. Do not use these unless you are
            // deliberately testing the UIKit hit-test position.
            if let Ok(offset) = std::env::var("TOUCHHLE_HITTEST_X_OFFSET") {
                if let Ok(offset) = offset.parse::<f32>() {
                    out_x += offset;
                }
            }
            if let Ok(offset) = std::env::var("TOUCHHLE_HITTEST_Y_OFFSET") {
                if let Ok(offset) = offset.parse::<f32>() {
                    out_y += offset;
                }
            }

            // Keep the result strictly *inside* the iOS window. CGRect
            // containment is half-open on the high edge (a point with
            // y == bounds.size.height is *outside*), so clamping to the
            // exclusive size of the window — e.g. y == 480 on an iPhone
            // 480-pt landscape screen — would still cause -[UIWindow
            // pointInside:] to return false. Subtract a single point.
            let max_x = (out_w.saturating_sub(1)) as f32;
            let max_y = (out_h.saturating_sub(1)) as f32;
            let out_x = out_x.clamp(0.0, max_x);
            let out_y = out_y.clamp(0.0, max_y);
            // Round to match touch precision of official devices.
            (out_x.round(), out_y.round())
        }
        fn transform_virt_accel_coords(window: &Window, (in_x, in_y): (i32, i32)) -> (f32, f32) {
            let (_, _, vw, vh) = window.viewport();
            let out_x = ((in_x as f32 / vw as f32) * 2.0 - 1.0).clamp(-1.0, 1.0);
            let out_y = ((in_y as f32 / vh as f32) * 2.0 - 1.0).clamp(-1.0, 1.0);
            (out_x, out_y)
        }
        fn translate_button(button: sdl2::controller::Button) -> Option<crate::options::Button> {
            match button {
                sdl2::controller::Button::DPadLeft => Some(crate::options::Button::DPadLeft),
                sdl2::controller::Button::DPadUp => Some(crate::options::Button::DPadUp),
                sdl2::controller::Button::DPadRight => Some(crate::options::Button::DPadRight),
                sdl2::controller::Button::DPadDown => Some(crate::options::Button::DPadDown),
                sdl2::controller::Button::Start => Some(crate::options::Button::Start),
                sdl2::controller::Button::A => Some(crate::options::Button::A),
                sdl2::controller::Button::B => Some(crate::options::Button::B),
                sdl2::controller::Button::X => Some(crate::options::Button::X),
                sdl2::controller::Button::Y => Some(crate::options::Button::Y),
                sdl2::controller::Button::LeftShoulder => {
                    Some(crate::options::Button::LeftShoulder)
                }
                sdl2::controller::Button::RightShoulder => {
                    Some(crate::options::Button::RightShoulder)
                }
                _ => None,
            }
        }
        fn finger_absolute_coords(window: &Window, (x, y): (f32, f32)) -> (f32, f32) {
            let (screen_width, screen_height) = window.window.drawable_size();
            (screen_width as f32 * x, screen_height as f32 * y)
        }

        // TAS-lite: `TOUCHHLE_DS_SCRIPT=<file>` drives touch input from a script
        // so a session can reproduce a specific moment unattended. Lines are
        // "<frame> tap|down|move|up <x> <y>", sorted by frame, coordinates in
        // landscape iOS points (the same space `--stick-to-touch` uses).
        if let Ok(path) = std::env::var("TOUCHHLE_DS_MACROS") {
            if !self.ds_macros_loaded {
                self.ds_macros_loaded = true;
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        for line in text.lines() {
                            let line = line.trim();
                            if line.is_empty() || line.starts_with('#') {
                                continue;
                            }
                            if let Some((key, action)) = line.split_once('=') {
                                let key = key.trim().to_string();
                                let action = action.trim().to_string();
                                if !key.is_empty() && !action.is_empty() {
                                    self.ds_macros.push((key, action));
                                }
                            }
                        }
                        log!(
                            "[DSmacro] loaded {} key macros from {}",
                            self.ds_macros.len(),
                            path
                        );
                    }
                    Err(e) => log!("[DSmacro] failed to read {}: {}", path, e),
                }
            }
        }
        if let Ok(path) = std::env::var("TOUCHHLE_DS_SCRIPT") {
            if !self.ds_script_loaded {
                self.ds_script_loaded = true;
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        for line in text.lines() {
                            let line = line.trim();
                            if line.is_empty() || line.starts_with('#') {
                                continue;
                            }
                            let mut it = line.splitn(2, char::is_whitespace);
                            let frame = it.next().and_then(|v| v.parse::<u64>().ok());
                            let rest = it.next().unwrap_or("").trim().to_string();
                            if let Some(frame) = frame {
                                if !rest.is_empty() {
                                    self.ds_script.push((frame, rest));
                                }
                            }
                        }
                        log!(
                            "[DSscript] loaded {} actions from {}",
                            self.ds_script.len(),
                            path
                        );
                    }
                    Err(e) => log!("[DSscript] failed to read {}: {}", path, e),
                }
            }
        }
        // Relative-mouse look (once per frame, before the script queue is drained):
        // turn the deltas accumulated since the last frame into ONE look drag.
        {
            // `forced_off` is the click-mode switch (right-click / F8):
            // while it is set the mouse stays absolute so the touch channel and
            // the user's own clicking keep working.
            let focused = unsafe { sdl2_sys::SDL_GetKeyboardFocus() == self.window.raw() };
            let mouse_want = if self.ds_mouse_capture.forced_off
                || crate::ds_frontend_controller::capture_menu()
                || crate::ds_frontend_controller::capture_scene_dialog()
                || (ds_native_pointer_enabled() && !DS_GAMEPLAY_ACTIVE.load(std::sync::atomic::Ordering::Relaxed))
                || self.ds_capture_suspended
                || self.mouse_left_button.pointer_drag()
                || self.ds_alt_released
                || self.ds_gamepad_cursor_mode
                || !focused
            {
                false
            } else {
                match ds_look_mode() {
                    DsLookMode::Off => false,
                    DsLookMode::Always => true,
                    DsLookMode::WhileAiming => {
                        DS_AIM_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
                    }
                }
            };
            // Controller camera motion uses the guest look gesture but must not
            // acquire the host mouse when mouse capture is disabled.
            if self.ds_mouse_capture.should_change(mouse_want)
                && self.ds_set_mouse_capture(mouse_want, "frame policy")
            {
                log!(
                    "[DSlook] relative mouse {} (mode={:?}, gain={}, anchor={:?})",
                    if mouse_want {
                        "ON (cursor hidden)"
                    } else {
                        "off"
                    },
                    match ds_look_mode() {
                        DsLookMode::Off => "off",
                        DsLookMode::Always => "all",
                        DsLookMode::WhileAiming => "aim",
                    },
                    ds_look_gain(),
                    ds_look_anchor()
                );
            }
            if ds_native_inputs() {
                if self.ds_look_dragging {
                    let coords=transform_input_coords(self,self.ds_look_position,true);
                    self.event_queue.push_back(Event::TouchesCancelled(HashMap::from([(FingerId::Look,coords)])));self.ds_look_dragging=false;
                }
            } else {
            let want = self.ds_mouse_capture.look_active(focused, self.controller_look_pending);
            if want {
                let (ax, ay) = self.ds_look_accum;
                self.controller_look_pending = false;
                if !self.ds_look_dragging && (ax.abs() >= 0.05 || ay.abs() >= 0.05) {
                    let (lx, ly) = ds_look_anchor();
                    let start = transform_input_coords(self, (lx, ly), true);
                    self.event_queue
                        .push_back(Event::TouchesDown(HashMap::from([(FingerId::Look, start)])));
                    self.ds_look_position = (lx, ly);
                    self.ds_look_dragging = true;
                    // Preserve this frame's motion; apply it after the touch has
                    // been established for at least one frame.
                    self.ds_look_accum = (ax, ay);
                } else if self.ds_look_dragging && (ax.abs() >= 0.05 || ay.abs() >= 0.05) {
                    let (lx, ly) = ds_look_anchor();
                    let target = (
                        (self.ds_look_position.0 + ax).clamp(12.0, 468.0),
                        (self.ds_look_position.1 + ay).clamp(12.0, 308.0),
                    );
                    let reached_edge =
                        (target.0 - lx).abs() > 112.0 || (target.1 - ly).abs() > 92.0;
                    if reached_edge {
                        let current = transform_input_coords(self, self.ds_look_position, true);
                        let centre = transform_input_coords(self, (lx, ly), true);
                        self.event_queue.push_back(Event::TouchesCancelled(HashMap::from([(
                            FingerId::Look,
                            current,
                        )])));
                        self.event_queue
                            .push_back(Event::TouchesDown(HashMap::from([(
                                FingerId::Look,
                                centre,
                            )])));
                        self.ds_look_position = (lx, ly);
                        self.ds_look_accum = (ax, ay);
                    } else {
                        let coords = transform_input_coords(self, target, true);
                        self.event_queue
                            .push_back(Event::TouchesMove(HashMap::from([(
                                FingerId::Look,
                                coords,
                            )])));
                        self.ds_look_position = target;
                        self.ds_look_accum = (0.0, 0.0);
                    }
                } else {
                    self.ds_look_accum = (0.0, 0.0);
                }
            } else {
                if self.ds_look_dragging {
                    let coords = transform_input_coords(self, self.ds_look_position, true);
                    self.event_queue
                        .push_back(Event::TouchesCancelled(HashMap::from([(FingerId::Look, coords)])));
                    self.ds_look_dragging = false;
                    echo!("[DSlook] camera gesture cancelled; no tap completion");
                }
                self.ds_look_accum = (0.0, 0.0);
            }
        }
        }
        if !self.ds_script.is_empty() {
            // Execute everything that is due; expansions (tap/swipe/repeat) push
            // further entries which are picked up on later frames.
            let frame = DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
            let mut due: Vec<(u64, String)> = Vec::new();
            let mut rest: Vec<(u64, String)> = Vec::new();
            for entry in self.ds_script.drain(..) {
                if entry.0 <= frame {
                    due.push(entry);
                } else {
                    rest.push(entry);
                }
            }
            self.ds_script = rest;
            for (at, text) in due {
                let mut it = text.split_whitespace();
                let action = it.next().unwrap_or("").to_string();
                let nums: Vec<f32> = it.filter_map(|v| v.parse::<f32>().ok()).collect();
                let raw = action.starts_with("raw");
                let base = action.trim_start_matches("raw").to_string();
                let x = nums.first().copied().unwrap_or(0.0);
                let y = nums.get(1).copied().unwrap_or(0.0);
                let coords = if raw {
                    (x, y)
                } else {
                    transform_input_coords(self, (x, y), true)
                };
                let id = FingerId::Touch(-1);
                log!("[DSscript] frame {} >= {} action {}", frame, at, text);
                match base.as_str() {
                    "down" => {
                        self.event_queue
                            .push_back(Event::TouchesDown(HashMap::from([(id, coords)])));
                    }
                    "move" => {
                        self.event_queue
                            .push_back(Event::TouchesMove(HashMap::from([(id, coords)])));
                    }
                    "up" => {
                        self.event_queue
                            .push_back(Event::TouchesUp(HashMap::from([(id, coords)])));
                    }
                    "tap" => {
                        self.event_queue
                            .push_back(Event::TouchesDown(HashMap::from([(id, coords)])));
                        // Lift at the SAME (transformed) point we pressed: the
                        // `raw*` actions skip the coordinate transform, so
                        // passing the raw spec values here made the touch jump
                        // between down and up - i.e. a drag, not a tap (that is
                        // why every emulator-injected tap looked inert while the
                        // host mouse click, which lifts where it pressed, works;
                        // DECOMPILE.md D-013/D-014).
                        // Hold for `TOUCHHLE_DS_TAP_HOLD` frames (default 1) -
                        // see `ds_tap_hold()` for why the hold is a variable.
                        self.ds_script.push((
                            at + nums.get(2).copied().filter(|n|n.is_finite())
                                .map(|n|n.clamp(1.,12.) as u64).unwrap_or_else(ds_tap_hold),
                            format!("rawup {} {}", coords.0, coords.1),
                        ));
                    }
                    // swipe x y x2 y2 - drag from (x,y) to (x2,y2) in 8 steps.
                    "swipe" => {
                        let x2 = nums.get(2).copied().unwrap_or(x);
                        let y2 = nums.get(3).copied().unwrap_or(y);
                        // Interpolate in the *guest* space, so both endpoints go
                        // through the same transform as a simple tap would.
                        let target = if raw {
                            (x2, y2)
                        } else {
                            transform_input_coords(self, (x2, y2), true)
                        };
                        const STEPS: u64 = 8;
                        self.event_queue
                            .push_back(Event::TouchesDown(HashMap::from([(id, coords)])));
                        self.ds_script
                            .push((at + STEPS + 1, format!("rawup {} {}", target.0, target.1)));
                        for step in 1..=STEPS {
                            let t = step as f32 / (STEPS + 1) as f32;
                            self.ds_script.push((
                                at + step,
                                format!(
                                    "rawmove {} {}",
                                    coords.0 + (target.0 - coords.0) * t,
                                    coords.1 + (target.1 - coords.1) * t
                                ),
                            ));
                        }
                    }
                    // repeat x y n gap - tap (x,y) n times, one every gap frames.
                    "repeat" => {
                        let n = nums.get(2).copied().unwrap_or(1.0).max(1.0) as u64;
                        let gap = nums.get(3).copied().unwrap_or(2.0).max(1.0) as u64;
                        for k in 0..n {
                            self.ds_script
                                .push((at + k * gap, format!("tap {} {}", x, y)));
                        }
                    }
                    other => log!("[DSscript] unknown action {:?} at frame {}", other, at),
                }
            }
            self.ds_script.sort_by_key(|e| e.0);
        }

        let mut controller_updated = false;
        // `TOUCHHLE_DS_AIM_HOLD="x,y"`: push the PC-style aim finger down once,
        // so the game enters its aim state without a real right mouse button
        // (the VM input tool can only click the left button). Used to make the
        // aim-mode HUD buttons appear for the action-code census
        // (DECOMPILE.md D-032).
        if let Ok(spec) = std::env::var("TOUCHHLE_DS_AIM_HOLD") {
            static AIM_HELD: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !AIM_HELD.swap(true, std::sync::atomic::Ordering::Relaxed) {
                let at: u64 = std::env::var("TOUCHHLE_DS_AIM_HOLD_AT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                let now = DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
                if now < at {
                    // Not yet: let the navigation/menus run untouched first.
                    AIM_HELD.store(false, std::sync::atomic::Ordering::Relaxed);
                } else {
                    let mut it = spec.split(',');
                    let ax: f32 = it
                        .next()
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(360.0);
                    let ay: f32 = it
                        .next()
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(160.0);
                    let coords = transform_input_coords(self, (ax, ay), true);
                    self.event_queue
                        .push_back(Event::TouchesDown(HashMap::from([(
                            FingerId::MouseAim,
                            coords,
                        )])));
                    echo!("[DSaim] forced aim hold at ({:.0},{:.0})", ax, ay);
                }
            }
        }
        // event_pump doesn't have a method to peek on events
        // so, we keep track of an unconsumed one from a previous loop iteration
        // FIXME: use peek_event() from even_subsystem
        let mut previous_event: Option<sdl2::event::Event> = None;
        // Emulator-requested tap (HUD state tracker -> AUTO_TAP). Must run every
        // frame, not only when a script action is pending - trying to hook it
        // into the script-action loop below worked only while that loop had work
        // to do (measured 2026-10-11: the request was logged, the tap never sent).
        let auto = AUTO_TAP.swap(0, std::sync::atomic::Ordering::Relaxed);
        if auto != 0 {
            let ax = ((auto >> 16) & 0xFFFF) as f32;
            let ay = (auto & 0xFFFF) as f32;
            let coords = transform_input_coords(self, (ax, ay), true);
            let id = FingerId::Touch(-1);
            self.event_queue
                .push_back(Event::TouchesDown(HashMap::from([(id, coords)])));
            // Same fish as the script `tap` above: lift where we pressed.
            // NOTE: the frame number here is ABSOLUTE and the script loop runs
            // *before* this block, so `0` means "next iteration" (1 frame);
            // use the live counter plus the hold.
            let hold = ds_tap_hold();
            let at = DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed) + hold;
            self.ds_script
                .push((at, format!("rawup {} {}", coords.0, coords.1)));
            echo!(
                "[DStap] auto tap sent at ({}, {}) dev=({:.1},{:.1}) hold={} up@f={}",
                ax,
                ay,
                coords.0,
                coords.1,
                hold,
                at
            );
        }
        crate::ds_frontend_controller::observe_frame(
            DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),
        );
        self.ds_gamepad_start_tick();
        let ui_capture=crate::ds_frontend_controller::capture_menu()||crate::ds_frontend_controller::capture_scene_dialog();
        if ui_capture&&!self.ds_ui_capture_previous {self.ds_cancel_inputs_for_ui();}
        if !ui_capture&&self.ds_ui_capture_previous {self.ds_reconcile_aim_after_ui();}
        self.ds_ui_capture_previous=ui_capture;
        if ui_capture || self.ds_capture_suspended || self.ds_gamepad_cursor_mode || !ds_native_hud_enabled() {
            self.pad_weapons.clear();
        } else if let Some(command)=self.pad_weapons.tick(ds_weapon_ms()) {ds_queue_hud(command);}
        while self.enable_event_polling {
            use sdl2::event::Event as E;
            let mut event = if let Some(e) = previous_event.take() {
                match e {
                    E::Unknown { .. } => (),
                    _ => log_dbg!("Consuming previous event: {:?}", e),
                }
                e
            } else if let Some(e) = self.event_pump.poll_event() {
                match e {
                    E::Unknown { .. } => (),
                    _ => log_dbg!("Consuming new event: {:?}", e),
                }
                e
            } else {
                break;
            };

            if let E::MouseMotion { timestamp, .. } = &event {
                if !self.ds_mouse_motion_gate.accepts(*timestamp) { continue; }
            }
            if let E::MouseMotion { x, y, .. } = &event {
                if crate::ds_frontend_controller::capture_menu() {
                    let (vw,vh)=self.window.size();
                    let (dw,dh)=self.window.drawable_size();
                    let (vx,vy,w,h)=self.viewport();
                    crate::ds_frontend_controller::hover(crate::ds_frontend_controller::Point{
                        x:(*x as f32*dw as f32/vw.max(1) as f32-vx as f32)*480.0/w.max(1) as f32,
                        y:(*y as f32*dh as f32/vh.max(1) as f32-vy as f32)*320.0/h.max(1) as f32,
                    });
                }
            }
            if ds_input_trace_enabled() {
                match &event {
                    E::KeyDown { keycode, scancode, repeat, keymod, .. } => echo!(
                        "[DSinputTrace] key DOWN raw={:?} scancode={:?} repeat={} mods={:?}",
                        keycode, scancode, repeat, keymod
                    ),
                    E::KeyUp { keycode, scancode, keymod, .. } => echo!(
                        "[DSinputTrace] key UP raw={:?} scancode={:?} mods={:?}",
                        keycode, scancode, keymod
                    ),
                    E::ControllerButtonDown { button, .. } => {
                        echo!("[DSinputTrace] pad DOWN raw={:?}", button)
                    }
                    E::ControllerButtonUp { button, .. } => {
                        echo!("[DSinputTrace] pad UP raw={:?}", button)
                    }
                    E::MouseButtonDown { mouse_btn, x, y, .. } => echo!(
                        "[DSinputTrace] mouse DOWN button={:?} at=({}, {})",
                        mouse_btn, x, y
                    ),
                    E::MouseButtonUp { mouse_btn, x, y, .. } => echo!(
                        "[DSinputTrace] mouse UP button={:?} at=({}, {})",
                        mouse_btn, x, y
                    ),
                    E::Window { win_event: sdl2::event::WindowEvent::FocusLost, .. } => {
                        echo!("[DSinputTrace] window focus LOST")
                    }
                    E::Window { win_event: sdl2::event::WindowEvent::FocusGained, .. } => {
                        echo!("[DSinputTrace] window focus GAINED")
                    }
                    _ => {}
                }
            }

            if !cfg!(target_os = "android") {
                use ds_gamepad_bindings::InputMode;
                let gamepad = matches!(event, E::ControllerButtonDown { .. } | E::ControllerButtonUp { .. } | E::ControllerAxisMotion { .. });
                let test_pad=match event {E::ControllerButtonDown{which,..}|E::ControllerButtonUp{which,..}|E::ControllerAxisMotion{which,..}=>self.ds_test_pad.as_ref().is_some_and(|p|p.instance==which),_=>false};
                if gamepad && ds_background_test() && !test_pad {continue;}
                let keyboard_mouse = matches!(event, E::KeyDown { .. } | E::KeyUp { .. } | E::MouseButtonDown { .. } | E::MouseButtonUp { .. } | E::MouseMotion { .. } | E::MouseWheel { .. });
                let diagnostic = matches!(event, E::KeyDown { keycode: Some(sdl2::keyboard::Keycode::F5 | sdl2::keyboard::Keycode::F7 | sdl2::keyboard::Keycode::F9 | sdl2::keyboard::Keycode::F11 | sdl2::keyboard::Keycode::F12), .. });
                let frontend_active = crate::ds_frontend_controller::capture_menu()
                    || crate::ds_frontend_controller::capture_scene_dialog();
                if ((gamepad || keyboard_mouse) && self.ds_capture_suspended && !test_pad)
                    || (!frontend_active && gamepad && ds_input_mode() == InputMode::Keyboard)
                    || (!frontend_active && keyboard_mouse && !diagnostic && ds_input_mode() == InputMode::Gamepad) { continue; }
                match event {
                    E::KeyDown{..}|E::MouseButtonDown{..}|E::MouseWheel{..}=>crate::ds_frontend_controller::note_input(true),
                    E::MouseMotion{xrel,yrel,..} if xrel.abs()>2||yrel.abs()>2=>crate::ds_frontend_controller::note_input(true),
                    E::ControllerButtonDown{..}=>crate::ds_frontend_controller::note_input(false),
                    E::ControllerAxisMotion{value,..} if i32::from(value).abs()>=8192=>crate::ds_frontend_controller::note_input(false),
                    _=>{},
                }
                if !crate::ds_frontend_controller::capture_menu()
                    && !crate::ds_frontend_controller::capture_scene_dialog()
                {
                    match &mut event {
                        E::ControllerButtonDown { button, .. } | E::ControllerButtonUp { button, .. } => *button = ds_remap_pad(*button, false),
                        _ => {}
                    }
                }
            }
            if !crate::ds_frontend_controller::capture_menu()
                && !crate::ds_frontend_controller::capture_scene_dialog()
            {
                match &mut event {
                    E::KeyDown { keycode, .. } | E::KeyUp { keycode, .. } => {
                        if let Some(key) = *keycode {
                        *keycode = ds_remap_key(key);
                    }
                    }
                    _ => {}
                }
            }
            // Virtual accelerometer
            match event {
                E::Window {
                    win_event: sdl2::event::WindowEvent::FocusLost,
                    ..
                } => {
                    // Win/Alt-Tab must go through the game's own HUD pause
                    // handler before gameplay input is cleared. The request
                    // is state-gated and coalesced by the frontend controller.
                    if crate::ds_frontend_controller::request_native_pause() {
                        echo!("[DSfocus] focus lost -> native pause queued");
                    }
                    // Win/Alt-Tab temporarily releases the mouse. If capture was
                    // active, remember it and restore it automatically on focus
                    // return; an explicit F8/right-click click mode stays off.
                    self.ds_mouse_capture.recapture_on_focus = (self.ds_mouse_capture.captured || self.ds_alt_released)
                        && ds_look_mode() != DsLookMode::Off;
                    self.ds_alt_released = false;
                    self.ds_capture_suspended = true;
                    self.ds_cancel_inputs();
                    if self.controller_lt_down {
                        crate::window::DS_AIM_REQUEST
                            .store(0, std::sync::atomic::Ordering::Relaxed);
                    }
                    if self.controller_rt_down || self.controller_rb_down {
                        crate::window::DS_FIRE_RELEASE_REQUEST
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    self.controller_lt_down = false;
                    DS_CONTROLLER_LT_HELD.store(false, std::sync::atomic::Ordering::Relaxed);
                    self.controller_rt_down = false;
                    self.controller_rb_down = false;
                    self.ds_look_accum = (0.0, 0.0);
                    self.controller_look_pending = false;
                    self.ds_mouse_capture.captured = false;
                    self.ds_set_mouse_capture(false, "window focus lost");
                    if self.ds_look_dragging {
                        let coords = transform_input_coords(self, self.ds_look_position, true);
                        self.event_queue
                            .push_back(Event::TouchesCancelled(HashMap::from([(FingerId::Look, coords)])));
                        self.ds_look_dragging = false;
                    }
                    continue;
                }
                E::Window {
                    win_event: sdl2::event::WindowEvent::FocusGained,
                    ..
                } => {
                    let recapture = self.ds_mouse_capture.recapture_on_focus && ds_look_mode() != DsLookMode::Off;
                    self.ds_capture_suspended = false;
                    self.ds_mouse_capture.recapture_on_focus = false;
                    if recapture {
                        self.ds_mouse_capture.forced_off = false;
                        self.ds_look_accum = (0.0, 0.0);
                        echo!("[DSlook] focus returned -> recapturing mouse");
                    }
                    continue;
                }
                E::MouseButtonDown {
                    x,
                    y,
                    mouse_btn: MouseButton::Right,
                    ..
                } => {
                    // When PC-style aiming is on, the right button is the
                    // aim trigger (handled in the touch block below) and must
                    // not also drive tilt. Same when the right button is the
                    // look/click mode switch: a mode switch must not jerk the
                    // camera through the virtual accelerometer.
                    if ds_aim_point().is_none() && !ds_look_capture_switch_armed() {
                        let (x, y) = transform_virt_accel_coords(self, (x, y));
                        self.virtual_accelerometer_last = Some((x, y, true));
                    }
                }
                E::MouseMotion {
                    x, y, mousestate, ..
                } => {
                    if mousestate.right()
                        && ds_aim_point().is_none()
                        && !ds_look_capture_switch_armed()
                    {
                        let (x, y) = transform_virt_accel_coords(self, (x, y));
                        self.virtual_accelerometer_last = Some((x, y, true));
                    }
                }
                E::MouseButtonUp {
                    x,
                    y,
                    mouse_btn: MouseButton::Right,
                    ..
                } => {
                    if !ds_look_capture_switch_armed() {
                        let (x, y) = transform_virt_accel_coords(self, (x, y));
                        self.virtual_accelerometer_last = Some((x, y, false));
                    }
                }
                _ => {}
            }

            if let E::KeyDown {
                keycode: Some(k),
                repeat: false,
                ..
            } = event
            {
                if std::env::var_os("TOUCHHLE_DS_LOG_KEYS").is_some() {
                    use std::sync::{Mutex, OnceLock};
                    static SEEN: OnceLock<Mutex<std::collections::HashSet<String>>> =
                        OnceLock::new();
                    let name = format!("{:?}", k);
                    let set = SEEN.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
                    if set.lock().unwrap().insert(name.clone()) {
                        log!("[DSkey] KeyDown {:?}", k);
                    }
                }
            }

            // Release edge latches before any modal UI consumes KeyUp. Opening
            // a dialog with E must not strand its gameplay held state.
            if let E::KeyUp{keycode:Some(key),..}=&event {
                self.ds_ui_key_held.remove(key);
                if *key==sdl2::keyboard::Keycode::E {self.ds_e_held=false;}
            }
            let frontend_key_action = match &event {
                E::KeyDown { keycode: Some(k), .. }
                | E::KeyUp { keycode: Some(k), .. } => {
                    crate::ds_frontend_controller::keyboard_action(*k)
                }
                _ => None,
            };
            if let Some(action) = frontend_key_action {
                let menu = crate::ds_frontend_controller::capture_menu();
                let scene = crate::ds_frontend_controller::capture_scene_dialog()
                    && matches!(
                        action,
                        crate::ds_frontend_controller::Action::Confirm
                            | crate::ds_frontend_controller::Action::Cancel
                    );
                if menu || scene {
                    let pressed=match &event {
                        E::KeyDown{keycode:Some(key),repeat:false,..}=>self.ds_ui_key_held.insert(*key),
                        E::KeyUp{keycode:Some(key),..}=>{self.ds_ui_key_held.remove(key);if *key==sdl2::keyboard::Keycode::E {self.ds_e_held=false;}false},
                        _=>false,
                    };
                    if pressed {
                        let point = if menu {
                            crate::ds_frontend_controller::queue_menu_action(action);
                            None::<crate::ds_frontend_controller::Point>
                        } else {
                            crate::ds_frontend_controller::queue_scene_action(action);
                            None
                        };
                        if let Some(point) = point {
                            echo!(
                                "[DSfrontendInput] keyboard {:?} -> tap ({:.0},{:.0})",
                                action,
                                point.x,
                                point.y
                            );
                            self.ds_queue_action(format!("tap {} {}", point.x, point.y));
                        }
                    }
                    continue;
                }
            }

            if crate::ds_frontend_controller::capture_menu()||crate::ds_frontend_controller::capture_scene_dialog() {
                if let E::KeyDown{keycode:Some(key),..}|E::KeyUp{keycode:Some(key),..}=&event {
                    use sdl2::keyboard::Keycode as K;
                    if !matches!(*key,K::F1|K::F2|K::F3|K::F4|K::F5|K::F6|K::F7|K::F8|K::F9|K::F10|K::F11|K::F12|K::LAlt|K::RAlt|K::LGui|K::RGui) {continue;}
                }
            }
            // Permanent input repair: G must not fall through to the legacy
            // diagnostic action trigger when experimental native HUD is off.
            if let E::KeyDown { keycode: Some(key), repeat: false, .. } = event {
                use sdl2::keyboard::Keycode as K;
                // ESC is a first-class native HUD toggle. Do this before the
                // macro/legacy paths so it cannot be swallowed by a stale
                // frontend capture or a remapped P macro. In a pause state
                // the same key routes to the native play/continue button.
                if key == K::Escape && ds_native_hud_enabled() {
                    if crate::ds_frontend_controller::pause_menu() {
                        crate::ds_frontend_controller::request_native_resume();
                        echo!("[DSkey] ESC -> native resume");
                    } else {
                        ds_queue_hud(ds_hud_commands::HudCommand::Pause);
                        echo!("[DSkey] ESC -> native pause");
                    }
                    continue;
                }
                match key {
                    K::G => { DS_LOCATOR_REQUEST.store(true, std::sync::atomic::Ordering::Relaxed); continue; }
                    K::Z => { DS_QUICK_TURN_REQUEST.store(true, std::sync::atomic::Ordering::Relaxed); continue; }
                    _ => {}
                }
            }
            if let E::KeyDown {
                keycode: Some(sdl2::keyboard::Keycode::Q),
                repeat: false,
                ..
            } = &event
            {
                DS_DEATH_CANCEL_REQUEST.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if ds_native_hud_enabled() {
                if let E::KeyDown { keycode: Some(key), repeat: false, .. } = event {
                    if let Some(command) = ds_hud_commands::HudCommand::from_key(&key.name()) {
                        ds_queue_hud(command);
                        continue;
                    }
                }
            }
            let produced = match event {
                E::KeyDown {
                    keycode: Some(k),
                    repeat: false,
                    ..
                } if self
                    .ds_macros
                    .iter()
                    .any(|(name, _)| macro_key_matches(name, k)) =>
                {
                    if let Some((_, action)) = self
                        .ds_macros
                        .iter()
                        .find(|(name, _)| macro_key_matches(name, k))
                    {
                        let action = action.clone();
                        let at = DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed) + 1;
                        log!("[DSmacro] {:?} -> \"{}\" at frame {}", k, action, at);
                        // A macro may chain several actions with ';' - useful
                        // for sweeping a set of candidate touch points with a
                        // single key. They run one frame apart.
                        //
                        // A `wait N` element advances every *following*
                        // element by N extra frames. That matters whenever the
                        // game has to animate between steps: e.g. switching
                        // weapon means "tap the top-right icon to open the
                        // wheel, wait for it to appear, then tap the slot",
                        // and one frame is far too soon.
                        let mut offset: u64 = 0;
                        for (i, part) in action.split(';').enumerate() {
                            let part = part.trim();
                            if part.is_empty() {
                                continue;
                            }
                            let mut words = part.split_whitespace();
                            if words.next() == Some("wait") {
                                offset += words
                                    .next()
                                    .and_then(|v| v.parse::<u64>().ok())
                                    .unwrap_or(0);
                                continue;
                            }
                            self.ds_script
                                .push((at + i as u64 + offset, part.to_string()));
                        }
                    }
                    continue;
                }
                E::Quit { .. } => {
                    if !self.ds_confirm_quit() { continue; }
                    Event::Quit
                },
                E::MouseButtonDown {
                    x,
                    y,
                    mouse_btn: MouseButton::Left,
                    ..
                } => {
                    let released = self.ds_mouse_capture.forced_off || self.ds_capture_suspended || self.ds_alt_released || self.ds_gamepad_cursor_mode;
                    let gameplay = !released && !crate::ds_frontend_controller::capture_menu()
                        && !crate::ds_frontend_controller::capture_scene_dialog()
                        && (self.ds_mouse_capture.captured || DS_AIM_ACTIVE.load(std::sync::atomic::Ordering::Relaxed));
                    match self.mouse_left_button.down((x as f32, y as f32), ds_aim_point(), gameplay) {
                        ds_mouse_capture::LeftPress::Fire((ax, ay)) => {
                            if ds_native_inputs() {
                                DS_MOUSE_KINESIS_LAUNCH_REQUEST
                                    .store(true, std::sync::atomic::Ordering::Relaxed);
                                ds_fire_control::enqueue(ds_fire_control::Input::Fire(ds_fire_control::Source::Mouse,None));
                                continue;
                            }
                            let coords = transform_input_coords(self, (ax, ay), true);
                            echo!("[DSaim] fire at ({:.0},{:.0}) aim_active={}", ax, ay, self.ds_aim_active);
                            Event::TouchesDown(HashMap::from([(FingerId::MouseFire, coords)]))
                        }
                        ds_mouse_capture::LeftPress::Pointer(point) => {
                            let coords = transform_input_coords(self, point, false);
                            echo!("[DSmouse] down win=({},{}) -> touch=({:.1},{:.1}) viewport={:?}", x, y, coords.0, coords.1, self.viewport());
                            Event::TouchesDown(HashMap::from([(FingerId::Mouse, coords)]))
                        }
                    }
                }
                E::MouseMotion { xrel, yrel, .. }
                    if self.ds_mouse_capture.gameplay_pointer(self.ds_capture_suspended, self.ds_alt_released, self.ds_gamepad_cursor_mode) => {
                    let g = ds_look_gain();
                    let (invert_x, invert_y) = ds_look_invert();
                    self.ds_look_accum.0 += xrel as f32 * g * invert_x;
                    self.ds_look_accum.1 += yrel as f32 * g * invert_y;
                    continue;
                }
                E::MouseMotion { x, y, mousestate, .. }
                    if mousestate.left() && self.mouse_left_button.pointer_drag() => {
                    let coords = transform_input_coords(self, (x as f32, y as f32), false);
                    Event::TouchesMove(HashMap::from([(FingerId::Mouse, coords)]))
                }
                E::MouseButtonUp { x, y, mouse_btn: MouseButton::Left, .. } => {
                    match self.mouse_left_button.up() {
                        Some(ds_mouse_capture::LeftPress::Fire(point)) => {
                            if ds_native_inputs() {ds_fire_control::enqueue(ds_fire_control::Input::Release(ds_fire_control::Source::Mouse));continue;}
                            let coords = transform_input_coords(self, point, true);
                            Event::TouchesUp(HashMap::from([(FingerId::MouseFire, coords)]))
                        }
                        Some(ds_mouse_capture::LeftPress::Pointer(point)) => {
                            // Origin-relative stick release must use the Down point.
                            let coords = transform_input_coords(self, point, false);
                            echo!("[DSmouse] up win=({},{}) -> release at down=({:.0},{:.0}) touch=({:.1},{:.1})", x, y, point.0, point.1, coords.0, coords.1);
                            Event::TouchesUp(HashMap::from([(FingerId::Mouse, coords)]))
                        }
                        None => continue, // focus cancellation already released it
                    }
                }
                E::MouseButtonDown {
                    mouse_btn: MouseButton::Right,
                    ..
                } if ds_look_capture_switch_armed() => {
                    // ★ Look mode <-> click mode. While the mouse is captured
                    // SDL pins the pointer to the window centre, so absolute
                    // clicks (the whole touch channel: menus, HUD, E/C/R/F
                    // fallbacks) cannot land where they are aimed. The user
                    // asked for a switch instead of an invisible trap; right
                    // button is that switch whenever mouse look is on and it is
                    // not already taken by PC-style aiming.
                    self.ds_mouse_capture.forced_off = !self.ds_mouse_capture.forced_off;
                    if self.ds_mouse_capture.forced_off { self.ds_cancel_inputs(); }
                    echo!(
                        "[DSlook] right-click -> {} mode ({})",
                        if self.ds_mouse_capture.forced_off {
                            "CLICK"
                        } else {
                            "LOOK"
                        },
                        if self.ds_mouse_capture.forced_off {
                            "mouse released: pointer is absolute again, use it to click the UI"
                        } else {
                            "mouse captured: move to turn the camera, right-click to get the pointer back"
                        }
                    );
                    continue;
                }
                E::MouseButtonDown {
                    mouse_btn: MouseButton::Right,
                    timestamp,
                    ..
                } if ds_aim_point().is_some() && !self.ds_mouse_capture.forced_off
                    && !self.ds_capture_suspended && !self.ds_alt_released && !self.ds_gamepad_cursor_mode => {
                    self.ds_mouse_right_down = true;
                    if !self.ds_right_press_gate.accepts(timestamp) {
                        echo!("[DSaim] right down rate-limited: max=2Hz; release remains immediate");
                        continue;
                    }
                    self.ds_aim_active = true;
                    DS_AIM_REQUEST.store(if ds_aim_toggle("MOUSE") { 2 } else { 1 }, std::sync::atomic::Ordering::Relaxed);
                    if ds_aim_toggle("MOUSE") { DS_FIRE_RELEASE_REQUEST.store(true, std::sync::atomic::Ordering::Relaxed); }
                    echo!("[DSaim] right down -> native aim request mode={}", if ds_aim_toggle("MOUSE") { "toggle" } else { "hold" });
                    continue;
                }
                E::MouseButtonUp {
                    mouse_btn: MouseButton::Right,
                    ..
                } if self.ds_mouse_right_down || self.ds_aim_active => {
                    let release =
                        ds_take_aim_release(&mut self.ds_mouse_right_down, self.ds_aim_active);
                    self.ds_aim_active = false;
                    if release && !ds_aim_toggle("MOUSE") {
                        DS_FIRE_RELEASE_REQUEST.store(true, std::sync::atomic::Ordering::Relaxed);
                        DS_AIM_REQUEST.store(0, std::sync::atomic::Ordering::Relaxed);
                    }
                    echo!("[DSaim] right up mode={}", if ds_aim_toggle("MOUSE") { "toggle" } else { "hold" });
                    continue;
                }
                E::ControllerDeviceAdded { which, .. } => {
                    self.controller_added(which);
                    continue;
                }
                E::ControllerDeviceRemoved { which, .. } => {
                    self.controller_removed(which);
                    continue;
                }
                E::ControllerButtonUp { button, .. }
                | E::ControllerButtonDown { button, .. }
                    if ds_frontend_pad_action(button).is_some()
                        && !(button == sdl2::controller::Button::Start
                            && crate::ds_frontend_controller::pause_menu())
                        && (crate::ds_frontend_controller::capture_menu()
                            || crate::ds_frontend_controller::capture_scene_dialog()) =>
                {
                    let semantic_down = matches!(event, E::ControllerButtonDown { .. });
                    let action = ds_frontend_pad_action(button).unwrap();
                    let menu = crate::ds_frontend_controller::capture_menu();
                    if menu && action == crate::ds_frontend_controller::Action::Confirm
                        && crate::ds_frontend_controller::menu_cursor().is_none()
                        && self.ds_gamepad_cursor_mode {
                        // Free pointer mode keeps the original touch hit test,
                        // including sliders and other nested option controls.
                        controller_updated = true;
                        continue;
                    }
                    let scene = crate::ds_frontend_controller::capture_scene_dialog()
                        && matches!(
                            action,
                            crate::ds_frontend_controller::Action::Confirm
                                | crate::ds_frontend_controller::Action::Cancel
                        );
                    if semantic_down {
                        let point = if menu {
                            crate::ds_frontend_controller::queue_menu_action(action);
                            None::<crate::ds_frontend_controller::Point>
                        } else if scene {
                            crate::ds_frontend_controller::queue_scene_action(action);
                            None
                        } else {
                            None
                        };
                        if let Some(point) = point {
                            echo!(
                                "[DSfrontendInput] pad {:?} -> tap ({:.0},{:.0})",
                                action,
                                point.x,
                                point.y
                            );
                            self.ds_queue_action(format!("tap {} {}", point.x, point.y));
                        }
                    }
                    continue;
                }
                // Note that accelerometer simulation with analog sticks is
                // handled with polling, rather than being event-based.
                E::ControllerButtonUp { button, .. } | E::ControllerButtonDown { button, .. } => {
                    controller_updated = true;
                    let semantic_down = matches!(event, E::ControllerButtonDown { .. });
                    // Keep the physical press that resumed the game from
                    // becoming a new pause toggle when its release arrives
                    // after the native HUD has already switched back to
                    // gameplay. This is most visible with Start, but the
                    // same guard also makes Back/B deterministic.
                    if self.ds_gamepad_resume_latched == Some(button) {
                        if !semantic_down {
                            self.ds_gamepad_resume_latched = None;
                        }
                        continue;
                    }
                    if button == sdl2::controller::Button::Start {
                        if semantic_down {
                            self.ds_gamepad_start_down = Some(Instant::now());
                            self.ds_gamepad_start_long = false;
                            if crate::ds_frontend_controller::pause_menu() {
                                self.ds_gamepad_resume_latched = Some(button);
                                crate::ds_frontend_controller::request_native_resume();
                                echo!("[DSpad] Start down -> native resume");
                            }
                        } else if self.ds_gamepad_start_down.take().is_some() {
                            let long = self.ds_gamepad_start_long;
                            self.ds_gamepad_start_long = false;
                            if !long && !self.ds_gamepad_cursor_mode {
                                if crate::ds_frontend_controller::pause_menu() {
                                    crate::ds_frontend_controller::request_native_resume();
                                    echo!("[DSpad] Start short-press -> native resume");
                                } else {
                                    ds_queue_hud(ds_hud_commands::HudCommand::Pause);
                                    echo!("[DSpad] Start short-press -> pause");
                                }
                            }
                        }
                        continue;
                    }
                    if button == sdl2::controller::Button::Back
                        && crate::ds_frontend_controller::pause_menu()
                    {
                        if semantic_down {
                            self.ds_gamepad_resume_latched = Some(button);
                            crate::ds_frontend_controller::request_native_resume();
                            echo!("[DSpad] Back down -> native resume");
                        }
                        continue;
                    }
                    // B is a convenient XInput resume key while the native
                    // pause page owns focus. It remains kinesis in gameplay.
                    if button == sdl2::controller::Button::B
                        && semantic_down
                        && crate::ds_frontend_controller::pause_menu()
                    {
                        self.ds_gamepad_resume_latched = Some(button);
                        crate::ds_frontend_controller::request_native_resume();
                        echo!("[DSpad] B -> native resume");
                        continue;
                    }
                    if crate::ds_frontend_controller::capture_menu()
                        || crate::ds_frontend_controller::capture_scene_dialog() { continue; }
                    if button == sdl2::controller::Button::Back && ds_native_hud_enabled() {
                        if semantic_down && !self.ds_gamepad_cursor_mode { ds_queue_hud(ds_hud_commands::HudCommand::Pause); }
                        continue;
                    }
                    if self.ds_gamepad_cursor_mode {
                        if button == sdl2::controller::Button::B && semantic_down {
                            self.ds_gamepad_cursor_mode = false;
                            echo!("[DSpad] B -> gamepad cursor OFF");
                            continue;
                        }
                        if button == sdl2::controller::Button::A {
                            // `update_virtual_cursor` polls A as the virtual
                            // click; suppress its gameplay E/QTE semantic.
                            continue;
                        }
                        // Cursor mode owns the controller until Start/B exits.
                        // Never leak X/Y/LB/RB/DPad into gameplay actions.
                        continue;
                    }
                    // Stick clicks own native actions in gameplay; cursor mode
                    // above consumes them without leaking through an open UI.
                    if matches!(button, sdl2::controller::Button::LeftStick | sdl2::controller::Button::RightStick) {
                        if semantic_down {
                            if button == sdl2::controller::Button::RightStick
                                && self.ds_controller_lt_pressed()
                            {
                                self.ds_start_zero_g_jump("LT+R3");
                                continue;
                            }
                            let request = if button == sdl2::controller::Button::LeftStick { &DS_LOCATOR_REQUEST } else { &DS_QUICK_TURN_REQUEST };
                            request.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        continue;
                    }
                    if ds_native_hud_enabled() {
                        use sdl2::controller::Button as B;
                        let direction=match button {B::DPadUp=>Some(0),B::DPadRight=>Some(1),B::DPadDown=>Some(2),B::DPadLeft=>Some(3),_=>None};
                        if let Some(direction)=direction {
                            if let Some(command)=self.pad_weapons.update(direction,semantic_down,ds_weapon_ms()) {ds_queue_hud(command);}
                            continue;
                        }
                    }
                    let Some(button) = translate_button(button) else {
                        continue;
                    };
                    if ds_gamepad_semantic() {
                        let handled = match button {
                            crate::options::Button::A => {
                                if semantic_down {
                                    crate::window::DS_EKEY_INTERACT
                                        .store(true, std::sync::atomic::Ordering::Relaxed);
                                    echo!("[DSpad] A -> interact/QTE");
                                }
                                true
                            }
                            crate::options::Button::Y => {
                                if semantic_down {
                                    crate::window::DS_CKEY_STASIS
                                        .store(true, std::sync::atomic::Ordering::Relaxed);
                                    echo!("[DSpad] Y -> stasis");
                                }
                                true
                            }
                            crate::options::Button::X => {
                                if semantic_down {
                                    crate::window::DS_RKEY_RELOAD
                                        .store(true, std::sync::atomic::Ordering::Relaxed);
                                    echo!("[DSpad] X -> reload 0x0a");
                                }
                                true
                            }
                            crate::options::Button::B => {
                                if semantic_down {
                                    crate::window::DS_DEATH_CANCEL_REQUEST
                                        .store(true, std::sync::atomic::Ordering::Relaxed);
                                    crate::window::DS_FKEY_KINESIS
                                        .store(true, std::sync::atomic::Ordering::Relaxed);
                                    echo!("[DSpad] B -> kinesis");
                                }
                                true
                            }
                            crate::options::Button::LeftShoulder => {
                                if semantic_down {
                                    crate::window::DS_CONTEXT_COMBAT_REQUEST
                                        .store(true, std::sync::atomic::Ordering::Relaxed);
                                    echo!("[DSpad] LB -> melee/stomp by HUD selection");
                                }
                                true
                            }
                            crate::options::Button::RightShoulder => {
                                use ds_fire_control::{Input,Source};
                                if ds_pad_mode_toggle() {
                                    if semantic_down {ds_fire_control::enqueue(Input::Toggle);echo!("[DSpad] RB -> select mode only");}
                                } else if semantic_down && (self.controller_lt_down || (ds_aim_toggle("CONTROLLER")&&DS_AIM_ACTIVE.load(std::sync::atomic::Ordering::Relaxed))) {
                                    self.controller_rb_down=true;ds_fire_control::enqueue(Input::Fire(Source::Primary,Some(false)));
                                    echo!("[DSpad] RB -> select primary, then fire");
                                } else if !semantic_down && self.controller_rb_down {
                                    self.controller_rb_down=false;ds_fire_control::enqueue(Input::Release(Source::Primary));
                                }
                                true
                            }
                            _ => false,
                        };
                        if handled {
                            continue;
                        }
                    }
                    // Called whenever a DPad direction is pressed or released
                    if (button == crate::options::Button::DPadLeft
                        || button == crate::options::Button::DPadUp
                        || button == crate::options::Button::DPadRight
                        || button == crate::options::Button::DPadDown)
                        && options.dpad_to_touch.is_some()
                    {
                        let Some((x, y, w, h)) = options.dpad_to_touch else {
                            // We already checked dpad_to_touch.is_some() in
                            // the surrounding if-condition, but be defensive
                            // against future refactors and bail out instead
                            // of panicking the host.
                            log!(
                                "Warning: dpad_to_touch became None between outer check and inner destructure; ignoring D-pad event."
                            );
                            continue;
                        };

                        // Update held state
                        let pressed = matches!(event, E::ControllerButtonDown { .. });
                        let direction = match button {
                            crate::options::Button::DPadUp=>0,crate::options::Button::DPadLeft=>1,
                            crate::options::Button::DPadDown=>2,crate::options::Button::DPadRight=>3,
                            _=>unreachable!("non-direction D-pad button")
                        };
                        self.pad_directions.update(direction,pressed);
                        match button {
                            crate::options::Button::DPadLeft => self.dpad_state.left = pressed,
                            crate::options::Button::DPadRight => self.dpad_state.right = pressed,
                            crate::options::Button::DPadUp => self.dpad_state.up = pressed,
                            crate::options::Button::DPadDown => self.dpad_state.down = pressed,
                            _ => {
                                // The outer `if` already restricts us to the
                                // four D-pad buttons; if a new variant slips
                                // through after a refactor, just ignore it
                                // instead of crashing.
                                log!(
                                    "Warning: unexpected button {:?} reached D-pad arm; ignoring.",
                                    button
                                );
                                continue;
                            }
                        }

                        // Compute center
                        let cx = x + w * 0.5;
                        let cy = y + h * 0.5;

                        // Compute combined delta
                        let mut dx = 0.0;
                        if ds_native_inputs() { continue; }
                        let mut dy = 0.0;

                        if self.dpad_state.left {
                            dx -= 0.5 * w;
                        }
                        if self.dpad_state.right {
                            dx += 0.5 * w;
                        }
                        if self.dpad_state.up {
                            dy -= 0.5 * h;
                        }
                        if self.dpad_state.down {
                            dy += 0.5 * h;
                        }

                        // Final coords: center + movement
                        let coords = transform_input_coords(self, (cx + dx, cy + dy), true);

                        // Send TouchDown if any dpad is held, TouchUp if none
                        if ds_native_inputs() { continue; }
                        let any_held = self.dpad_state.left
                            || self.dpad_state.right
                            || self.dpad_state.up
                            || self.dpad_state.down;

                        if !self.dpad_state.active && any_held {
                            // New touch
                            self.dpad_state.active = true;
                            Event::TouchesDown(HashMap::from([(FingerId::DpadToTouch, coords)]))
                        } else if self.dpad_state.active && any_held {
                            // Move existing touch
                            Event::TouchesMove(HashMap::from([(FingerId::DpadToTouch, coords)]))
                        } else if self.dpad_state.active && !any_held {
                            // Release touch
                            self.dpad_state.active = false;
                            Event::TouchesUp(HashMap::from([(FingerId::DpadToTouch, coords)]))
                        } else {
                            continue;
                        }
                    } else {
                        let Some(&(x, y)) = options.button_to_touch.get(&button) else {
                            continue;
                        };
                        match event {
                            E::ControllerButtonUp { .. } => {
                                let coords = transform_input_coords(self, (x, y), true);
                                Event::TouchesUp(HashMap::from([(
                                    FingerId::ButtonToTouch(button),
                                    coords,
                                )]))
                            }
                            E::ControllerButtonDown { .. } => {
                                let coords = transform_input_coords(self, (x, y), true);
                                Event::TouchesDown(HashMap::from([(
                                    FingerId::ButtonToTouch(button),
                                    coords,
                                )]))
                            }
                            _ => {
                                // Outer arm only matches ControllerButton{Up,Down}.
                                log!(
                                    "Warning: unexpected event {:?} in button-to-touch arm; ignoring.",
                                    event
                                );
                                continue;
                            }
                        }
                    }
                }
                // Dead Space: WASD / arrow keys drive the same synthetic
                // virtual stick as a gamepad, so the game is playable with a
                // keyboard (needs --stick-to-touch to be configured).
                E::KeyDown {
                    keycode: Some(k),
                    repeat: false,
                    ..
                } if matches!(
                    k,
                    sdl2::keyboard::Keycode::W
                        | sdl2::keyboard::Keycode::A
                        | sdl2::keyboard::Keycode::S
                        | sdl2::keyboard::Keycode::D
                        | sdl2::keyboard::Keycode::Up
                        | sdl2::keyboard::Keycode::Down
                        | sdl2::keyboard::Keycode::Left
                        | sdl2::keyboard::Keycode::Right
                ) =>
                {
                    let (sx, sy) = self.key_directions.update(ds_direction_key(k),true);
                    self.key_stick = (sx, sy);
                    // Which key produced which stick vector - added while
                    // chasing "W produced a leftward stick target"
                    // (2026-10-11, unattended WASD verification).
                    log!("[DSstick] key {:?} -> stick ({:.1},{:.1})", k, sx, sy);
                    if ds_native_inputs() { continue; }
                    if let Some((x, y, w, h)) = options.stick_to_touch {
                        // Push as far as the *working* mouse gesture does.
                        //
                        // The rect edge is only half the deflection a real drag
                        // uses (drag from the rect centre to 115 points away vs
                        // 55 to the edge) and Dead Space ignores a stick that is
                        // not pushed far enough: measured 2026-10-10, holding W
                        // produced the touch at the rect edge and the character
                        // never moved, while an otherwise identical drag of twice
                        // that distance walked him down the chapter-12 corridor.
                        // `TOUCHHLE_DS_KEY_STICK_GAIN=1` restores the old reach.
                        let gain: f32 = std::env::var("TOUCHHLE_DS_KEY_STICK_GAIN")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(2.0);
                        let (cx, cy) = (x + w * 0.5, y + h * 0.5);
                        let coords = transform_input_coords(
                            self,
                            (cx + sx * w * 0.5 * gain, cy + sy * h * 0.5 * gain),
                            true,
                        );
                        if sx.abs() < options.deadzone && sy.abs() < options.deadzone {
                            continue;
                        }
                        if !self.stick_active {
                            self.stick_active = true;
                            // The game`s on-screen stick is *origin relative*: the point where
                            // the finger lands becomes the stick centre, so a touch that simply
                            // APPEARS already deflected produces no movement at all.
                            // Measured 2026-10-10: sending TouchesDown at the deflected position
                            // (and even Down+Move in the same frame) left the character rooted to
                            // the spot, while a real mouse drag from the stick centre out to the
                            // deflected position - which spans many frames - walked him the whole
                            // way down the chapter-12 corridor. So: land on the centre, then
                            // spread the move out over the next few frames.
                            let centre =
                                transform_input_coords(self, (x + w * 0.5, y + h * 0.5), true);
                            echo!("[DSstick] down touch=({:.1},{:.1}) target=({:.1},{:.1}) viewport={:?}", centre.0, centre.1, coords.0, coords.1, self.viewport());
                            // The finger id must match the id the slide's
                            // `rawmove`/`rawup` steps use, which is the script
                            // executor's `FingerId::Touch(-1)`. It used to be
                            // `StickToTouch` here while the slide went out as
                            // `Touch(-1)`, so the moves addressed a finger that
                            // did not exist and the stick never actually left the
                            // centre - measured 2026-10-11 in the VM: the mouse
                            // drag walked the character, held W/A/D did nothing.
                            self.event_queue
                                .push_back(Event::TouchesDown(HashMap::from([(
                                    FingerId::Touch(-1),
                                    centre,
                                )])));
                            self.key_stick_last = centre;
                            // Then drag out to the deflected position over the next few
                            // frames. A Down+Move in the SAME frame is not treated as a
                            // drag by the game (`measured 2026-10-10`): the character stayed
                            // rooted. A real mouse drag, which spans many frames, walked him
                            // across the corridor with the very same start/end points, so
                            // spread the movement over frames like the `swipe` action does.
                            let at = DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
                            // Slide out over more frames than a flick: the game
                            // reads a *fast* deflection as a flick/turn gesture
                            // (user: "快速点击变为转身"), so the push has to look
                            // like a finger being pulled out. 8 frames ≈ 0.25 s at
                            // 30 fps; `TOUCHHLE_DS_STICK_SLIDE=<frames>` tunes it.
                            let STEPS: u64 = std::env::var("TOUCHHLE_DS_STICK_SLIDE")
                                .ok()
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(8)
                                .max(1);
                            for step in 1..=STEPS {
                                let t = step as f32 / STEPS as f32;
                                self.ds_script.push((
                                    at + step,
                                    format!(
                                        "rawmove {} {}",
                                        centre.0 + (coords.0 - centre.0) * t,
                                        centre.1 + (coords.1 - centre.1) * t
                                    ),
                                ));
                            }
                            continue;
                        } else {
                            Event::TouchesMove(HashMap::from([(FingerId::Touch(-1), coords)]))
                        }
                    } else {
                        continue;
                    }
                }
                E::KeyUp {
                    keycode: Some(k), ..
                } if matches!(
                    k,
                    sdl2::keyboard::Keycode::W
                        | sdl2::keyboard::Keycode::A
                        | sdl2::keyboard::Keycode::S
                        | sdl2::keyboard::Keycode::D
                        | sdl2::keyboard::Keycode::Up
                        | sdl2::keyboard::Keycode::Down
                        | sdl2::keyboard::Keycode::Left
                        | sdl2::keyboard::Keycode::Right
                ) =>
                {
                    let (sx, sy) = self.key_directions.update(ds_direction_key(k),false);
                    self.key_stick = (sx, sy);
                    log!("[DSstick] release {:?} -> stick ({:.1},{:.1})", k, sx, sy);
                    if ds_native_inputs() { continue; }
                    if let Some((x, y, w, h)) = options.stick_to_touch {
                        // Same reach as the KeyDown arm (see the comment there);
                        // with every key released sx/sy are 0, so this is the
                        // rect centre - i.e. the touch lifts at the stick origin.
                        let gain: f32 = std::env::var("TOUCHHLE_DS_KEY_STICK_GAIN")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(2.0);
                        let (cx, cy) = (x + w * 0.5, y + h * 0.5);
                        let coords = transform_input_coords(
                            self,
                            (cx + sx * w * 0.5 * gain, cy + sy * h * 0.5 * gain),
                            true,
                        );
                        if sx.abs() < options.deadzone && sy.abs() < options.deadzone {
                            self.stick_active = false;
                            // Slide the finger back to the stick origin instead of
                            // lifting it where it happens to be: the release then
                            // looks like a drag returning home (the same reason the
                            // push is ramped), and a quick tap becomes a short
                            // nudge rather than an instant flick/turn.
                            let last = self.key_stick_last;
                            let at = DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
                            const BACK: u64 = 4;
                            for step in 1..=BACK {
                                let t = step as f32 / BACK as f32;
                                self.ds_script.push((
                                    at + step,
                                    format!(
                                        "rawmove {} {}",
                                        last.0 + (coords.0 - last.0) * t,
                                        last.1 + (coords.1 - last.1) * t
                                    ),
                                ));
                            }
                            self.ds_script
                                .push((at + BACK + 1, format!("rawup {} {}", coords.0, coords.1)));
                            self.key_stick_last = coords;
                            continue;
                        } else {
                            // Direction changed while held (e.g. W then W+A): slide
                            // to the new position instead of teleporting.
                            let last = self.key_stick_last;
                            let at = DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
                            const TURN: u64 = 4;
                            for step in 1..=TURN {
                                let t = step as f32 / TURN as f32;
                                self.ds_script.push((
                                    at + step,
                                    format!(
                                        "rawmove {} {}",
                                        last.0 + (coords.0 - last.0) * t,
                                        last.1 + (coords.1 - last.1) * t
                                    ),
                                ));
                            }
                            self.key_stick_last = coords;
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                E::ControllerAxisMotion { axis, value, .. } => {
                    let scene_dialog = crate::ds_frontend_controller::capture_scene_dialog();
                    controller_updated = true;
                    let norm = (value as f32 / 32768.0).clamp(-1.0, 1.0);
                    if axis == sdl2::controller::Axis::RightX
                        || axis == sdl2::controller::Axis::RightY
                    {
                        if self.ds_cursor_mode_active() {
                            // In cursor mode right stick owns the pointer only;
                            // it must not also enqueue a camera-look gesture.
                            continue;
                        }
                        if ds_native_inputs() { continue; }
                        let deadzone: f32 = std::env::var("TOUCHHLE_DS_CONTROLLER_LOOK_DEADZONE")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0.14);
                        if norm.abs() >= deadzone {
                            let gain: f32 = std::env::var("TOUCHHLE_DS_CONTROLLER_LOOK_GAIN")
                                .ok()
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(18.0);
                            let invert = std::env::var("TOUCHHLE_DS_CONTROLLER_LOOK_INVERT").unwrap_or_default();
                            if axis == sdl2::controller::Axis::RightX {
                                self.ds_look_accum.0 += norm * gain * if invert == "x" || invert == "both" { -1.0 } else { 1.0 };
                            } else {
                                self.ds_look_accum.1 += norm * gain * if invert == "y" || invert == "both" { -1.0 } else { 1.0 };
                            }
                            self.controller_look_pending = true;
                        }
                        continue;
                    }
                    if axis == sdl2::controller::Axis::TriggerLeft {
                        if self.ds_gamepad_cursor_mode {
                            continue;
                        }
                        if value > 16000 && !self.controller_lt_down {
                            self.controller_lt_physical_down = true;
                            DS_CONTROLLER_LT_HELD.store(true, std::sync::atomic::Ordering::Relaxed);
                            if !scene_dialog {
                                self.controller_lt_down = true;
                                self.ds_aim_active = true;
                                crate::window::DS_AIM_REQUEST
                                    .store(if ds_aim_toggle("CONTROLLER") { 2 } else { 1 }, std::sync::atomic::Ordering::Relaxed);
                                if ds_aim_toggle("CONTROLLER") { DS_FIRE_RELEASE_REQUEST.store(true, std::sync::atomic::Ordering::Relaxed); }
                                echo!("[DSpad] LT down -> native aim mode={}", if ds_aim_toggle("CONTROLLER") { "toggle" } else { "hold" });
                            } else {
                                echo!("[DSpad] LT down retained during native control");
                            }
                        } else if value < 6000 && (self.controller_lt_physical_down || self.controller_lt_down) {
                            let logical_down = self.controller_lt_down;
                            let release = ds_take_aim_release(
                                &mut self.controller_lt_physical_down,
                                self.controller_lt_down,
                            );
                            self.controller_lt_down = false;
                            self.ds_aim_active = false;
                            DS_CONTROLLER_LT_HELD.store(false, std::sync::atomic::Ordering::Relaxed);
                            if release && logical_down && !ds_aim_toggle("CONTROLLER") {
                                if self.controller_rt_down || self.controller_rb_down {
                                    crate::window::DS_FIRE_RELEASE_REQUEST.store(true, std::sync::atomic::Ordering::Relaxed);
                                }
                                crate::window::DS_AIM_REQUEST.store(0, std::sync::atomic::Ordering::Relaxed);
                            }
                            echo!("[DSpad] LT up mode={} logical={}", if ds_aim_toggle("CONTROLLER") { "toggle" } else { "hold" }, logical_down);
                        }
                        continue;
                    }
                    if scene_dialog { continue; }
                    if axis == sdl2::controller::Axis::TriggerRight {
                        if self.ds_gamepad_cursor_mode {
                            continue;
                        }
                        if value > 16000 && !self.controller_rt_down {
                            self.controller_rt_down = true;
                            DS_RT_KINESIS_LAUNCH_REQUEST.store(true, std::sync::atomic::Ordering::Relaxed);
                            if self.controller_lt_down || (ds_aim_toggle("CONTROLLER") && DS_AIM_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)) {
                                ds_fire_control::enqueue(ds_fire_control::Input::Fire(ds_fire_control::Source::Secondary,
                                    if ds_pad_mode_toggle(){None}else{Some(true)}));
                                echo!("[DSpad] RT -> {}",if ds_pad_mode_toggle(){"fire selected mode"}else{"select secondary, then fire"});
                            } else {
                                echo!("[DSpad] RT ignored: LT is not held");
                            }
                        } else if value < 6000 && self.controller_rt_down {
                            self.controller_rt_down = false;
                            ds_fire_control::enqueue(ds_fire_control::Input::Release(ds_fire_control::Source::Secondary));
                            echo!("[DSpad] RT up");
                        }
                        continue;
                    }
                    if ds_native_inputs() { continue; }
                    let Some((x, y, w, h)) = options.stick_to_touch else {
                        continue;
                    };
                    if axis == sdl2::controller::Axis::LeftX
                        || axis == sdl2::controller::Axis::LeftY
                    {
                        let (stick_x, stick_y, _) = self.get_controller_stick(options, true);
                        let coords = transform_input_coords(
                            self,
                            (
                                x + ((stick_x + 1.0) / 2.0) * w,
                                y + ((stick_y + 1.0) / 2.0) * h,
                            ),
                            true,
                        );
                        if stick_x.abs() < options.deadzone && stick_y.abs() < options.deadzone {
                            if !self.stick_active {
                                // Ignore deadzone events when stick is inactive
                                continue;
                            } else {
                                // Release touch when stick returns to deadzone
                                self.stick_active = false;
                                Event::TouchesUp(HashMap::from([(FingerId::StickToTouch, coords)]))
                            }
                        } else if !self.stick_active {
                            // New touch
                            self.stick_active = true;
                            Event::TouchesDown(HashMap::from([(FingerId::StickToTouch, coords)]))
                        } else {
                            // Move existing touch
                            Event::TouchesMove(HashMap::from([(FingerId::StickToTouch, coords)]))
                        }
                    } else {
                        continue;
                    }
                }
                E::AppWillEnterBackground { .. } => {
                    log!("Received app-will-resign-active event.");
                    if self.high_priority_event.is_none() {
                        self.high_priority_event = Some(Event::AppWillResignActive);
                    }
                    self.backgrounded = true;
                    continue;
                }
                E::AppDidEnterBackground { .. } => Event::AppDidEnterBackground,
                E::AppWillEnterForeground { .. } => {
                    log!("Received app-will-enter-foreground event.");
                    self.backgrounded = false;
                    Event::AppWillEnterForeground
                }
                E::AppDidEnterForeground { .. } => {
                    log!("Received app-did-enter-foreground event.");
                    self.backgrounded = false;
                    Event::AppDidBecomeActive
                }
                E::AppTerminating { .. } => {
                    log!("Received app-will-terminate event.");
                    // Android may deliver termination immediately after a
                    // resign-active event that has not been consumed yet.
                    // Termination supersedes that pending lifecycle event;
                    // asserting here aborts the whole process on a normal
                    // background/ANR teardown sequence.
                    if let Some(previous) = self.high_priority_event.take() {
                        log!(
                            "App termination supersedes pending lifecycle event {:?}.",
                            previous
                        );
                    }
                    self.high_priority_event = Some(Event::AppWillTerminate);
                    self.enable_event_polling = false;
                    continue;
                }
                E::FingerUp {
                    timestamp,
                    finger_id,
                    x,
                    y,
                    ..
                }
                | E::FingerMotion {
                    timestamp,
                    finger_id,
                    x,
                    y,
                    ..
                }
                | E::FingerDown {
                    timestamp,
                    finger_id,
                    x,
                    y,
                    ..
                } => {
                    log_dbg!("Starting multi-touch for {:?}", event);
                    // To implement multi-touch we accumulate here same touch
                    // events at the same timestamp. This is consistent with
                    // UIKit, but could be broken if events come out of order.
                    // (in worst case we separate multi-touches in several ones)
                    // TODO: handle out of order touches
                    let curr_timestamp = timestamp;
                    let abs_coords = finger_absolute_coords(self, (x, y));
                    let coords = transform_input_coords(self, abs_coords, false);
                    log_dbg!("Finger event x {}, y {}, coords {:?}", x, y, coords);
                    let mut map = HashMap::from([(FingerId::Touch(finger_id), coords)]);
                    while let Some(next) = self.event_pump.poll_event() {
                        match next {
                            E::Unknown { .. } => (),
                            _ => log_dbg!("Next possible multi-touch event: {:?}", next),
                        }
                        match next {
                            E::FingerUp {
                                timestamp,
                                finger_id,
                                x,
                                y,
                                ..
                            }
                            | E::FingerMotion {
                                timestamp,
                                finger_id,
                                x,
                                y,
                                ..
                            }
                            | E::FingerDown {
                                timestamp,
                                finger_id,
                                x,
                                y,
                                ..
                            } if timestamp == curr_timestamp && next.is_same_kind_as(&event) => {
                                let abs_coords = finger_absolute_coords(self, (x, y));
                                let coords = transform_input_coords(self, abs_coords, false);
                                map.insert(FingerId::Touch(finger_id), coords);
                            }
                            E::MultiGesture { timestamp, .. } if timestamp == curr_timestamp => {
                                // TODO: handle gestures
                                continue;
                            }
                            _ => {
                                // event_pump doesn't have a method to peek on
                                // events, so we keep track of an unconsumed
                                // one from a previous loop iteration
                                assert!(previous_event.is_none());
                                previous_event = Some(next);
                                break;
                            }
                        }
                    }
                    log_dbg!("Finishing multi-touch for {:?} with {:?}", event, map);
                    match event {
                        E::FingerUp { .. } => Event::TouchesUp(map),
                        E::FingerMotion { .. } => Event::TouchesMove(map),
                        E::FingerDown { .. } => Event::TouchesDown(map),
                        _ => {
                            // Outer arm matches FingerUp/Motion/Down only.
                            log!(
                                "Warning: unexpected event {:?} in multi-touch arm; treating as TouchesMove.",
                                event
                            );
                            Event::TouchesMove(map)
                        }
                    }
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F8),
                    repeat: false,
                    ..
                } if ds_look_mode() != DsLookMode::Off && ds_release_key_enabled("F8") => {
                    // F8 is the always-available look/click mode switch (the
                    // right mouse button does it too, unless PC-style aiming or
                    // `TOUCHHLE_DS_MOUSE_LOOK_RMB=0` claims it).
                    self.ds_mouse_capture.forced_off = !self.ds_mouse_capture.forced_off;
                    if self.ds_mouse_capture.forced_off { self.ds_cancel_inputs(); }
                    echo!(
                        "[DSlook] F8 -> {} mode ({})",
                        if self.ds_mouse_capture.forced_off {
                            "CLICK"
                        } else {
                            "LOOK"
                        },
                        if self.ds_mouse_capture.forced_off {
                            "pointer is absolute again: click the UI normally"
                        } else {
                            "mouse captured: move to turn the camera"
                        }
                    );
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::LAlt),
                    repeat: false,
                    ..
                }
                | E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::RAlt),
                    repeat: false,
                    ..
                } if ds_look_mode() != DsLookMode::Off && ds_release_key_enabled("ALT") => {
                    self.ds_cancel_inputs();
                    self.ds_alt_released = true;
                    self.ds_look_accum = (0.0, 0.0);
                    self.controller_look_pending = false;
                    self.ds_mouse_capture.captured = false;
                    self.ds_set_mouse_capture(false, "Alt held");
                    echo!("[DSlook] Alt held -> temporary CLICK mode");
                    continue;
                }
                E::KeyUp {
                    keycode: Some(sdl2::keyboard::Keycode::LAlt),
                    ..
                }
                | E::KeyUp {
                    keycode: Some(sdl2::keyboard::Keycode::RAlt),
                    ..
                } if self.ds_alt_released => {
                    self.ds_alt_released = false;
                    self.ds_look_accum = (0.0, 0.0);
                    echo!("[DSlook] Alt released -> restoring LOOK capture");
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::LGui),
                    repeat: false,
                    ..
                }
                | E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::RGui),
                    repeat: false,
                    ..
                } if ds_look_mode() != DsLookMode::Off && ds_release_key_enabled("WIN") => {
                    // Win only suspends capture. Returning to the game restores
                    // capture automatically; it must not become a permanent
                    // click-mode toggle.
                    self.ds_mouse_capture.recapture_on_focus = self.ds_mouse_capture.captured;
                    self.ds_cancel_inputs();
                    self.ds_capture_suspended = true;
                    self.ds_mouse_capture.captured = false;
                    self.ds_set_mouse_capture(false, "Windows key held");
                    echo!("[DSlook] Win -> mouse temporarily released");
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::E),
                    ..
                } => {
                    if self.ds_e_held
                        || crate::ds_frontend_controller::capture_scene_dialog()
                    {
                        continue;
                    }
                    self.ds_e_held = true;
                    crate::window::DS_EKEY_INTERACT
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    echo!("[DSe] E pressed: native context action queued for the next frame");
                    continue;
                }
                E::KeyUp {
                    keycode: Some(sdl2::keyboard::Keycode::E),
                    ..
                } => {
                    self.ds_e_held = false;
                    continue;
                }
                E::KeyDown { keycode: Some(sdl2::keyboard::Keycode::F7),repeat:false,.. }=>{
                    crate::ds_diagnostics::mark_problem();continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::M),
                    repeat: false,
                    ..
                }
                => {
                    crate::window::DS_MARK.store(true, std::sync::atomic::Ordering::Relaxed);
                    crate::window::DS_TRACE_MARKED
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    echo!("[DSmark] mark: a full-resolution frame will be saved this frame");
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F),
                    repeat: false,
                    ..
                } => {
                    crate::window::DS_FKEY_KINESIS
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    echo!("[DSe] F pressed: will look for the kinesis prompt on the next frame");
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::C),
                    repeat: false,
                    ..
                } => {
                    crate::window::DS_CKEY_STASIS.store(true, std::sync::atomic::Ordering::Relaxed);
                    echo!("[DSe] C pressed: will tap the stasis button on the next frame");
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::X),
                    repeat: false,
                    ..
                } => {
                    crate::window::DS_XKEY_ROTATE.store(true, std::sync::atomic::Ordering::Relaxed);
                    echo!("[DSx] X pressed: plasma cutter rotate queued for the next frame");
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::R),
                    repeat: false,
                    ..
                } => {
                    crate::window::DS_RKEY_RELOAD.store(true, std::sync::atomic::Ordering::Relaxed);
                    echo!("[DSr] R pressed: will look for the ammo box on the next frame");
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::N),
                    repeat: false,
                    ..
                } => {
                    crate::window::DS_PROBE.store(true, std::sync::atomic::Ordering::Relaxed);
                    echo!("[DSgauge] N pressed: will log the player gauges and dump this frame");
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::V),
                    repeat: false,
                    ..
                } => {
                    // V is a direct alias for the tested zero-g gesture.
                    self.ds_start_zero_g_jump("V");
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F8),
                    repeat: false,
                    ..
                } => {
                    // Toggle the touch-input recorder (TAS-lite). Recording is
                    // what makes "let me just demonstrate it to you" work: the
                    // script can be replayed in the VM as-is.
                    if self.ds_record_file.is_some() {
                        self.ds_record_file = None;
                        self.ds_record_enabled = false;
                        echo!("[DSrecord] recording STOPPED (file closed)");
                    } else {
                        match std::env::var("TOUCHHLE_DS_RECORD") {
                            Ok(p) => {
                                self.ds_record_enabled = true;
                                echo!(
                                    "[DSrecord] recording ARMED -> {} (starts at the next touch)",
                                    p
                                );
                            }
                            Err(_) => {
                                echo!("[DSrecord] no TOUCHHLE_DS_RECORD set; nothing to toggle")
                            }
                        }
                    }
                    continue;
                }
                E::KeyUp {keycode:Some(sdl2::keyboard::Keycode::Space),..}=>{
                    if self.ds_space_held && ds_keyboard_alt_hold(){
                        if ds_native_inputs(){ds_fire_control::enqueue(ds_fire_control::Input::Hold(false));}
                        else{self.virtual_accelerometer_last=Some((0.8,0.0,true));}
                    }
                    self.ds_space_held=false;continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::LCtrl),
                    repeat: false,
                    ..
                } => {
                    self.ds_start_zero_g_jump("Ctrl");
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::Space),
                    keymod,
                    repeat: false,
                    ..
                } => {
                    if keymod
                        .intersects(sdl2::keyboard::Mod::LSHIFTMOD | sdl2::keyboard::Mod::RSHIFTMOD)
                    {
                        self.ds_start_zero_g_jump("Shift+Space");
                    } else {
                        if crate::ds_frontend_controller::capture_menu() || crate::ds_frontend_controller::capture_scene_dialog(){continue;}
                        self.ds_space_held=true;
                        if ds_native_inputs(){
                            ds_fire_control::enqueue(if ds_keyboard_alt_hold(){ds_fire_control::Input::Hold(true)}else{ds_fire_control::Input::Toggle});
                            echo!("[DSpc] Space -> {} mode selection",if ds_keyboard_alt_hold(){"hold secondary"}else{"toggle"});
                        } else {
                            let cur=self.virtual_accelerometer_last.map(|v|v.0).unwrap_or(0.8);
                            let nx=if ds_keyboard_alt_hold(){-0.8}else if cur>=0.0{-0.8}else{0.8};
                            self.virtual_accelerometer_last=Some((nx,0.0,true));
                        }
                    }
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F6),
                    keymod,
                    repeat: false,
                    ..
                } if keymod.intersects(
                    sdl2::keyboard::Mod::LSHIFTMOD | sdl2::keyboard::Mod::RSHIFTMOD,
                ) =>
                {
                    let m = crate::gles::gles1_on_gl2::ds_z_mode_cycle();
                    echo!(
                        "[DSz] frame {} z-fix mode -> {} ({})",
                        DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),
                        m,
                        crate::gles::gles1_on_gl2::ds_z_mode_name(m)
                    );
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F6),
                    repeat: false,
                    ..
                } => {
                    DS_PROBE.store(true,std::sync::atomic::Ordering::Relaxed);
                    echo!(
                        "[DSverify] checkpoint frame={}",
                        DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed)
                    );
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F5),
                    repeat: false,
                    ..
                } => {
                    // Save-state. Originally Shift+F5, but a synthetic Shift
                    // (e.g. ControlSend) does not reliably reach SDL as a
                    // keymod, so accept bare F5 as well. F5/F9 are unused by
                    // the guest and by any other shortcut in this tree.
                    echo!("F5 (or Shift+F5) pressed, SaveState event queued.");
                    Event::SaveState
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F9),
                    repeat: false,
                    ..
                } => {
                    echo!("F9 (or Shift+F9) pressed, LoadState event queued.");
                    Event::LoadState
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F11),
                    repeat: false,
                    ..
                } => {
                    self.toggle_fullscreen();
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::Return),
                    keymod,
                    repeat: false,
                    ..
                } if keymod
                    .intersects(sdl2::keyboard::Mod::LALTMOD | sdl2::keyboard::Mod::RALTMOD) =>
                {
                    self.toggle_fullscreen();
                    continue;
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::F12),
                    ..
                } => {
                    // Log this so you can tell when touchHLE has received
                    // the event but it's stuck in the queue.
                    echo!("F12 pressed, EnterDebugger event queued.");
                    Event::EnterDebugger
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::Backspace),
                    ..
                } => {
                    log_dbg!("SDL TextInput Backspace");
                    Event::TextInput(TextInputEvent::Backspace)
                }
                E::KeyDown {
                    keycode: Some(sdl2::keyboard::Keycode::Return),
                    ..
                } => {
                    log_dbg!("SDL TextInput Return");
                    Event::TextInput(TextInputEvent::Return)
                }
                E::TextInput { text, .. } => {
                    log_dbg!("SDL TextInput {}", text);
                    Event::TextInput(TextInputEvent::Text(text))
                }
                _ => continue,
            };
            self.record_touch_event(&produced);
            self.event_queue.push_back(produced);
        }

        // Sample native movement/look after SDL events have been consumed.
        // The old order sampled once before the event loop, so a right-stick
        // or LT change could not reach the guest until the next poll. Keeping
        // this after the loop makes controller aim/look respond in the same
        // input turn while preserving the single native sample per poll.
        self.ds_refresh_controller_lt_state();
        if ds_native_inputs() {
            let focused = unsafe { sdl2_sys::SDL_GetKeyboardFocus() == self.window.raw() };
            self.ds_sample_native_inputs(options, focused);
        }

        if (controller_updated && !ds_native_inputs()) || self.ds_cursor_mode_active() {
            let (new_x, new_y, pressed, pressed_changed, moved) =
                self.update_virtual_cursor(options);
            self.event_queue
                .push_back(match (pressed, pressed_changed, moved) {
                    (true, true, _) => {
                        let coords = transform_input_coords(self, (new_x, new_y), false);
                        Event::TouchesDown(HashMap::from([(FingerId::VirtualCursor, coords)]))
                    }
                    (false, true, _) => {
                        let coords = transform_input_coords(self, (new_x, new_y), false);
                        Event::TouchesUp(HashMap::from([(FingerId::VirtualCursor, coords)]))
                    }
                    (true, _, true) => {
                        let coords = transform_input_coords(self, (new_x, new_y), false);
                        Event::TouchesMove(HashMap::from([(FingerId::VirtualCursor, coords)]))
                    }
                    _ => return,
                });
        }
    }

    /// Stop the guest run loop while Android owns the background. The event
    /// pump remains alive so a real foreground transition can resume the same
    /// process, rather than treating HOME as an implicit app termination.
    fn wait_until_foreground(&mut self) {
        use sdl2::event::Event as E;

        log!("touchHLE suspended in the background; waiting for foreground");
        while self.backgrounded && self.event_queue.is_empty() && self.high_priority_event.is_none()
        {
            match self.event_pump.wait_event_timeout(250) {
                Some(E::AppDidEnterBackground { .. }) => {
                    self.high_priority_event = Some(Event::AppDidEnterBackground);
                }
                Some(E::AppWillEnterForeground { .. }) => {
                    log!("Received app-will-enter-foreground event.");
                    self.backgrounded = false;
                    self.high_priority_event = Some(Event::AppWillEnterForeground);
                }
                Some(E::AppDidEnterForeground { .. }) => {
                    log!("Received app-did-enter-foreground event.");
                    self.backgrounded = false;
                    self.high_priority_event = Some(Event::AppDidBecomeActive);
                }
                Some(E::AppTerminating { .. }) => {
                    log!("Received app-will-terminate event while backgrounded.");
                    self.high_priority_event = Some(Event::AppWillTerminate);
                }
                Some(E::Quit { .. }) => {
                    self.high_priority_event = Some(Event::Quit);
                }
                Some(_) => {}
                None => {}
            }
        }
    }

    /// TAS-lite recorder: `TOUCHHLE_DS_RECORD=<file>` appends every *real* touch
    /// event as `<frame> raw<action> <x> <y>`. The coordinates are already in the
    /// guest's touch space, so `raw*` actions replay without re-applying the
    /// normal input transform. Synthetic fingers (our own replay and the
    /// virtual stick/d-pad) are skipped so a recording stays clean.
    fn record_touch_event(&mut self, ev: &Event) {
        if !self.ds_record_enabled {
            return;
        }
        if self.ds_record_file.is_none() {
            let Ok(path) = std::env::var("TOUCHHLE_DS_RECORD") else {
                return;
            };
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                Ok(f) => {
                    log!("[DSrecord] recording touch input to {}", path);
                    self.ds_record_file = Some(f);
                }
                Err(e) => {
                    log!("[DSrecord] cannot open {}: {}", path, e);
                    return;
                }
            }
        }
        let (action, map) = match ev {
            Event::TouchesDown(m) => ("rawdown", m),
            Event::TouchesMove(m) => ("rawmove", m),
            Event::TouchesUp(m) => ("rawup", m),
            Event::TouchesCancelled(m) => ("rawcancel", m),
            _ => return,
        };
        let frame = DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
        let mut line = String::new();
        for (id, (x, y)) in map.iter() {
            match id {
                FingerId::Mouse | FingerId::VirtualCursor => {}
                FingerId::Touch(n) if *n >= 0 => {}
                _ => continue,
            }
            line.push_str(&format!(
                "{} {} {} {}
",
                frame, action, x, y
            ));
        }
        if line.is_empty() {
            return;
        }
        use std::io::Write;
        if let Some(f) = self.ds_record_file.as_mut() {
            if let Err(e) = f.write_all(line.as_bytes()) {
                log!("[DSrecord] write failed: {}", e);
            }
        }
    }

    fn ds_test_pad_input(&mut self,name:&str,down:Option<bool>,axis:Option<i16>) {
        if !ds_background_test(){echo!("[DScontrol] test pad rejected: explicit hidden inbox mode required");return;}
        if self.ds_test_pad.is_none(){
            sdl2::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS","1");
            match ds_test_pad::TestPad::attach(){Ok(pad)=>{echo!("[DStestpad] attached instance={}",pad.instance);self.ds_test_pad=Some(pad);},Err(e)=>{echo!("[DStestpad] attach failed: {}",e);return;}}
        }
        let pad=self.ds_test_pad.as_mut().unwrap();let result=if let Some(down)=down {pad.button(name,down)}else{pad.axis(name,axis.unwrap_or(0))};
        echo!("[DStestpad] {} down={:?} axis={:?} result={:?}",name,down,axis,result);
    }

    /// Pop an event from the queue (in FIFO order, except for high priority
    /// events)
    /// Queue one DS-script action at the current frame (used by the E key to
    /// tap the interaction ring it just located). Coordinates are landscape
    /// iOS points, same space as `TOUCHHLE_DS_SCRIPT`.
    pub fn ds_queue_action(&mut self, action: String) {
        let at = DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed) + 1;
        self.ds_script.push((at, action));
    }

    /// Native options buttons ignore a one-frame down/up at 60 FPS. Keep
    /// their pointer press for three frames so original repeat/callback logic
    /// sees it. Never overlap a synthetic press with its pending release.
    pub fn ds_queue_ui_tap(&mut self,x:f32,y:f32) {
        if self.ds_script.iter().any(|(_,s)|s.starts_with("rawup ")||s.starts_with("tap ")){return;}
        self.ds_queue_action(format!("tap {x} {y} 3"));
    }

    pub fn pop_event(&mut self) -> Option<Event> {
        let event = self.high_priority_event
            .take()
            .or_else(|| self.event_queue.pop_front());
        match &event {
            Some(Event::TouchesDown(touches)) => self.ds_held_touches.down(touches),
            Some(Event::TouchesMove(touches)) => self.ds_held_touches.moved(touches),
            Some(Event::TouchesUp(touches)) | Some(Event::TouchesCancelled(touches)) => self.ds_held_touches.up(touches),
            _ => {}
        }
        event
    }

    /// Cancel queued synthetic work and release touches already sent to UIKit.
    pub(crate) fn ds_cancel_inputs(&mut self) {
        self.ds_script.clear();
        self.event_queue.retain(|event| !matches!(event, Event::TouchesDown(_) | Event::TouchesMove(_) | Event::TouchesUp(_) | Event::TouchesCancelled(_)));
        // Keep these tracked until TouchesUp is delivered: a second cancellation
        // before pop_event must requeue, rather than discard, the release.
        let mut held = self.ds_held_touches.snapshot();
        if let Some(coords) = held.remove(&FingerId::Look) {
            self.event_queue.push_back(Event::TouchesCancelled(HashMap::from([(FingerId::Look, coords)])));
        }
        if !held.is_empty() { self.event_queue.push_back(Event::TouchesUp(held)); }
        self.key_stick = (0.0, 0.0);
        self.key_directions.clear();
        self.pad_directions.clear();
        self.pad_weapons.clear();
        *DS_WEAPON_COOLDOWN.lock().unwrap()=ds_weapon_input::Cooldown::new();
        self.stick_active = false;
        self.dpad_state = DpadState { left: false, right: false, up: false, down: false, active: false };
        self.ds_aim_active = false;
        self.ds_look_dragging = false;
        self.ds_look_accum = (0.0, 0.0);
        self.controller_look_pending = false;
        self.controller_lt_down = false;
        DS_CONTROLLER_LT_HELD.store(false, std::sync::atomic::Ordering::Relaxed);
        self.controller_rt_down = false;
        self.controller_rb_down = false;
        self.ds_mouse_right_down = false;
        self.controller_lt_physical_down = false;
        self.ds_e_held = false;
        self.ds_space_held=false;
        ds_fire_control::enqueue(ds_fire_control::Input::Cancel);
        self.ds_ui_key_held.clear();
        self.mouse_left_button.cancel();
        self.ds_diag_native_stick=None;
        self.ds_diag_native_look=(0.0,0.0);
        self.ds_gamepad_resume_latched = None;
        self.ds_gamepad_start_down = None;
        self.ds_gamepad_start_long = false;
        *DS_PC_INPUT.lock().unwrap()=ds_pc_input::Input::new();
        self.virtual_cursor_last = None;
        use std::sync::atomic::Ordering::Relaxed;
        DS_AIM_REQUEST.store(0, Relaxed);
        DS_FIRE_RELEASE_REQUEST.store(true, Relaxed);
        DS_FIRE_MODE_REQUEST.store(0, Relaxed);
        DS_CONTEXT_COMBAT_REQUEST.store(false, Relaxed);
        DS_HUD_COMMANDS.lock().unwrap().clear();
        crate::ds_frontend_controller::cancel_loading_input();
        for flag in [&DS_EKEY_INTERACT, &DS_FKEY_KINESIS, &DS_RT_KINESIS_LAUNCH_REQUEST, &DS_MOUSE_KINESIS_LAUNCH_REQUEST, &DS_DEATH_CANCEL_REQUEST, &DS_CKEY_STASIS, &DS_RKEY_RELOAD, &DS_XKEY_ROTATE, &DS_LOCATOR_REQUEST, &DS_QUICK_TURN_REQUEST] { flag.store(false, Relaxed); }
    }

    /// Native control/QTE owns the player for a while, but a hold-to-aim LT
    /// edge still belongs to the physical controller. Preserve that edge when
    /// cancelling gameplay touches so a release or a held press observed during
    /// the modal can be reconciled after the player object returns.
    fn ds_cancel_inputs_for_ui(&mut self) {
        let lt_physical = self.controller_lt_physical_down
            || self
                .controllers
                .iter()
                .any(|controller| controller.axis(sdl2::controller::Axis::TriggerLeft) > 16000);
        self.ds_cancel_inputs();
        if lt_physical {
            self.controller_lt_physical_down = true;
            DS_CONTROLLER_LT_HELD.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// SDL can omit a trigger edge while a native control/QTE owns the input
    /// loop. Re-read the physical LT state when that modal releases ownership
    /// so hold-to-aim cannot remain latched after the player let go.
    fn ds_reconcile_aim_after_ui(&mut self) {
        let physical = self
            .controllers
            .iter()
            .any(|controller| controller.axis(sdl2::controller::Axis::TriggerLeft) > 16000);
        if physical {
            self.controller_lt_physical_down = true;
            DS_CONTROLLER_LT_HELD.store(true, std::sync::atomic::Ordering::Relaxed);
            if !ds_aim_toggle("CONTROLLER") && !self.controller_lt_down {
                self.controller_lt_down = true;
                self.ds_aim_active = true;
                DS_AIM_REQUEST.store(1, std::sync::atomic::Ordering::Relaxed);
                echo!("[DSpad] LT held through native control -> aim rearmed");
            }
        } else {
            let was_logical = self.controller_lt_down;
            self.controller_lt_physical_down = false;
            self.controller_lt_down = false;
            self.ds_aim_active = false;
            DS_CONTROLLER_LT_HELD.store(false, std::sync::atomic::Ordering::Relaxed);
            if was_logical || !ds_aim_toggle("CONTROLLER") {
                DS_AIM_REQUEST.store(0, std::sync::atomic::Ordering::Relaxed);
            }
            if self.controller_rt_down || self.controller_rb_down {
                DS_FIRE_RELEASE_REQUEST.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            echo!("[DSpad] LT released during native control -> aim cleared");
        }
    }

    fn controller_added(&mut self, joystick_idx: u32) {
        let Ok(controller) = self.controller_ctx.open(joystick_idx) else {
            log!("Warning: A new controller was connected, but it couldn't be accessed!");
            return;
        };

        let controller_name = controller.name();
        // Repeated add events must not create duplicate logical inputs.
        if self.controllers.iter().any(|open| open.instance_id() == controller.instance_id()) {
            return;
        }
        if env::consts::OS == "android" && controller_name.starts_with("uinput-") {
            log!("ignoring fingerprint device: {}", controller_name);
            return;
        }
        log!(
            "New controller connected: {}. Left stick = device tilt. Right stick = touch input (press the stick or shoulder button to tap/hold).",
            controller_name
        );
        self.controllers.push(controller);
    }
    fn controller_removed(&mut self, instance_id: u32) {
        if !cfg!(target_os = "android") { self.ds_cancel_inputs(); }
        DS_CONTROLLER_LT_HELD.store(false, std::sync::atomic::Ordering::Relaxed);
        let Some(idx) = self
            .controllers
            .iter()
            .position(|controller| controller.instance_id() == instance_id)
        else {
            return;
        };
        let controller = self.controllers.remove(idx);
        log!("Warning: Controller disconnected: {}", controller.name());
    }
    pub fn print_accelerometer_notice(&self, options: &Options) {
        log!("This app uses the accelerometer.");

        if !self.controllers.is_empty() && options.analog_stick_tilt_controls {
            log!("Your connected controller's left analog stick will be used for accelerometer simulation.");
            if self.accelerometer.is_some() {
                log!("Disconnect the controller if you want to use your device's accelerometer.");
            }
        } else if self.accelerometer.is_some() {
            log!("Your device's accelerometer will be used for accelerometer simulation.");
            if options.analog_stick_tilt_controls {
                log!("Connect a controller if you would prefer to use an analog stick.");
            }
        } else if self.controllers.is_empty() && options.analog_stick_tilt_controls {
            log!("Connect a controller to get accelerometer simulation.");
        }

        if self.accelerometer.is_none() {
            log!(
                "You can {}hold right click and move the cursor to simulate the accelerometer.",
                if options.analog_stick_tilt_controls {
                    "also "
                } else {
                    ""
                }
            );
        }
    }

    /// Get the real or simulated accelerometer output.
    /// See also [crate::frameworks::uikit::ui_accelerometer].
    pub fn get_acceleration(&self, options: &Options) -> (f32, f32, f32) {
        // Zero-g jump (V / Shift+Space / Ctrl / LT+R3): synthesise the tested
        // *vigorous* device shake.
        // The real gesture is "shake the phone up and down, fairly hard", so
        // the old alternation every 2 presented frames (about 2 Hz, +/-1 g on
        // one axis) was far too gentle and on the wrong axis. Drive it from the
        // wall clock instead of the presented-frame counter.
        if let Some(t0) = self.ds_shake_start {
            let el = t0.elapsed().as_secs_f32();
            let dur = self.ds_shake_dur.max(0.2);
            if el < dur {
                let w = 2.0 * std::f32::consts::PI * self.ds_shake_freq.max(0.5) * el;
                let s = w.sin();
                let c = w.cos();
                // ramp up quickly, then decay, so the guest sees a clear onset
                let env = if el < 0.15 {
                    el / 0.15
                } else {
                    1.0 - (el - 0.15) / (dur - 0.15).max(0.01) * 0.6
                };
                // Dead Space's jump action requires |a| > 2 g and a normalized
                // direction dot product > 0.75 with -Z or landscape -Y.
                // A constant 1 g direction swing cannot pass that detector.
                // Default to Z, which is unchanged by landscape rotation;
                // 6 g / 3 s / 4 Hz / Z was playtested successfully 2026-10-25.
                let a = self.ds_shake_amp * env;
                // axis: 0 = legacy "faithful" 1 g direction swing,
                // 1 = linear X, 2 = linear Y, 3 = linear Z,
                // 4 = all-axis, 5 = legacy 3-axis tumble.
                return match self.ds_shake_axis % 6 {
                    0 => (0.0, s, -c),
                    1 => (a * s, 0.0, -1.0),
                    2 => (0.0, a * s, -1.0),
                    3 => (0.0, 0.0, -1.0 + a * s),
                    4 => (a * s, a * c, -1.0 + a * s * 0.5),
                    _ => (s * 0.7, c * 0.7, -0.7),
                };
            }
        }
        if !cfg!(target_os="android") && ds_native_inputs(){return (0.0,0.0,-1.0);}
        if self.controllers.is_empty() || !options.analog_stick_tilt_controls {
            if let Some(ref accelerometer) = self.accelerometer {
                let data = accelerometer.get_data().unwrap();
                let sdl2::sensor::SensorData::Accel(data) = data else {
                    // We asked SDL for the accelerometer sensor explicitly
                    // earlier; if SDL handed us a different sensor variant
                    // (driver bug, future SDL version, etc.), keep the host
                    // running by reporting the device flat-on-its-back
                    // (UIAcceleration neutral position).
                    log!(
                        "Warning: accelerometer sensor returned non-Accel data ({:?}); reporting neutral acceleration.",
                        data
                    );
                    return (0.0, 0.0, -1.0);
                };
                let [x, y, z] = data;
                // UIAcceleration reports acceleration towards gravity, but SDL2
                // reports acceleration away from gravity.
                let (x, y, z) = (-x, -y, -z);
                // UIAcceleration reports acceleration in units of g-force, but
                // SDL2 reports acceleration in units of m/s^2.
                let gravity: f32 = 9.80665; // SDL_STANDARD_GRAVITY
                let (x, y, z) = (x / gravity, y / gravity, z / gravity);
                return (x, y, z);
            }
        }

        let (x, y) = if self
            .virtual_accelerometer_last
            .is_some_and(|(_x, _y, right_click_hold)| right_click_hold)
        {
            self.virtual_accelerometer_last
                .map(|(x, y, _right_click_hold)| (x, y))
                .unwrap()
        } else {
            // Get left analog stick input. The range is [-1, 1] on each axis.
            let (x, y, _) = self.get_controller_stick(options, true);
            (x, y)
        };

        // Correct for window rotation
        let [x, y] = self.rotation_matrix().inverse().unwrap().transform([x, y]);
        let (x, y) = (x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)); // just in case

        // Let's simulate tilting the device based on the analog stick inputs.
        //
        // If an iPhone is lying flat on its back, level with the ground, and it
        // is on Earth, the accelerometer will report approximately (0, 0, -1).
        // The acceleration x and y axes are aligned with the screen's x and y
        // axes. +x points to the right of the screen, +y points to the top of
        // the screen, and +z points away from the screen. In the example
        // scenario, the z axis is parallel to gravity.

        let gravity: [f32; 3] = [0.0, 0.0, -1.0];

        let neutral_x = options.x_tilt_offset.to_radians();
        let neutral_y = options.y_tilt_offset.to_radians();
        let x_rotation_range = options.x_tilt_range.to_radians() / 2.0;
        let y_rotation_range = options.y_tilt_range.to_radians() / 2.0;
        // (x, y) are swapped because the controller Y axis usually corresponds
        // to forward/backward movement, but rotating about the Y axis means
        // tilting the device left/right.
        let x_rotation = neutral_x - x_rotation_range * y;
        let y_rotation = neutral_y - y_rotation_range * x;
        let matrix =
            Matrix::<3>::y_rotation(y_rotation).multiply(&Matrix::<3>::x_rotation(x_rotation));
        let [x, y, z] = matrix.transform(gravity);

        (x, y, z)
    }

    /// For use when redrawing the screen: Get the cached on-screen position and
    /// press state of the analog stick-controlled virtual cursor, if it is
    /// visible.
    pub fn virtual_cursor_visible_at(&self) -> Option<(f32, f32, bool)> {
        if let Some(point) = crate::ds_frontend_controller::menu_cursor() {
            let (x,y,w,h) = self.viewport();
            return Some((x as f32+point.x*w as f32/480.0,y as f32+point.y*h as f32/320.0,false));
        }
        // Native camera look must not also draw the legacy 20px touch cursor.
        // It remains available in explicit gamepad/click cursor mode.
        if ds_native_pointer_enabled() && !self.ds_cursor_mode_active() {
            let focused = unsafe { sdl2_sys::SDL_GetKeyboardFocus() == self.window.raw()
                && sdl2_sys::SDL_GetMouseFocus() == self.window.raw() };
            if self.ds_mouse_capture.captured || !focused {
                if !self.ds_mouse_capture.captured { unsafe { sdl2_sys::SDL_ShowCursor(1); } }
                return None;
            }
            let (ww,wh)=self.window.size();
            let (dw,dh)=self.window.drawable_size();
            let mut mx=0;let mut my=0;
            let buttons=unsafe { sdl2_sys::SDL_GetMouseState(&mut mx,&mut my) };
            let cursor=crate::ds_pointer::mouse_at((ww,wh),(dw,dh),self.viewport(),(mx,my),buttons&1!=0);
            // In native-pointer mode the software cursor owns the pointer even
            // when it is over letterbox space; showing the OS arrow there
            // produces the visible double-cursor bug.
            unsafe { sdl2_sys::SDL_ShowCursor(0); }
            if cursor.is_some() { log_once!("[DScursor] native output pointer active; SDL click hotspot, HiDPI-aware"); }
            return cursor;
        }
        if ds_native_inputs() && !self.ds_cursor_mode_active() { return None; }

        let (x, y, pressed, visible) = self.virtual_cursor_last?;
        if visible {
            // When stickyness is in use, the visual cursor movement appears
            // uncomfortably choppy. Showing the un-sticky position is a bit
            // misleading but it *feels* better, and it is documented.
            if let Some((x_unsticky, y_unsticky, _time)) = self.virtual_cursor_last_unsticky {
                Some((x_unsticky, y_unsticky, pressed))
            } else {
                Some((x, y, pressed))
            }
        } else {
            None
        }
    }

    /// Update the virtual cursor's position, click state and visibility, then
    /// return the new position, pressed state, whether the press state changed
    /// and whether the cursor moved.
    fn update_virtual_cursor(&mut self, options: &Options) -> (f32, f32, bool, bool, bool) {
        // Get right analog stick input. The range is [-1, 1] on each axis.
        let (x, y, pressed) = self.get_controller_stick(options, false);
        if crate::ds_frontend_controller::capture_menu() {
            let (vx,vy,vw,vh)=self.viewport();
            let now=Instant::now();
            let dt=self.virtual_cursor_last_unsticky.map(|(_,_,time)|now.saturating_duration_since(time).as_secs_f32().min(0.05)).unwrap_or(1.0/60.0);
            let prior=crate::ds_frontend_controller::menu_cursor().map(|p|(vx as f32+p.x*vw as f32/480.0,vy as f32+p.y*vh as f32/320.0))
                .or_else(||self.virtual_cursor_last.map(|v|(v.0,v.1))).unwrap_or((vx as f32+vw as f32/2.0,vy as f32+vh as f32/2.0));
            let speed=vh as f32*0.85;
            let nx=(prior.0+x*speed*dt).clamp(vx as f32,(vx+vw) as f32);
            let ny=(prior.1+y*speed*dt).clamp(vy as f32,(vy+vh) as f32);
            if x!=0.0||y!=0.0 {
                // No stick motion must not hover the center during station
                // entry/scrolling and erase the native directional selection.
                crate::ds_frontend_controller::hover(crate::ds_frontend_controller::Point{x:(nx-vx as f32)*480.0/vw.max(1) as f32,y:(ny-vy as f32)*320.0/vh.max(1) as f32});
            }
            let old_pressed=self.virtual_cursor_last.map(|v|v.2).unwrap_or(false);
            let click=pressed&&crate::ds_frontend_controller::menu_cursor().is_none();
            self.virtual_cursor_last=Some((nx,ny,click,true));
            self.virtual_cursor_last_unsticky=Some((nx,ny,now));
            return (nx,ny,click,click!=old_pressed,nx!=prior.0||ny!=prior.1);
        }

        // The cursor is intended to only show up once you move the analog stick
        // out of its deadzone, or while the button is held.
        let visible = self.ds_cursor_mode_active() || pressed || x != 0.0 || y != 0.0;

        // Though the analog stick output fits within a square, its actual range
        // is usually a circle enclosed by the square. So we need to cut out the
        // rectangular shape of the screen from that circle within the square.
        let (vx, vy, vw, vh) = self.viewport();
        let (vx, vy, vw, vh) = (vx as f32, vy as f32, vw as f32, vh as f32);

        let (x, y) = {
            // Use Pythagoras's theorem to find the largest size the rectangle
            // can have within the circle.
            let ratio = vw / vh;
            let rect_height = (ratio * ratio + 1.0).powf(-0.5);
            let rect_width = ratio * rect_height;

            let x_abs = x.abs().min(rect_width) / rect_width;
            let y_abs = y.abs().min(rect_height) / rect_height;
            (x_abs.copysign(x), y_abs.copysign(y))
        };

        // Convert to on-screen window co-ordinates
        let x = (x / 2.0 + 0.5) * vw + vx;
        let y = (y / 2.0 + 0.5) * vh + vy;

        let (old_x, old_y, old_pressed, _old_visible) =
            self.virtual_cursor_last.unwrap_or_default();

        let (x, y) = if let Some((smoothing_strength, sticky_radius)) =
            options.stabilize_virtual_cursor
        {
            let new_time = Instant::now();

            let (old_x_unsticky, old_y_unsticky, old_time) = self
                .virtual_cursor_last_unsticky
                .unwrap_or((0.0, 0.0, new_time));

            let delta_t = new_time.saturating_duration_since(old_time).as_secs_f32();

            // Apply a feedback-based smoothing with exponential decay, to try
            // to dampen shakiness in the stick movement.

            let smooth = |old: f32, new: f32| -> f32 {
                if smoothing_strength != 0.0 {
                    let lerp_factor = 1.0 - (0.5_f32).powf(delta_t * (1.0 / smoothing_strength));
                    old + (new - old) * lerp_factor
                } else {
                    new
                }
            };

            let new_x_unsticky = smooth(old_x_unsticky, x);
            let new_y_unsticky = smooth(old_y_unsticky, y);

            self.virtual_cursor_last_unsticky = Some((new_x_unsticky, new_y_unsticky, new_time));

            // Make the reported position "sticky" within a certain radius, i.e.
            // if the new position's distance from the old one is within the
            // radius, report no change in position.

            if (new_x_unsticky - old_x).hypot(new_y_unsticky - old_y) < sticky_radius {
                (old_x, old_y)
            } else {
                (new_x_unsticky, new_y_unsticky)
            }
        } else {
            (x, y)
        };

        self.virtual_cursor_last = Some((x, y, pressed, visible));

        (
            x,
            y,
            pressed,
            pressed != old_pressed,
            x != old_x || y != old_y,
        )
    }

    /// Get the summed X and Y positions and button state of the left or right
    /// analog stick of the game controllers. Each axis value is in the range
    /// [-1, 1].
    fn get_controller_stick(&self, options: &Options, left: bool) -> (f32, f32, bool) {
        if !cfg!(target_os = "android") && ((self.ds_capture_suspended && self.ds_test_pad.is_none()) || ds_input_mode() == ds_gamepad_bindings::InputMode::Keyboard) {
            return (0.0, 0.0, false);
        }
        fn convert_axis(axis: i16, deadzone: f32) -> f32 {
            assert!(deadzone >= 0.0);
            let axis = ((axis as f32) / (i16::MAX as f32)).clamp(-1.0, 1.0);
            let abs_axis = (axis.abs().max(deadzone) - deadzone) / (1.0 - deadzone);
            abs_axis.copysign(axis)
        }

        let (mut x, mut y) = (0.0, 0.0);
        let mut pressed = false;
        for controller in &self.controllers {
            if ds_background_test() && !self.ds_test_pad.as_ref().is_some_and(|p|p.instance==controller.instance_id()){continue;}
            use sdl2::controller::{Axis, Button};
            let (x_axis, y_axis, button1, button2) = if left {
                (
                    Axis::LeftX,
                    Axis::LeftY,
                    Button::LeftStick,
                    Button::LeftShoulder,
                )
            } else {
                (Axis::RightX, Axis::RightY, Button::RightStick, Button::A)
            };
            x += convert_axis(controller.axis(x_axis), options.deadzone);
            y += convert_axis(controller.axis(y_axis), options.deadzone);
            // On PC, L3/R3 (and buttons remapped to those semantics) belong to
            // player shortcuts. Polling them as touch clicks also fired the
            // weapon/virtual cursor in the same frame. Android touch is intact.
            if cfg!(target_os = "android") { pressed |= controller.button(button1); }
            // Semantic A/LB already dispatch interact/combat. Polling them
            // again as legacy touch clicks re-entered benches continuously and
            // mixed a slash into stomp. Only explicit cursor mode owns A-click.
            if cfg!(target_os="android") || !ds_gamepad_semantic()
                || (!left && self.ds_gamepad_cursor_mode) {
                pressed |= controller.button(if !cfg!(target_os="android") {ds_remap_pad(button2,true)}else{button2});
            }
        }
        let (x, y) = (x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0));

        (x, y, pressed)
    }

    pub fn create_gl_context(&self, version: GLVersion) -> Result<GLContext, String> {
        let attr = self.video_ctx.gl_attr();
        match version {
            GLVersion::GLES11 => {
                attr.set_context_version(1, 1);
                attr.set_context_profile(sdl2::video::GLProfile::GLES);
            }
            GLVersion::GLES20 => {
                attr.set_context_version(2, 0);
                attr.set_context_profile(sdl2::video::GLProfile::GLES);
            }
            GLVersion::GLES30 => {
                attr.set_context_version(3, 0);
                attr.set_context_profile(sdl2::video::GLProfile::GLES);
            }
            GLVersion::GL21Compat => {
                attr.set_context_version(2, 1);
                attr.set_context_profile(sdl2::video::GLProfile::Compatibility);
            }
            GLVersion::GL33Core => {
                attr.set_context_version(3, 3);
                attr.set_context_profile(sdl2::video::GLProfile::Core);
            }
        }

        let gl_ctx = self.window.gl_create_context()?;
        // `TOUCHHLE_DS_VSYNC=0`: present immediately instead of waiting for the
        // display refresh. On a high-refresh monitor this lifts the emulator's
        // frame rate ceiling (the guest still runs its own logic cadence, but
        // presentation - and therefore the frame counter - is no longer tied to
        // 60 Hz). Default keeps the upstream vsync behaviour.
        if std::env::var("TOUCHHLE_DS_VSYNC")
            .map(|v| v.trim() == "0")
            .unwrap_or(false)
        {
            match self.video_ctx.gl_set_swap_interval(0) {
                Ok(()) => log!("[DSvsync] swap interval 0 (vsync off)"),
                Err(e) => log!("[DSvsync] could not disable vsync: {}", e),
            }
        }

        Ok(GLContext(gl_ctx))
    }

    pub fn gl_get_proc_address(&self, procname: &str) -> *const std::ffi::c_void {
        // For some reason, rust-sdl2 uses *const (), but () is not meant to be
        // used for void pointees (just void results), so let's fix that.
        self.video_ctx.gl_get_proc_address(procname) as *const _
    }

    pub fn set_share_with_current_context(&self, value: bool) {
        self.video_ctx
            .gl_attr()
            .set_share_with_current_context(value)
    }

    pub unsafe fn make_gl_context_current(&self, gl_ctx: &GLContext) {
        self.window.gl_make_current(&gl_ctx.0).unwrap();
    }

    /// Make the internal OpenGL ES context (for splash screen and UI rendering)
    /// current.
    #[must_use]
    pub fn make_internal_gl_ctx_current<'win>(&'win mut self) -> Box<dyn GLES + 'win> {
        // The invariant is held up here - since the instance we return is
        // bound to the lifetime of window, it can't outlive the internal GL
        // context and can't outlive the window.
        let gl_ins = unsafe {
            self.internal_gl_ins
                .as_mut()
                .unwrap()
                .make_current_unchecked_for_window(
                    &mut |gl_ctx| self.window.gl_make_current(&gl_ctx.0).unwrap(),
                    &mut |s| self.video_ctx.gl_get_proc_address(s) as *const _,
                )
        };
        gl_ins
    }

    fn display_splash(&mut self) {
        assert!(self.splash_image.is_some());

        let present_state = std::rc::Rc::clone(&self.internal_present_state);
        let image = self.splash_image.as_ref().unwrap();
        let (image_width, image_height) = image.dimensions();

        // Legacy iPhone landscape launch images are stored in a portrait-sized
        // Default.png with their content already rotated. Applying the normal
        // framebuffer rotation to such an image turns it upside down, so use
        // the inverse rotation for that case. Native landscape-sized launch
        // images and portrait launch images use the regular transform.
        let is_landscape = matches!(
            self.device_orientation,
            DeviceOrientation::LandscapeLeft | DeviceOrientation::LandscapeRight
        );
        let rotation = if self.splash_image_is_orientation_specific {
            Matrix::identity()
        } else if is_landscape && image_height > image_width {
            self.rotation_matrix().inverse().unwrap()
        } else {
            self.rotation_matrix()
        };

        // OpenGL ES expects bottom-to-top row order for image data, but our
        // image data will be top-to-bottom. A reflection transform compensates.
        let matrix = rotation.multiply(&Matrix::y_flip());
        let (vx, vy, vw, vh) = self.viewport();
        let viewport = (vx, vy + self.viewport_y_offset(), vw, vh);

        unsafe {
            let mut gl_ctx = self
                .internal_gl_ins
                .as_mut()
                .unwrap()
                .make_current_unchecked_for_window(
                    &mut |gl_ctx| self.window.gl_make_current(&gl_ctx.0).unwrap(),
                    &mut |s| self.video_ctx.gl_get_proc_address(s) as *const _,
                );

            use crate::gles::gles11_raw as gles11; // constants only

            let mut texture = 0;
            gl_ctx.GenTextures(1, &mut texture);
            gl_ctx.BindTexture(gles11::TEXTURE_2D, texture);
            gl_ctx.TexImage2D(
                gles11::TEXTURE_2D,
                0,
                gles11::RGBA as _,
                image_width as _,
                image_height as _,
                0,
                gles11::RGBA,
                gles11::UNSIGNED_BYTE,
                image.pixels().as_ptr() as *const _,
            );
            gl_ctx.TexParameteri(
                gles11::TEXTURE_2D,
                gles11::TEXTURE_MIN_FILTER,
                gles11::LINEAR as _,
            );
            gl_ctx.TexParameteri(
                gles11::TEXTURE_2D,
                gles11::TEXTURE_MAG_FILTER,
                gles11::LINEAR as _,
            );

            present_frame(
                gl_ctx.as_mut(),
                &mut present_state.borrow_mut(),
                viewport,
                matrix,
                /* virtual_cursor_visible_at: */ None,
                /* source_size: */ None,
                /* text_overlay: */ None,
            );

            gl_ctx.DeleteTextures(1, &texture);
        };

        self.window.gl_swap_window();

        // hold onto GL context so the image doesn't disappear, and hold
        // onto image so we can rotate later if necessary
    }

    /// Swap front-buffer and back-buffer so the result of OpenGL rendering is
    /// presented.
    pub fn swap_window(&self) {
        // Frame-rate cap (`TOUCHHLE_DS_FPS_CAP`): sleep until the frame budget
        // has elapsed, so a fast machine does not spin the guest at hundreds of
        // frames per second. Unset = no sleep at all (previous behaviour), which
        // is what `TOUCHHLE_DS_VSYNC=0` runs want.
        {
            if let Some(cap) = ds_fps_cap() {
                // Budget the *presented frame count* rather than the number of
                // swap calls: `swap_window()` runs twice per presented frame in
                // this port (Core Animation composition + the EAGL present), and
                // per-swap pacing made a 15 fps cap come out as 7.8 fps
                // (measured 2026-10-16). Using the guest's frame counter is
                // self-correcting and also right if the two paths are ever
                // reordered.
                use std::sync::OnceLock;
                static EPOCH: OnceLock<std::time::Instant> = OnceLock::new();
                let frame = DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed) as f64;
                let epoch = *EPOCH.get_or_init(std::time::Instant::now);
                let target = epoch + std::time::Duration::from_secs_f64(frame / cap as f64);
                let now = std::time::Instant::now();
                if target > now {
                    let rest = target - now;
                    if rest > std::time::Duration::from_micros(200) {
                        std::thread::sleep(rest);
                    }
                }
            }
        }
        self.window.gl_swap_window();
    }

    /// Consider the emulated device to be rotated to a particular orientation.
    ///
    /// On a PC or laptop, this will make the window be rotated so the app
    /// content appears upright. On a mobile device, this might do something
    /// else, because the user can physically rotate the screen.
    pub fn rotate_device(&mut self, new_orientation: DeviceOrientation) {
        if !self.on_main_stack {
            log!("Warning: rotate_device called off main stack, skipping");
            return;
        }
        if new_orientation == self.device_orientation {
            return;
        }

        if !self.fullscreen && !Self::rotatable_fullscreen() {
            let (width, height) = if Self::rotatable_fullscreen() {
                set_sdl2_orientation(new_orientation);
                rotate_fullscreen_size(new_orientation, self.window.size())
            } else {
                size_for_orientation(self.device_family, new_orientation, self.present_scale)
            };

            // macOS quirk: when resizing the window, the new framebuffer's size
            // is apparently max(new_size, old_size) in each dimension, but the
            // viewport is positioned wrong on the y axis for some reason, so we
            // need to apply an offset.
            // Recreating the OpenGL context was an alternative workaround, but
            // that apparently stops other OpenGL contexts drawing to the
            // framebuffer!
            #[cfg(target_os = "macos")]
            {
                let (_old_width, old_height) = self.window.size();
                self.max_height = self.max_height.max(old_height).max(height);
                self.viewport_y_offset = self.max_height - height;
            }

            self.window.set_size(width, height).unwrap();
        }

        if Self::rotatable_fullscreen() {
            set_sdl2_orientation(new_orientation);
            // Hack: from reading SDL2's source code, it seems that SDL2 will
            // only re-do the orientation when changing whether a window is
            // "resizeable" (can be rotated). You can't set the resizeable state
            // on a fullscreen window, so it must be temporarily stop being
            // fulscreen.
            // Apparently, doing this does result in resizing the window.
            self.window
                .set_fullscreen(sdl2::video::FullscreenType::Off)
                .unwrap();
            unsafe {
                let window_raw = self.window.raw();
                sdl2_sys::SDL_SetWindowResizable(window_raw, sdl2_sys::SDL_bool::SDL_FALSE);
                sdl2_sys::SDL_SetWindowResizable(window_raw, sdl2_sys::SDL_bool::SDL_TRUE);
            }
            self.window
                .set_fullscreen(sdl2::video::FullscreenType::True)
                .unwrap();
        }

        self.device_orientation = new_orientation;

        if self.splash_image.is_some() {
            self.display_splash();
        }
    }

    pub fn device_family(&self) -> DeviceFamily {
        self.device_family
    }

    pub fn screen_size(&self) -> (u32, u32) {
        self.host_screen_size
            .unwrap_or_else(|| self.device_family.portrait_size())
    }

    pub fn screen_scale(&self) -> f32 {
        // Dead Space 1.3.34 derives atlas sub-rect UVs from UIScreen.scale.
        // Reporting 1.0 in windowed mode (the upstream default) makes it
        // sample a quarter of each sprite, which shows up as a 4x magnified
        // fragment. Allow overriding to the real device scale for testing.
        if let Ok(v) = std::env::var("TOUCHHLE_DS_SCREEN_SCALE") {
            if let Ok(s) = v.parse::<f32>() {
                if s > 0.0 {
                    return s;
                }
            }
        }
        if self.host_screen_size.is_some() {
            1.0
        } else {
            self.device_family.scale_factor()
        }
    }

    /// Returns the current device orientation
    pub fn current_rotation(&self) -> DeviceOrientation {
        self.device_orientation
    }

    /// Get the size in pixels of the window without rotation or scaling.
    ///
    /// The aspect ratio, scale and orientation reflect the guest app's view of
    /// the world.
    pub fn size_unrotated_unscaled(&self) -> (u32, u32) {
        size_for_orientation_from_size(
            self.screen_size(),
            DeviceOrientation::Portrait,
            NonZeroU32::new(1).unwrap(),
        )
    }

    /// Get the region of the on-screen window (x, y, width, height) used to
    /// display the app content.
    ///
    /// The aspect ratio of this region always reflects the guest app's view of
    /// the world, but the scale and orientation might not.
    pub fn viewport(&self) -> (u32, u32, u32, u32) {
        let (app_width, app_height) = size_for_orientation_from_size(
            self.screen_size(),
            self.device_orientation,
            self.present_scale,
        );
        if !self.fullscreen && !Self::rotatable_fullscreen() && self.windowed_output_size.is_none()
        {
            return (0, 0, app_width, app_height);
        }

        let (screen_width, screen_height) = self.window.drawable_size();

        // Optional edge-to-edge presentation for phones whose physical screen
        // is much wider than the game's original 3:2 iPhone panel. Keeping the
        // original aspect is the default; stretching is explicit because it
        // changes the intended composition.
        if std::env::var("TOUCHHLE_DS_VIEWPORT_STRETCH").as_deref() == Ok("1") {
            log_once!("[DSviewport] stretching full 3:2 guest frame to the physical window");
            return (0, 0, screen_width, screen_height);
        }

        let app_aspect = app_width as f32 / app_height as f32;
        let screen_aspect = screen_width as f32 / screen_height as f32;
        let (scaled_width, scaled_height) = if app_aspect < screen_aspect {
            (
                (screen_height as f32 * app_aspect).round() as u32,
                screen_height,
            )
        } else {
            (
                screen_width,
                (screen_width as f32 / app_aspect).round() as u32,
            )
        };
        let x = (screen_width - scaled_width) / 2;
        let y = (screen_height - scaled_height) / 2;
        (x, y, scaled_width, scaled_height)
    }

    /// Special offset to add to y co-ordinates, only when drawing to screen.
    pub fn viewport_y_offset(&self) -> u32 {
        #[cfg(target_os = "macos")]
        return self.viewport_y_offset;
        #[cfg(not(target_os = "macos"))]
        return 0;
    }

    /// Transformation matrix for transforming between the window's co-ordinate
    /// space and the app's original co-ordinate space when rotation is in use
    /// (see [Self::rotate_device]). This returns a matrix appropriate for
    /// rotating texture co-ordinates to display the image in the window; when
    /// rotating input co-ordinates, invert the matrix.
    pub fn rotation_matrix(&self) -> Matrix<2> {
        match self.device_orientation {
            DeviceOrientation::Portrait => Matrix::identity(),
            DeviceOrientation::PortraitUpsideDown => Matrix::z_rotation(PI),
            DeviceOrientation::LandscapeLeft => Matrix::z_rotation(-FRAC_PI_2),
            DeviceOrientation::LandscapeRight => Matrix::z_rotation(FRAC_PI_2),
        }
    }

    pub fn is_screen_saver_enabled(&self) -> bool {
        self.video_ctx.is_screen_saver_enabled()
    }
    pub fn set_screen_saver_enabled(&mut self, enabled: bool) {
        if !self.on_main_stack {
            log!("Warning: set_screen_saver_enabled called off main stack, skipping");
            return;
        }
        match enabled {
            true => self.video_ctx.enable_screen_saver(),
            false => self.video_ctx.disable_screen_saver(),
        }
    }

    pub fn start_text_input(&self) {
        if !self.on_main_stack {
            log!("Warning: start_text_input called off main stack, skipping");
            return;
        }
        unsafe {
            sdl2_sys::SDL_StartTextInput();
        }
    }
    pub fn stop_text_input(&self) {
        if !self.on_main_stack {
            log!("Warning: stop_text_input called off main stack, skipping");
            return;
        }
        unsafe {
            sdl2_sys::SDL_StopTextInput();
        }
    }

    pub fn on_main_stack(&self) -> bool {
        self.on_main_stack
    }
}

pub fn open_url(env: &mut Environment, url: &str) -> Result<(), String> {
    env.on_parent_stack_in_coroutine(|_, _| sdl2::url::open_url(url).map_err(|e| e.to_string()))
}

/// Show an SDL messagebox for an error (typically after a panic).
///
/// The window argument allows for passing in the parent window for the
/// messagebox, which is not required but should be done if possible.
pub fn show_error_messagebox(window: Option<&Window>, error_message: &str) {
    if window.is_some_and(|win| !win.on_main_stack) {
        log!("Warning: show_error_messagebox called off main stack, skipping");
        return;
    }
    use sdl2::messagebox;
    let mbox = [
        messagebox::ButtonData {
            flags: messagebox::MessageBoxButtonFlag::NOTHING,
            button_id: 0,
            text: "Open touchHLE directory",
        },
        messagebox::ButtonData {
            flags: messagebox::MessageBoxButtonFlag::NOTHING,
            button_id: 1,
            text: "Close",
        },
    ];

    let Ok(clicked_button) = messagebox::show_message_box(
        messagebox::MessageBoxFlag::ERROR,
        &mbox,
        "touchHLE crashed!",
        &format!("touchHLE crashed with the following error: {error_message}"),
        window.map(|win| &win.window),
        None,
    ) else {
        log!("Warning: Failed to show error message box; falling back to stderr only.");
        eprintln!("touchHLE crashed: {}", error_message);
        return;
    };

    match clicked_button {
        messagebox::ClickedButton::CloseButton => {}
        messagebox::ClickedButton::CustomButton(button) => {
            match button.button_id {
                // Open data directory (contains log file on android)
                0 => match crate::paths::url_for_opening_user_data_dir() {
                    Ok(url) => {
                        if let Err(e) = sdl2::url::open_url(&url).map_err(|e| e.to_string()) {
                            echo!("Couldn't open file manager at {:?}: {}", url, e);
                        } else {
                            echo!("Opened file manager at {:?}, exiting.", url);
                        }
                    }
                    Err(e) => echo!("Couldn't open file manager: {}", e),
                },
                // Close
                1 => {}
                _ => {
                    // SDL_ShowMessageBox should never return an unknown id
                    // for the buttons we configured, but be defensive.
                    log!("Warning: unexpected message-box button id; ignoring.");
                }
            }
        }
    }
}

/// Get current battery state from SDL2.
///
/// Returns:
/// - pct: i32 - percentage of battery remaining.
/// - status: [BatteryState] - the current status of the battery
///   (unplugged, charging, full, etc.)
pub fn get_battery_status(env: &mut Environment) -> (i32, BatteryState) {
    // On Android SDL_GetPowerInfo crosses into Java via JNI. Calling that from
    // a corosensei guest stack makes ART report a bogus StackOverflowError and
    // abort. Switching to the SDL parent stack is safe on every host.
    env.on_parent_stack_in_coroutine(|_, _| {
        let mut pct = 0;
        // Unfortunately, Rust-SDL2 does not expose this function yet.
        // iPhoneOS does not measure the battery in seconds remaining,
        // so we discard this argument.
        let status = unsafe { sdl2_sys::SDL_GetPowerInfo(null_mut(), &mut pct) };
        (
            pct,
            match status {
                SDL_PowerState::SDL_POWERSTATE_UNKNOWN => BatteryState::Unknown,
                SDL_PowerState::SDL_POWERSTATE_ON_BATTERY => BatteryState::OnBattery,
                SDL_PowerState::SDL_POWERSTATE_NO_BATTERY => BatteryState::NoBattery,
                SDL_PowerState::SDL_POWERSTATE_CHARGING => BatteryState::Charging,
                SDL_PowerState::SDL_POWERSTATE_CHARGED => BatteryState::Full,
            },
        )
    })
}

pub fn get_preferred_language_codes(env: &mut Environment) -> Vec<String> {
    // In headless mode there is no window, and the parent-stack machinery
    // [Environment::on_parent_stack_in_coroutine] relies on requires one. The
    // closure below doesn't actually use the window, but routing through it
    // would still unwrap the absent window and panic. There is no meaningful
    // user locale to report without a session anyway, so report no preference
    // and let the caller fall back to its default (English).
    if env.window.is_none() {
        return Vec::new();
    }
    env.on_parent_stack_in_coroutine(|_, _| {
        sdl2::locale::get_preferred_locales()
            .map(|loc| loc.lang)
            .collect()
    })
}

pub fn get_preferred_country_codes(env: &mut Environment) -> Vec<String> {
    // See the note in `get_preferred_language_codes` about headless mode.
    if env.window.is_none() {
        return Vec::new();
    }
    env.on_parent_stack_in_coroutine(|_, _| {
        sdl2::locale::get_preferred_locales()
            .filter_map(|loc| loc.country)
            .collect()
    })
}

/// Show a UIAlertView-style dialog using SDL2 message box.
/// Returns the index of the clicked button, or 0 if closed.
pub fn show_alert_dialog(
    env: &mut Environment,
    title: &str,
    message: &str,
    buttons: &[&str],
) -> i32 {
    let title = title.to_string();
    let message = message.to_string();
    let buttons: Vec<String> = buttons.iter().map(|s| s.to_string()).collect();

    env.on_parent_stack_in_coroutine(move |window, _options| {
        use sdl2::messagebox;

        let button_data: Vec<messagebox::ButtonData> = buttons
            .iter()
            .enumerate()
            .map(|(i, text)| messagebox::ButtonData {
                flags: messagebox::MessageBoxButtonFlag::NOTHING,
                button_id: i as i32,
                text: text.as_str(),
            })
            .collect();

        match messagebox::show_message_box(
            messagebox::MessageBoxFlag::INFORMATION,
            &button_data,
            &title,
            &message,
            Some(&window.window),
            None,
        ) {
            Ok(messagebox::ClickedButton::CustomButton(btn)) => btn.button_id,
            _ => 0,
        }
    })
}

#[cfg(test)]
mod aim_release_tests {
    use super::ds_take_aim_release;

    #[test]
    fn physical_release_survives_a_logical_aim_cancel() {
        let mut physical = true;
        assert!(ds_take_aim_release(&mut physical, false));
        assert!(!physical);
        assert!(!ds_take_aim_release(&mut physical, false));
    }

    #[test]
    fn logical_hold_release_is_still_acknowledged_without_physical_state() {
        let mut physical = false;
        assert!(ds_take_aim_release(&mut physical, true));
        assert!(!ds_take_aim_release(&mut physical, false));
    }
}

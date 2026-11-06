/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! touchHLE is a high-level emulator (HLE) for iPhone OS applications.
//!
//! In various places, the terms "guest" and "host" are used to distinguish
//! between the emulated application (the "guest") and the emulator itself (the
//! "host"), and more generally, their different environments.
//! For example:
//! - The guest is a 32-bit application, so a "guest pointer" is 32 bits.
//! - The host is a 64-bit application, so a "host pointer" is 64 bits.
//! - The guest can only directly access "guest memory".
//! - The host can access both "guest memory" and "host memory".
//! - A "guest function" is emulated Arm code, usually from the app binary.
//! - A "host function" is a Rust function that is part of this emulator.
// Allow the crate to have a non-snake-case name (touchHLE).
// This also allows items in the crate to have non-snake-case names.
#![allow(non_snake_case)]
// The documentation for this crate is intended to include private items.
// rustdoc complains about some public macros that link to private items, but
// we're forced to make those macros public by the weird macro scoping rules,
// so this warning is unhelpful.
#![allow(rustdoc::private_intra_doc_links)]

#[macro_use]
mod log;
mod abi;
mod audio;
mod bundle;
mod cpu;
mod debug;
mod ds_savestate;
mod ds_texid;
mod ds_texture_mips;
mod ds_chapter_overlay;
mod ds_frontend_controller;
mod ds_font_roles;
mod ds_frontend_trace;
mod ds_game_clock;
mod ds_qte_geometry;
mod ds_pointer;
mod ds_subtitle_overlay;
mod ds_objective_overlay;
mod ds_host_overlay;
mod ds_text_presentation;
mod ds_caption_layout;
mod ds_panel_motion;
mod ds_tutorial_overlay;
mod ds_tutorial_text;
mod ds_tutorial_prompts;
mod ds_qte_overlay;
mod ds_perf;
mod ds_perf_control;
mod ds_monitor;
mod dyld;
mod environment;
mod ds_startup_report;
mod ds_diagnostics;
mod ds_exit_trace;
mod ds_scene_diagnostics;
mod ds_draw_env;
mod ds_gl_profile;
mod ds_guest_skin_probe;
#[cfg(all(windows, target_arch = "x86_64"))]
mod ds_pc_skin_math;
#[cfg(all(windows, target_arch = "x86_64"))]
mod ds_pc_transform_math;
#[cfg(all(windows, target_arch = "x86_64"))]
mod ds_pc_transform;
#[cfg(all(windows, target_arch = "x86_64"))]
mod ds_pc_skin_shadow;
mod ds_stutter_probe;
mod ds_player_action_cache;
/// Bounded best-effort flush on process teardown, never on a normal frame.
pub fn finish_play_diagnostics(normal:bool){ds_diagnostics::finish(normal);}
mod ds_hardware_benchmark;
mod font;
mod frameworks;
mod fs;
mod gdb;
mod gles;
mod image;
mod libc;
mod mach_o;
mod matrix;
mod mem;
mod objc;
mod options;
mod paths;
mod stack;
mod window;

/// JNI bridge used by the Android real-present FPS overlay.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_MainActivity_nativeGetFrameCounter(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> i64 {
    crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed) as i64
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_MainActivity_nativeGetFrameStats(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> i64 {
    crate::window::ds_present_stats_packed() as i64
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_MainActivity_nativeGetFrontendUiState(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> i32 {
    if crate::ds_frontend_controller::hint_text().is_some() {
        if crate::ds_frontend_controller::capture_scene_dialog() { 1000 }
        else { crate::ds_frontend_controller::state() as i32 }
    } else { -1 }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_MainActivity_nativeFrontendKeyboardHints(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> bool {
    crate::ds_frontend_controller::keyboard_hints()
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_MainActivity_nativeRequestGamePause(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> bool {
    crate::ds_frontend_controller::request_native_pause()
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_MainActivity_nativeResumeGame(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) { crate::ds_frontend_controller::request_native_resume(); }

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_MainActivity_nativeHasInGameHud(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> bool { crate::ds_frontend_controller::in_game() }

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_MainActivity_nativeCancelPendingGamePause(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) { crate::ds_frontend_controller::cancel_pending_pause(); }


#[cfg(target_os = "android")]
mod android_crash {
    use std::ffi::{c_char, c_int, c_void};

    #[link(name = "log")]
    extern "C" {
        fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }

    extern "C" fn signal_handler(
        signal: c_int,
        _info: *mut libc::siginfo_t,
        _context: *mut c_void,
    ) {
        let message = match signal {
            libc::SIGSEGV => b"touchHLE native crash: SIGSEGV\0".as_slice(),
            libc::SIGABRT => b"touchHLE native crash: SIGABRT\0".as_slice(),
            libc::SIGBUS => b"touchHLE native crash: SIGBUS\0".as_slice(),
            libc::SIGFPE => b"touchHLE native crash: SIGFPE\0".as_slice(),
            libc::SIGILL => b"touchHLE native crash: SIGILL\0".as_slice(),
            _ => b"touchHLE native crash: unknown signal\0".as_slice(),
        };
        unsafe {
            // This handler only performs async-signal-safe operations. The
            // platform's default handler is restored before re-raising so the
            // normal Android tombstone remains available.
            __android_log_write(
                3, // ANDROID_LOG_DEBUG
                b"touchHLE-crash\0".as_ptr().cast(),
                message.as_ptr().cast(),
            );
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }

    pub fn install() {
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = signal_handler as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO | libc::SA_RESETHAND;
            libc::sigemptyset(&mut action.sa_mask);

            for signal in [
                libc::SIGSEGV,
                libc::SIGABRT,
                libc::SIGBUS,
                libc::SIGFPE,
                libc::SIGILL,
            ] {
                let _ = libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
    }
}

// Environment is used very frequently used and used to be in this module, so
// it is re-exported to avoid having to update lots of imports. The other things
// probably shouldn't be, but they need a new home (TODO).
// Unlike its siblings, this module should be considered private and only used
// via re-exports.
use environment::{Environment, MutexId, MutexType, ThreadId, PTHREAD_MUTEX_DEFAULT};

use std::path::PathBuf;

pub use touchHLE_version::*;
/// This is the true entry point on Android (SDLActivity calls it after
/// initialization). On other platforms the true entry point is in src/bin.rs.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn SDL_main(
    argc: std::ffi::c_int,
    argv: *const *const std::ffi::c_char,
) -> std::ffi::c_int {
    android_crash::install();
    echo!("[Android] installed native crash signal handlers");
    // Rust's default panic handler prints to stderr, but on Android that just
    // gets discarded, so we set a custom hook to make debugging easier.
    std::panic::set_hook(Box::new(|info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            *s
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.as_str()
        } else {
            "(non-string payload)"
        };
        if let Some(location) = info.location() {
            echo_no_panic!("Panic at {}: {}", location, payload);
        } else {
            echo_no_panic!("Panic: {}", payload);
        }
    }));

    // SDLActivity passes the String[] returned by MainActivity.getArguments()
    // through nativeRunMain(). Dropping these arguments makes the Android
    // bundled-IPA auto-loader fall back to the app picker even though the Java
    // layer has already extracted and selected the game. Preserve argv,
    // including argv[0], because `main` expects and skips the program name.
    let mut args = Vec::with_capacity(argc.max(0) as usize);
    if argc > 0 && !argv.is_null() {
        unsafe {
            for index in 0..argc as usize {
                let value = *argv.add(index);
                if value.is_null() {
                    continue;
                }
                match std::ffi::CStr::from_ptr(value).to_str() {
                    Ok(value) => args.push(value.to_owned()),
                    Err(error) => {
                        echo!("[Android] Ignoring non-UTF-8 SDL argument #{index}: {error}");
                    }
                }
            }
        }
    }
    if args.is_empty() {
        args.push(String::new());
    }
    echo!("[Android] SDL_main argc={argc}, arguments={args:?}");

    match main(args.into_iter()) {
        Ok(_) => echo!("touchHLE finished"),
        Err(e) => echo!("touchHLE errored: {e:?}"),
    }
    0
}

const USAGE: &str = "\
Usage:
    touchHLE [PATH] [OPTIONS]

PATH should be a path to a .app bundle or .ipa file.

If no app path or special option is specified, a GUI app picker is displayed.

Special options:
    --help
        Display this help text.

    --info
        Print basic information about the app bundle without running the app.
";
pub fn main<T: Iterator<Item = String>>(mut args: T) -> Result<(), String> {
    ds_diagnostics::init(VERSION);
    #[cfg(windows)]
    if ds_diagnostics::enabled() {
        let previous=std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().name()!=Some("ds-diagnostics") {
                ds_diagnostics::fault();ds_diagnostics::finish(false);
            }
            previous(info);
        }));
    }
    echo!(
        "touchHLE {}{}{}",
        branding(),
        if branding().is_empty() { "" } else { " " },
        VERSION,
    );
    if GITHUB_RUN_ID.is_some() && !branding().is_empty() {
        echo!(
            "Built from branch {:?} of {:?} by GitHub Actions workflow run {}/{}/actions/runs/{}.",
            GITHUB_REF_NAME.unwrap(),
            GITHUB_REPOSITORY.unwrap(),
            GITHUB_SERVER_URL.unwrap(),
            GITHUB_REPOSITORY.unwrap(),
            GITHUB_RUN_ID.unwrap()
        );
    }
    echo!();

    {
        let base_path = paths::user_data_base_path();
        log!("Base path for touchHLE files: {}", base_path.display());
        paths::prepopulate_user_data_dir();
    }

    let _ = args.next().unwrap(); // skip argv[0]

    let mut bundle_path: Option<PathBuf> = None;
    let mut just_info = false;
    let mut option_args = Vec::new();
    let mut options = options::Options::default();
    let mut app_args = None::<Vec<String>>;
    for arg in args {
        if let Some(ref mut app_args) = app_args {
            app_args.push(arg);
        } else if arg == "--args" {
            app_args = Some(Vec::new());
        } else if arg == "--help" {
            echo!("{}", USAGE);
            echo!("{}", options::OPTIONS_HELP);
            return Ok(());
        } else if arg == "--info" {
            just_info = true;
        // Parse an option and store a backup in option_args so that we can
        // reapply them after file options are loaded. This ensures that
        // command line options take precedence over file options.
        } else if options.parse_argument(&arg)? {
            option_args.push(arg);
        } else if bundle_path.is_none() {
            bundle_path = Some(PathBuf::from(arg));
        } else {
            echo!("{}", USAGE);
            echo!("{}", options::OPTIONS_HELP);
            return Err(format!("Unexpected argument: {arg:?}"));
        }
    }

    if options.dumping_options.symbols {
        let mut file = std::fs::File::create(&options.dumping_file).map_err(|e| e.to_string())?;
        dyld::Dyld::dump_host_symbols(&mut file).unwrap();
        return Ok(());
    }

    let bundle_path = if let Some(bundle_path) = bundle_path {
        bundle_path
    } else {
        #[cfg(target_os = "android")]
        {
            return Err(
                "No bundled Dead Space IPA was supplied; refusing to open touchHLE's desktop-style app picker."
                    .to_string(),
            );
        }
        #[cfg(not(target_os = "android"))]
        {
            let mut options = options::Options::default();
            // Apply command-line options only (no app-specific options apply)
            for option_arg in &option_args {
                let parse_result = options.parse_argument(option_arg);
                assert!(parse_result == Ok(true));
            }
            if options.headless {
                return Err(
                    "No app specified. Use the --help flag to see command-line usage.".to_string(),
                );
            }
            echo!(
            "No app specified, opening app picker. Use the --help flag to see command-line usage."
        );
            let (bundle_path, mut extra_options) = environment::app_picker::app_picker(options)?;
            option_args.append(&mut extra_options);
            bundle_path
        }
    };
    // When PowerShell does tab-completion on a directory, for some reason it
    // expands it to `'..\My Bundle.app\'` and that trailing \ seems to
    // get interpreted as escaping a double quotation mark?
    #[cfg(windows)]
    if let Some(fixed) = bundle_path.to_str().and_then(|s| s.strip_suffix('"')) {
        log!("Warning: The bundle path has a trailing quotation mark! This often happens accidentally on Windows when tab-completing, because '\\\"' gets interpreted by Rust in the wrong way. Did you meant to write {:?}?", fixed);
    }

    let bundle_data = fs::BundleData::open_any(&bundle_path)
        .map_err(|e| format!("Could not open app bundle: {e}"))?;
    let (bundle, fs) = match bundle::Bundle::new_bundle_and_fs_from_host_path(
        bundle_data,
        /* read_only_mode: */ false,
    ) {
        Ok(bundle) => bundle,
        Err(err) => {
            return Err(format!("Application bundle error: {err}. Check that the path is to an .app directory or an .ipa file."));
        }
    };

    let app_id = bundle.bundle_identifier();

    // Windows DeadSpace defaults are independent of the Android profile below.
    // Explicit environment 0 remains the rollout/diagnostic rollback.
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    if app_id == "com.ea.deadspace.bv" {
        for name in ["TOUCHHLE_DS_GL_STACK_DRIVER", "TOUCHHLE_DS_GL_ENV_CACHE"] {
            if std::env::var_os(name).is_none() {
                std::env::set_var(name, "1");
            }
        }
    }

    // Android cannot rely on the desktop wrapper's environment. Apply the
    // validated Dead Space defaults here while leaving every value overridable
    // by the launcher.
    #[cfg(target_os = "android")]
    if app_id == "com.ea.deadspace.bv" {
        unsafe {
            for (name, value) in [
                ("TOUCHHLE_DS_DEPTH_SIZE", "24"),
                ("TOUCHHLE_DS_NATIVE_DEPTH24", "1"),
                ("TOUCHHLE_DS_MSAA", "4"),
                ("TOUCHHLE_DS_MSAA_CLEAR_DEPTH", "1"),
                ("TOUCHHLE_DS_NO_FOG", "0"),
                ("TOUCHHLE_DS_NO_FOG_ORTHO", "1"),
                ("TOUCHHLE_DS_NO_FOG_SCREEN", "0"),
                ("TOUCHHLE_DS_NO_FOG_HASH", "7027023c,56c62e8a"),
                ("TOUCHHLE_DS_OPENING_FOG_FIX", "1"),
                (
                    "TOUCHHLE_DS_DEPTH_LEQUAL_HASH",
                    "25a08a82,cee43546,36497762,1d5cccb8,8c12a704,87127d0f,e70efc97,2131afd5,18c949fb,f4d178fa",
                ),
                ("TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE", "1"),
                ("TOUCHHLE_DS_NO_CULL_HASH_COUNT", "02d3486c:405"),
                (
                    "TOUCHHLE_DS_TEXNODEPTH_HASH_COUNT",
                    "56c62e8a:6,56c62e8a:36,7027023c:6,7027023c:36",
                ),
                ("TOUCHHLE_DS_TEXFILTER_LINEAR_HASH", "25a08a82"),
                ("TOUCHHLE_DS_TEXFILTER_LINEAR_MAX_COUNT", "6"),
                ("TOUCHHLE_DS_BLOOD_DEPTH_FIX", "1"),
                ("TOUCHHLE_DS_BLOOD_SIZE", "0.5"),
                ("TOUCHHLE_DS_AUTO_OPENING_MIPS", "1"),
                ("TOUCHHLE_DS_AUTO_MIPS_ALL", "0"),
                (
                    "TOUCHHLE_DS_AUTO_MIPS_HASH",
                    "0712153f,24bec7ad,60bb1a47,e0549cfb,02d3486c",
                ),
                (
                    "TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH",
                    "0712153f,24bec7ad,60bb1a47,e0549cfb,02d3486c",
                ),
                ("TOUCHHLE_DS_TEX_UPLOAD_FINISH", "1"),
                ("TOUCHHLE_DS_SCREEN_SCALE", "2"),
                ("TOUCHHLE_DS_CG_GLYPH_SCALE", "0.68"),
                ("TOUCHHLE_DS_CG_GLYPH_DY", "0"),
                ("TOUCHHLE_DS_ALPHA_AS_WHITE", "0"),
            ] {
                if std::env::var_os(name).is_none() {
                    std::env::set_var(name, value);
                }
            }
        }
    }

    // ULTRAHLE_MINIONJUMP_SCREEN_BEGIN
    // Minion Jump / SheepEscape needs the iPad landscape identity/profile.
    unsafe {
        std::env::remove_var("TOUCHHLE_FORCE_IPAD_DEVICE_IDENTITY");
        std::env::remove_var("TOUCHHLE_FORCE_IPAD_LANDSCAPE_SCREEN");
    }

    if matches!(
        app_id,
        "com.apprisetec9.minionjump" | "com.risinghighapps.kingdomprincepro"
    ) {
        unsafe {
            std::env::set_var("TOUCHHLE_FORCE_IPAD_DEVICE_IDENTITY", "1");
            std::env::set_var("TOUCHHLE_FORCE_IPAD_LANDSCAPE_SCREEN", "1");
        }
    }
    // ULTRAHLE_MINIONJUMP_SCREEN_END

    // ULTRAHLE_POTATO_LANDSCAPE_BEGIN
    // Potato Panic / Potato Story: use normal PC-style present rotation/composition; remap touch coordinates as landscape-right.
    unsafe {
        std::env::remove_var("TOUCHHLE_FORCE_LANDSCAPE_VIEWPORT");
        std::env::remove_var("TOUCHHLE_FORCE_LANDSCAPE_RENDERBUFFER");
        std::env::remove_var("TOUCHHLE_FORCE_LANDSCAPE_VIEW_BOUNDS");
        std::env::remove_var("TOUCHHLE_TOUCH_LOCATION_PORTRAIT_TO_LANDSCAPE");
        std::env::remove_var("TOUCHHLE_TOUCH_MODE");
        if app_id == "com.robtop.geometryjump" {
            std::env::set_var("TOUCHHLE_TOUCH_LOCATION_PORTRAIT_TO_LANDSCAPE", "1");
            std::env::set_var("TOUCHHLE_TOUCH_MODE", "right");
        }
        std::env::remove_var("TOUCHHLE_TOUCH_LOCATION_X_OFFSET");
        std::env::remove_var("TOUCHHLE_TOUCH_LOCATION_Y_OFFSET");
        std::env::remove_var("TOUCHHLE_PRESENT_STRETCH_TO_VIEWPORT");
        std::env::remove_var("TOUCHHLE_POTATO_ANDROID_THUMB2_COMPAT");
    }

    if matches!(app_id, "at.source.potpan" | "at.source.potato3D") {
        unsafe {
            std::env::set_var("TOUCHHLE_TOUCH_LOCATION_PORTRAIT_TO_LANDSCAPE", "1");
            std::env::set_var("TOUCHHLE_TOUCH_MODE", "right-flip-x");

            if cfg!(target_os = "android") {
                // Potato Story/Panic must use the same logical GL shape as desktop:
                // 480x320 landscape, not Android's current 320x480 Cocos viewport.
                std::env::set_var("TOUCHHLE_POTATO_ANDROID_THUMB2_COMPAT", "1");
                std::env::set_var("TOUCHHLE_FORCE_LANDSCAPE_VIEWPORT", "1");
                std::env::set_var("TOUCHHLE_FORCE_LANDSCAPE_RENDERBUFFER", "1");
                std::env::set_var("TOUCHHLE_FORCE_LANDSCAPE_VIEW_BOUNDS", "1");
                std::env::set_var("TOUCHHLE_PRESENT_STRETCH_TO_VIEWPORT", "1");
            }
        }
    }
    // ULTRAHLE_POTATO_LANDSCAPE_END

    let minimum_os_version = bundle.minimum_os_version();
    let required_device_capabilities = bundle.required_device_capabilities();
    let device_family = bundle.device_family_array();

    echo!("App bundle info:");
    echo!("- Display name: {}", bundle.display_name());
    echo!("- Version: {}", bundle.bundle_version());
    echo!("- Identifier: {}", app_id);
    if let Some(canonical_name) = bundle.canonical_bundle_name() {
        echo!("- Internal name (canonical): {}.app", canonical_name);
    } else {
        echo!("- Internal name (from FS): {}.app", bundle.bundle_name());
    }
    echo!(
        "- Minimum OS version: {}",
        minimum_os_version.as_deref().unwrap_or("(not specified)")
    );
    echo!(
        "- Required device capabilities: {}",
        if !required_device_capabilities.is_empty() {
            required_device_capabilities.join(", ")
        } else {
            "(not specified)".to_string()
        }
    );
    echo!(
        "- Device family: {}",
        if !device_family.is_empty() {
            device_family
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            "(not specified)".to_string()
        }
    );
    echo!();

    if let Some(version) = minimum_os_version.as_deref() {
        // Apple's `MinimumOSVersion` Info.plist key follows the standard
        // dotted version format (`MAJOR[.MINOR[.PATCH]]`). Some apps ship
        // with just `"7"`, others with `"7.0"`, others with `"7.0.0"` or
        // even `"6.1.3"` — see Apple's
        // <https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/iPhoneOSKeys.html#//apple_ref/doc/uid/TP40009252-SW33>.
        // Previously we required at least one `.` separator and would
        // `unwrap()` the resulting Option, which panicked when the value
        // was a bare integer (e.g. Swordigo's iPhone OS bundle declares
        // `MinimumOSVersion = 7`). Parse defensively instead and treat any
        // non-numeric / unparseable component as zero, matching how dyld
        // itself tolerates malformed plists.
        let (major_str, minor_str) = match version.split_once('.') {
            Some((maj, rest)) => {
                let minor_str = rest.split_once('.').map_or(rest, |(minor, _patch)| minor);
                (maj, minor_str)
            }
            None => (version, "0"),
        };
        let major: u32 = major_str.parse().unwrap_or(0);
        let minor: u32 = minor_str.parse().unwrap_or(0);
        // Apps targeting up to iOS 9.0 attempt to run silently. Only warn
        // when the deployment target is beyond iOS 9, where breakage from
        // missing post-iOS-9 APIs becomes the rule rather than the exception.
        if major > 9 || (major == 9 && minor > 0) {
            echo!(
                "Warning: app requires OS version {}. touchHLE currently aims \
                 for iOS 2.x–9.0; newer APIs may be missing.",
                version
            );
        }
    }

    if required_device_capabilities.contains(&"opengles-3") {
        echo!(
            "Warning: app requires OpenGL ES 3.0+ support. HyperHLE now routes EAGL OpenGL ES 3 contexts to its GLES 3 backend."
        );
    }

    if just_info {
        return Ok(());
    }

    // Apply options from files
    fn apply_options<F: std::io::Read, P: std::fmt::Display>(
        file: F,
        path: P,
        options: &mut options::Options,
        app_id: &str,
    ) -> Result<(), String> {
        match options::get_options_from_file(file, app_id) {
            Ok(Some(options_string)) => {
                echo!(
                    "Using options from {} for this app: {}",
                    path,
                    options_string
                );
                for option_arg in options_string.split_ascii_whitespace() {
                    match options.parse_argument(option_arg) {
                        Ok(true) => (),
                        Ok(false) => return Err(format!("Unknown option {option_arg:?}")),
                        Err(err) => return Err(format!("Invalid option {option_arg:?}: {err}")),
                    }
                }
            }
            Ok(None) => {
                echo!("No options found for this app in {}", path);
            }
            Err(e) => {
                echo!("Warning: {}", e);
            }
        }
        Ok(())
    }
    let default_options_path = paths::DEFAULT_OPTIONS_FILE;
    match paths::ResourceFile::open(default_options_path) {
        Ok(mut file) => apply_options(file.get(), default_options_path, &mut options, app_id)?,
        Err(err) => echo!("Warning: Could not open {}: {}", default_options_path, err),
    }
    let user_options_path = paths::user_data_base_path().join(paths::USER_OPTIONS_FILE);
    match std::fs::File::open(&user_options_path) {
        Ok(file) => apply_options(file, user_options_path.display(), &mut options, app_id)?,
        Err(err) => echo!(
            "Warning: Could not open {}: {}",
            user_options_path.display(),
            err
        ),
    }
    echo!();
    // Apply command-line options
    for option_arg in option_args {
        let parse_result = options.parse_argument(&option_arg);
        assert!(parse_result == Ok(true));
    }

    ds_draw_env::initialize_after_app_configuration();

    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Environment::new(bundle, fs, options.clone(), app_args.unwrap_or_default())
    }));
    let env = match res {
        Ok(ret) => match ret {
            Ok(env) => env,
            Err(e) => {
                if options.popup_errors {
                    window::show_error_messagebox(None, e.as_str());
                }
                return Err(e);
            }
        },
        Err(e) => {
            if options.popup_errors {
                let error_string = if let Some(s) = e.downcast_ref::<&str>() {
                    s
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s
                } else {
                    "(non-string payload)"
                };
                window::show_error_messagebox(None, error_string);
            }
            std::panic::resume_unwind(e)
        }
    };
    env.run();
    Ok(())
}

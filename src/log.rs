/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Logging and terminal output macros.

use std::fs::File;
use std::sync::LazyLock;

/// Get a handle to the log file. This is only for use by logging macros!
///
/// All the logging macros print to stderr or (on Android) logcat, but this
/// is not convenient for users who aren't accustomed to command-line tools or
/// who don't have access to ADB, so we also write to a log file.
pub fn get_log_file() -> &'static File {
    static LOG_FILE: LazyLock<File> = LazyLock::new(|| {
        File::create(crate::paths::user_data_base_path().join("touchHLE_log.txt")).unwrap()
    });

    &LOG_FILE
}

/// Log verbosity from `TOUCHHLE_DS_LOG_LEVEL`: `quiet`(0) / `normal`(1) /
/// `verbose`(2) / `debug`(3) / `trace`(4). Unset or unknown means `normal`.
///
/// `quiet` silences `log!`; `debug`/`trace` also turn on `log_dbg!` everywhere.
pub fn log_level() -> u8 {
    static LEVEL: LazyLock<u8> = LazyLock::new(|| {
        match std::env::var("TOUCHHLE_DS_LOG_LEVEL")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "quiet" | "none" | "off" => 0,
            "verbose" => 2,
            "debug" => 3,
            "trace" => 4,
            _ => 1,
        }
    });
    *LEVEL
}

/// Whether `log!` output should be emitted at the current log level.
pub fn info_enabled() -> bool {
    log_level() >= 1
}

/// Modules whose `log_dbg!` output is enabled by `verbose`.
const VERBOSE_MODULES: &[&str] = &[
    "touchHLE::window",
    "touchHLE::frameworks::opengles::gles1_on_gl2",
    "touchHLE::frameworks::opengles::gles_guest",
    // Glyph rasterisation: tells us the bitmap size each glyph is drawn at,
    // which is what `TOUCHHLE_DS_CG_GLYPH_DENSITY` scales.
    "touchHLE::font",
];

/// Additional modules enabled by `debug`.
///
/// `touchHLE::mem` and `touchHLE::dyld` are deliberately *not* here: measured on
/// this app they produced ~200k lines in the first few seconds (186 MB in 35 s),
/// which buries everything useful. They are left to `trace`.
const DEBUG_MODULES: &[&str] = &[
    "touchHLE::frameworks::opengles::eagl",
    "touchHLE::frameworks::opengles::gles_generic",
    "touchHLE::frameworks::uikit::ui_view",
    "touchHLE::frameworks::uikit::ui_touch",
    "touchHLE::environment",
];

/// Whether `log_dbg!` output should be emitted for `module`.
///
/// `verbose`/`debug` only switch on a curated set of modules; `trace` turns on
/// everything, which can slow the emulator down enormously and produce a very
/// large log, so use it deliberately.
pub fn dbg_enabled(module: &str) -> bool {
    if ENABLED_MODULES.contains(&module) {
        return true;
    }
    match log_level() {
        // Level 4 ("trace") is meant to mean "every module", including
        // `touchHLE::mem` and `touchHLE::dyld`. Measured 2026-10-09: that makes
        // the emulator spin at ~100% of a core and emit **zero** bytes of log -
        // 3 lines / 0 KB in 150 s, and even the internal `touchHLE_log.txt`
        // stays empty (an earlier note claimed ~186 MB in 35 s, so something on
        // that path is now wedged rather than merely verbose). Until that is
        // diagnosed, treat `trace` as `debug`: the caller gets the richest log
        // that is actually usable instead of a silent hang.
        4 | 3 => DEBUG_MODULES.contains(&module) || VERBOSE_MODULES.contains(&module),
        2 => VERBOSE_MODULES.contains(&module),
        _ => false,
    }
}

/// Prints a log message. Use this for errors or warnings.
///
/// The message is prefixed with the module path, so it is clear where it comes
/// from. Suppressed entirely when `TOUCHHLE_DS_LOG_LEVEL=quiet`.
macro_rules! log {
    ($($arg:tt)+) => {
        if $crate::log::info_enabled() {
            echo!("{}: {}", module_path!(), format_args!($($arg)+))
        }
    }
}

/// Same as [log], but silently fails on panic instead of
/// panicking.
macro_rules! log_no_panic {
    ($($arg:tt)+) => {
        echo_no_panic!("{}: {}", module_path!(), format_args!($($arg)+));
    }
}

/// Like [log], but prints the message only if debugging is enabled for the
/// module where it is used. This can be used for verbose things only needed
/// when debugging.
macro_rules! log_dbg {
    ($($arg:tt)+) => {
        if $crate::log::dbg_enabled(module_path!()) {
            log!($($arg)*);
        }
    }
}

/// Like [log], but messages only log once and cannot have formatting.
/// To be used for log messages that are known to spam the log file (like those
/// logged every frame).
macro_rules! log_once {
    ($msg:literal) => {{
        static LOG_ONCE: std::sync::Once = std::sync::Once::new();
        LOG_ONCE.call_once(|| {
            log!("{} [this log will only be shown once]", $msg);
        });
    }};
}

/// Print a message (with implicit newline). This should be used for all
/// touchHLE output that isn't coming from the app itself.
///
/// Prefer use [log] or [log_dbg] for errors and warnings during emulation.
macro_rules! echo {
    ($($arg:tt)+) => {
        {
            let formatted_str = format!($($arg)+);

            #[cfg(target_os = "android")]
            {
                sdl2::log::log(&formatted_str);
            }
            #[cfg(not(target_os = "android"))]
            eprintln!("{}", formatted_str);

            use std::io::Write;
            let mut log_file = $crate::log::get_log_file();
            let _ = log_file.write_all(formatted_str.as_bytes());
            let _ = log_file.write_all(b"\n");
        }
    };
    () => {
        {
            #[cfg(target_os = "android")]
            {
                sdl2::log::log("");
            }
            #[cfg(not(target_os = "android"))]
            eprintln!("");

            use std::io::Write;
            let _ = $crate::log::get_log_file().write_all(b"\n");
        }
    }
}

/// Same as [echo], but silently fails on panic instead of
/// panicking.
macro_rules! echo_no_panic {
    ($($arg:tt)*) => {
        {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                echo!($($arg)*);
            }));
        }
    }
}

pub const ENABLED_MODULES: &[&str] = &[];

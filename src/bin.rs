/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
// Allow the crate to have a non-snake-case name (touchHLE).
// This also allows items in the crate to have non-snake-case names.
#![allow(non_snake_case)]

/// Dead Space compat: make the process DPI-aware *before* SDL creates a window.
///
/// WHY: on a Windows desktop scaled to 150% the emulator asked SDL for a
/// 960x640 window but Windows silently virtualised it to 1464x1019 physical
/// pixels. The renderer only ever draws its 960x640 viewport, so the extra
/// right/bottom strips kept stale pixels (the user's "digits and letters are
/// wrong": stray glyphs along the right and left edge of every menu), and the
/// whole game image was non-uniformly stretched (1.525x horizontally vs
/// 1.592x vertically) by the DWM. Declaring DPI awareness makes the window
/// exactly 960x640 and the pixels 1:1 (measured 2026-10-10: host NVIDIA driver
/// vs the VM's unscaled llvmpipe window, which never showed the stray glyphs).
///
/// No-op on any other platform, and harmless if called twice.
#[cfg(windows)]
fn make_process_dpi_aware() {
    #[link(name = "user32")]
    extern "system" {
        fn SetProcessDPIAware() -> i32;
    }
    unsafe {
        // Failure just means the manifest already set an awareness level.
        let _ = SetProcessDPIAware();
    }
}

#[cfg(not(windows))]
fn make_process_dpi_aware() {}

/// Dead Space compat: keep a Chinese/Japanese IME from swallowing the letter
/// keys the game needs.
///
/// WHY: with an IME active Windows routes plain letters (W/A/S/D, P, ...) into
/// the IME's composition instead of delivering WM_KEYDOWN to the window, so the
/// emulator never sees them - measured 2026-10-11 from the user's own run log:
/// after pressing W (2 s), P and F5, exactly ONE key event arrived
/// (`Keycode(1073741886)` = F5); the letters were gone, while 22 mouse events
/// came through. `ImmDisableIME(-1)` disables the IME for every thread of the
/// process (no window handle needed, so nothing else has to change) and letters
/// arrive as ordinary key events again.
///
/// Opt out with `TOUCHHLE_DS_OS_IME=1` if a CJK IME is wanted for text entry
/// inside the guest (e.g. naming a save profile).
#[cfg(windows)]
fn disable_os_ime() {
    if std::env::var_os("TOUCHHLE_DS_OS_IME").is_some() {
        return;
    }
    #[link(name = "imm32")]
    extern "system" {
        fn ImmDisableIME(id_thread: u32) -> i32;
    }
    unsafe {
        // 0xFFFFFFFF == (DWORD)-1: all threads of this process.
        let ok = ImmDisableIME(0xFFFF_FFFF);
        eprintln!("[DSime] ImmDisableIME(-1) -> {ok} (letters no longer go to the OS IME)");
    }
}

#[cfg(not(windows))]
fn disable_os_ime() {}

fn main() -> Result<(), String> {
    make_process_dpi_aware();
    disable_os_ime();
    let result=touchHLE::main(std::env::args());
    touchHLE::finish_play_diagnostics(result.is_ok());
    result
}

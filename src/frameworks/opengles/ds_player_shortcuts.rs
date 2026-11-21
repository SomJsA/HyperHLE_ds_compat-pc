//! Native player shortcuts, separate from experimental RIG menu automation.
//!
//! These are the original game's actions, including its animation, busy-state
//! guards and tutorial flags. Never replace them with camera rotation, guessed
//! double taps, or writes to the player's/tutorial's state.
use super::{
    ds_current_hud_player, ds_dispatch_action_forwarder, ds_native_profile,
    ds_refresh_action_forwarders, ekey_fwds, DsNativeProfile,
};
use crate::abi::{CallFromHost, GuestFunction};
use crate::mem::ConstVoidPtr;
use crate::Environment;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

fn word(env: &Environment, address: u32) -> Option<u32> {
    let bytes = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(address), 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn supported(env: &Environment) -> bool {
    // Exact ARM build/layout. Check before even calling GetHud; another guest
    // must never execute these addresses because a desktop key was pressed.
    [
        (0x0003_0474, 0xe92d_4080), // GetHud
        (0x0003_ae60, 0xe590_01a4), // CanUseLocator
        (0x0003_aee8, 0xe5d0_11a4), // CanQuickTurn
        (0x0004_af1c, 0xe92d_40f0), // DoLocatorMeshAnim(bool)
        (0x000e_55cc, 0xe92d_4080), // InputForwarderTapToAction::SendAction
        (0x002f_7e64, 0xe92d_4080), // ObjectivePopup::ResetPopup
    ]
    .iter()
    .all(|&(address, expected)| word(env, address) == Some(expected))
}

struct LocatorLayout {
    can_use: (u32, u32),
    animate: (u32, u32),
    reset_popup: (u32, u32),
}

fn locator_layout(env: &Environment) -> Option<LocatorLayout> {
    // Each known ARMv7 Hud::OnStringIdEvent invokes these methods for its
    // marker button and embeds ObjectivePopup at Hud+0xf08.
    let (can_use, animate, reset_popup) = match ds_native_profile(env)?.get_hud {
        0x0003_0474 if supported(env) => (
            (0x0003_ae60, 0xe590_01a4),
            (0x0004_af1c, 0xe92d_40f0),
            (0x002f_7e64, 0xe92d_4080),
        ),
        0x0002_a73c => (
            (0x0003_4f44, 0xe590_01a4),
            (0x0004_4adc, 0xe92d_40f0),
            (0x002c_468c, 0xe92d_4080),
        ),
        0x0002_1f18 if env.bins[0]
            .executable_sha256
            .is_some_and(|digest| super::DS_IPAD_COMBAT_IMAGES.contains(&digest)) => (
            (0x0002_c720, 0xe590_01a4),
            (0x0003_c2b8, 0xe92d_40f0),
            (0x002c_52b8, 0xe92d_4080),
        ),
        0x0003_62f0 => (
            (0x0004_9610, 0xe590_21a4),
            (0x0005_ee90, 0xe92d_40f0),
            (0x0039_54d0, 0xeaff_fe62),
        ),
        0x0006_1fa0 => (
            (0x0007_4e00, 0xe590_21a4),
            (0x0008_5380, 0xe92d_40f0),
            (0x0033_0bf0, 0xeaff_fe66),
        ),
        _ => return None,
    };
    [can_use, animate, reset_popup]
        .iter()
        .all(|&(address, expected)| word(env, address) == Some(expected))
        .then_some(LocatorLayout { can_use, animate, reset_popup })
}

fn live_player(env: &mut Environment) -> Option<(u32, u32)> {
    if crate::ds_frontend_controller::capture_menu()
        || crate::ds_frontend_controller::capture_scene_dialog()
    {
        return None;
    }
    let (hud, player) = ds_current_hud_player(env)?;
    // Ordinary closed HUD only: no input behind pause, shop, workbench,
    // confirmation or RIG. No pending action survives closing those screens.
    (word(env, hud + 0x234) == Some(0)).then_some((hud, player))
}

pub(super) fn locator(env: &mut Environment) {
    let Some(layout) = locator_layout(env) else {
        echo!("[DSshortcut] locator ignored: unsupported native layout");
        return;
    };
    let Some((hud, player)) = live_player(env) else {
        echo!("[DSshortcut] locator ignored: no active gameplay player");
        return;
    };
    let before = word(env, player + 0x208).unwrap_or(u32::MAX);
    // The original method writes other flags *before* rejecting busy states.
    // Gate first so a shortcut during a cutscene/reload has no side effects.
    if !matches!(before, 0 | 18) {
        echo!("[DSshortcut] locator ignored: player state={before}");
        return;
    }
    let allowed: u32 =
        GuestFunction::from_addr_and_thumb_flag(layout.can_use.0, false).call_from_host(env, (player,));
    if allowed == 0 {
        // Some zero-g deck transitions intentionally have no locator mesh.
        // The native touch button still replays the current objective label,
        // so keep the target popup independent from route eligibility.
        let target_reset = reset_target_popup(env, hud, layout.reset_popup.0);
        echo!(
            "[DSshortcut] locator route unavailable; target_reset={target_reset}"
        );
        return;
    }
    // Same player method/argument as BTN_MARKER, with no BTN_RIG event, wheel
    // preparation or HUD state transition. Native code owns flag 0x400.
    let accepted: u32 = GuestFunction::from_addr_and_thumb_flag(layout.animate.0, false)
        .call_from_host(env, (player, 0u32));
    // Preserve BTN_MARKER's original objective text popup even when the
    // current world has no route pointer (the native deck case). This native
    // presentation object does not open or navigate a HUD/RIG menu.
    let target_reset = accepted != 0 && reset_target_popup(env, hud, layout.reset_popup.0);
    echo!(
        "[DSshortcut] locator native accepted={} target_reset={} player={:#x} state={}->{} flags={:#x} hud={}",
        accepted,
        target_reset,
        player,
        before,
        word(env, player + 0x208).unwrap_or(u32::MAX),
        word(env, player + 0x204).unwrap_or(0),
        word(env, hud + 0x234).unwrap_or(u32::MAX)
    );
}

fn reset_target_popup(env: &mut Environment, hud: u32, reset_popup: u32) -> bool {
    if hud < 0x1000 || word(env, hud + 0xf08).is_none() {
        return false;
    }
    let _: () = GuestFunction::from_addr_and_thumb_flag(reset_popup, false)
        .call_from_host(env, (hud + 0xf08,));
    // A zero-G deck may have no locator mesh. Native locator still shows the
    // current objective, so replay the last target independently of the route.
    crate::ds_objective_overlay::force_target_replay();
    true
}

fn turn_forwarder(env: &mut Environment, player: u32, action_forwarder_vptr: u32) -> Option<u32> {
    static CACHED: AtomicU64 = AtomicU64::new(0);
    let valid = |env: &Environment, fwd: u32| {
        fwd != 0
            && word(env, fwd) == Some(action_forwarder_vptr)
            && word(env, fwd + 0x1c) == Some(8)
            && word(env, fwd + 8) == Some(player)
            && env
                .mem
                .get_bytes_fallible(ConstVoidPtr::from_bits(fwd + 4), 1)
                .is_some_and(|v| v[0] != 0)
    };
    let cached = CACHED.load(Relaxed);
    let fwd = cached as u32;
    if (cached >> 32) as u32 == player && valid(env, fwd) {
        return Some(fwd);
    }
    // Share the interaction cache on the first request, then cache this exact
    // player's enabled forwarder. Validate every use across scene/save changes.
    // Never use ds_send_action's fallback to an unrelated target object.
    for refresh in [false, true] {
        if refresh {
            ds_refresh_action_forwarders(env, 0);
        }
        let objects = ekey_fwds().lock().unwrap().clone();
        if let Some(fwd) = objects.into_iter().find(|&fwd| valid(env, fwd)) {
            CACHED.store(((player as u64) << 32) | fwd as u64, Relaxed);
            return Some(fwd);
        }
    }
    CACHED.store(0, Relaxed);
    None
}

fn legacy_turn_supported(env: &Environment, profile: DsNativeProfile) -> bool {
    // The older ARMv7 handlers inline the turn preconditions in action 8.
    // Validate its jump-table entry and the instructions writing state 2 and
    // the tutorial flag. These profiles already verify the loaded executable.
    if profile.combat.is_none() {
        return false;
    }
    let (slot, destination, transition, signature) = match profile.player_on_event {
        0x0004_d93c => (0x0004_dd18, 0x0000_0410, 0x0004_e25c,
                        [0xe596_4204, 0xe3a0_1002, 0xe384_4a01, 0xe586_4204]),
        0x0004_5118 => (0x0004_54f4, 0x0000_0410, 0x0004_5a38,
                        [0xe596_4204, 0xe3a0_1002, 0xe384_4a01, 0xe586_4204]),
        0x0006_d700 => (0x0006_d840, 0xea00_017b, 0x0006_def0,
                        [0xe596_2204, 0xe3a0_1002, 0xe382_0a01, 0xe586_0204]),
        0x0009_3020 => (0x0009_3160, 0x0009_3714, 0x0009_3834,
                        [0xe596_1204, 0xe381_0a01, 0xe3a0_1002, 0xe586_0204]),
        _ => return false,
    };
    word(env, slot) == Some(destination)
        && signature.iter().enumerate().all(|(i, &expected)| {
            word(env, transition + i as u32 * 4) == Some(expected)
        })
}

fn quick_turn(env: &mut Environment) {
    let Some(profile) = ds_native_profile(env) else {
        echo!("[DSshortcut] quick-turn ignored: unsupported native layout");
        return;
    };
    let current = profile.get_hud == 0x0003_0474;
    let has_turn = if current { supported(env) } else { legacy_turn_supported(env, profile) };
    if !has_turn {
        echo!("[DSshortcut] quick-turn ignored: unsupported native layout");
        return;
    }
    let Some((hud, player)) = live_player(env) else {
        echo!("[DSshortcut] quick-turn ignored: no active gameplay player");
        return;
    };
    let before = word(env, player + 0x208).unwrap_or(u32::MAX);
    if before != 0 {
        echo!("[DSshortcut] quick-turn ignored: player state={before}");
        return;
    }
    if current {
        let allowed: u32 =
            GuestFunction::from_addr_and_thumb_flag(0x0003_aee8, false).call_from_host(env, (player,));
        if allowed == 0 {
            echo!("[DSshortcut] quick-turn ignored: CanQuickTurn rejected");
            return;
        }
    }
    let Some(fwd) = turn_forwarder(env, player, profile.action_forwarder_vptr) else {
        echo!("[DSshortcut] quick-turn ignored: no enabled current-player action-8 forwarder");
        return;
    };
    // OnEvent enters native state 2 and sets the action flag checked by the
    // quick-turn tutorial. Only the current image exports the sender.
    if current {
        let _: () =
            GuestFunction::from_addr_and_thumb_flag(0x000e_55cc, false).call_from_host(env, (fwd,));
    } else if ds_dispatch_action_forwarder(env, fwd, 8).is_none() {
        echo!("[DSshortcut] quick-turn ignored: native action dispatch rejected");
        return;
    }
    echo!(
        "[DSshortcut] quick-turn native player={:#x} fwd={:#x} state={}->{} flags={:#x} hud={}",
        player,
        fwd,
        before,
        word(env, player + 0x208).unwrap_or(u32::MAX),
        word(env, player + 0x204).unwrap_or(0),
        word(env, hud + 0x234).unwrap_or(u32::MAX)
    );
}

pub(super) fn tick(env: &mut Environment) {
    let turn = crate::window::DS_QUICK_TURN_REQUEST.swap(false, Relaxed);
    let locate = crate::window::DS_LOCATOR_REQUEST.swap(false, Relaxed);
    if turn {
        quick_turn(env);
    }
    if locate {
        locator(env);
    }
}

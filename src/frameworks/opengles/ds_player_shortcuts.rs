//! Native player shortcuts, separate from experimental RIG menu automation.
//!
//! These are the original game's actions, including its animation, busy-state
//! guards and tutorial flags. Never replace them with camera rotation, guessed
//! double taps, or writes to the player's/tutorial's state.
use super::{ds_current_hud_player, ds_refresh_action_forwarders, ekey_fwds};
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

fn live_player(env: &mut Environment) -> Option<(u32, u32)> {
    if !supported(env)
        || crate::ds_frontend_controller::capture_menu()
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
        GuestFunction::from_addr_and_thumb_flag(0x0003_ae60, false).call_from_host(env, (player,));
    if allowed == 0 {
        // Some zero-g deck transitions intentionally have no locator mesh.
        // The native touch button still replays the current objective label,
        // so keep the target popup independent from route eligibility.
        let target_reset = reset_target_popup(env, hud);
        echo!(
            "[DSshortcut] locator route unavailable; target_reset={target_reset}"
        );
        return;
    }
    // Same player method/argument as BTN_MARKER, with no BTN_RIG event, wheel
    // preparation or HUD state transition. Native code owns flag 0x400.
    let accepted: u32 = GuestFunction::from_addr_and_thumb_flag(0x0004_af1c, false)
        .call_from_host(env, (player, 0u32));
    // Preserve BTN_MARKER's original objective text popup even when the
    // current world has no route pointer (the native deck case). This native
    // presentation object does not open or navigate a HUD/RIG menu.
    let target_reset = accepted != 0 && reset_target_popup(env, hud);
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

fn reset_target_popup(env: &mut Environment, hud: u32) -> bool {
    if hud < 0x1000 || word(env, hud + 0xf08).is_none() {
        return false;
    }
    let _: () = GuestFunction::from_addr_and_thumb_flag(0x002f_7e64, false)
        .call_from_host(env, (hud + 0xf08,));
    // A zero-G deck may have no locator mesh. Native locator still shows the
    // current objective, so replay the last target independently of the route.
    crate::ds_objective_overlay::force_target_replay();
    true
}

fn turn_forwarder(env: &mut Environment, player: u32) -> Option<u32> {
    static CACHED: AtomicU64 = AtomicU64::new(0);
    let valid = |env: &Environment, fwd: u32| {
        fwd != 0
            && word(env, fwd) == Some(0x00a7_d1e8)
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

fn quick_turn(env: &mut Environment) {
    let Some((hud, player)) = live_player(env) else {
        echo!("[DSshortcut] quick-turn ignored: no active gameplay player");
        return;
    };
    let before = word(env, player + 0x208).unwrap_or(u32::MAX);
    if before != 0 {
        echo!("[DSshortcut] quick-turn ignored: player state={before}");
        return;
    }
    let allowed: u32 =
        GuestFunction::from_addr_and_thumb_flag(0x0003_aee8, false).call_from_host(env, (player,));
    if allowed == 0 {
        echo!("[DSshortcut] quick-turn ignored: CanQuickTurn rejected");
        return;
    }
    let Some(fwd) = turn_forwarder(env, player) else {
        echo!("[DSshortcut] quick-turn ignored: no enabled current-player action-8 forwarder");
        return;
    };
    // The real forwarder constructs and dispatches the original C++ event.
    // OnEvent enters native state 2 and sets action flag 0x1000, which
    // TutorialAnim::OnUpdateQuickTurnTutorial tests at ARM 0x002cb320.
    let _: () =
        GuestFunction::from_addr_and_thumb_flag(0x000e_55cc, false).call_from_host(env, (fwd,));
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

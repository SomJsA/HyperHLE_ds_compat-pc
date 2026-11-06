//! Explicit isolated stress-test aid, never a gameplay default or performance fix.
//! All guest/GL/input work remains on its owning Environment thread.
use crate::{
    abi::{CallFromHost, GuestFunction},
    cpu::Cpu,
    mem::{ConstVoidPtr, MutPtr},
    Environment,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
        OnceLock,
    },
    time::Instant,
};
fn event(code: &'static str, values: [u32; 4]) {
    crate::ds_diagnostics::event(code, values.map(u64::from));
}
const HEALTH: u32 = 0x41748;
const SURVIVAL: u32 = 0x306210;
const PUSH: u32 = 0xe92d40f0;
const BKPT: u32 = 0xe1200070;
const PLAYER_VPTR: u32 = 0xa7a4f8;
const LEASE_US: u64 = 5_000_000;
const MAX_OBJECTS: u32 = 4096;
const MAX_GROUPS: usize = 128;
static PERMITTED: AtomicBool = AtomicBool::new(false);
static DEADLINE: AtomicU64 = AtomicU64::new(0);
fn clock() -> u64 {
    static BASE: OnceLock<Instant> = OnceLock::new();
    BASE.get_or_init(Instant::now)
        .elapsed()
        .as_micros()
        .min(u64::MAX as u128) as u64
}
fn active(now: u64, deadline: u64) -> bool {
    deadline != 0 && now < deadline
}
pub(crate) fn request(on: bool) -> bool {
    if !PERMITTED.load(Relaxed) {
        return false;
    }
    DEADLINE.store(
        if on {
            clock().saturating_add(LEASE_US)
        } else {
            0
        },
        Relaxed,
    );
    true
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Attack {
    Off,
    Shoot,
    Melee,
    #[default]
    Mixed,
}
#[derive(Default)]
pub(crate) struct State {
    armed: bool,
    active: bool,
    last_frame: u64,
    next_tick: u64,
    next_world: u64,
    next_status: u64,
    next_ammo: u64,
    next_action: u64,
    started: u64,
    attack: Attack,
    auto_qte: bool,
    shooting: bool,
    mode: u32,
    health_calls: u64,
    prevented: u64,
    qte: u64,
    shots: u64,
    melee: u64,
    refills: u64,
    mode_updates: u64,
    verified_enemies: BTreeSet<u32>,
    seen_enemies: BTreeSet<(u32, u32)>,
}
fn word(env: &Environment, at: u32) -> Option<u32> {
    if at < 0x1000 {
        return None;
    }
    let b = env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(at), 4)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}
fn byte(env: &Environment, at: u32) -> Option<u8> {
    if at < 0x1000 {
        return None;
    }
    env.mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(at), 1)?
        .first()
        .copied()
}
fn fingerprint(env: &Environment, at: u32, len: u32) -> Option<u64> {
    let b = env
        .mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(at), len)?;
    (b.len() == len as usize).then(|| {
        b.iter().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ *b as u64).wrapping_mul(0x100000001b3)
        })
    })
}
fn private_configuration() -> bool {
    if !cfg!(all(windows, target_arch = "x86_64"))
        || std::env::var("TOUCHHLE_DS_TEST_ASSIST").as_deref() != Ok("1")
        || std::env::var_os("TOUCHHLE_DS_CONTROL_INBOX").is_none()
        || std::env::var("TOUCHHLE_DS_BACKGROUND_TEST").as_deref() != Ok("1")
    {
        return false;
    }
    let Some(root) = std::env::var_os("TOUCHHLE_USER_DATA_DIR") else {
        return false;
    };
    let root = std::path::PathBuf::from(root);
    if !root.is_absolute() || root.canonicalize().is_err() {
        return false;
    }
    let marker = root.join(".ds-test-assist");
    if !std::fs::metadata(&marker).is_ok_and(|m| m.is_file() && m.len() <= 128) {
        return false;
    }
    std::fs::read_to_string(marker).is_ok_and(|s| s == "R15-ISOLATED-TEST-ASSIST-v1\n")
}
const ENEMIES: &[(u32, u64, &str)] = &[
    (0x00a85068, 9749925630930148821, "GameObjectBoss"),
    (0x00a7c798, 8025945760561008549, "GameObjectBrute"),
    (0x00a7dc18, 2973482214541093690, "GameObjectSwarm"),
    (0x00a7cbd8, 17857155716422102964, "GameObjectLurker"),
    (0x00a7ce38, 15595939655028782925, "GameObjectSlasher"),
    (0x00a81618, 7839809864861990289, "GameObjectExploder"),
    (0x00a7d988, 1626631794720047458, "GameObjectPregnant"),
    (0x00a7feb8, 499422607287305225, "GameObjectNecromorph"),
    (0x00a87cf8, 7947060179550103861, "GameObjectBossReactor"),
];
pub(crate) fn prime(env: &mut Environment) {
    if !private_configuration() || env.bundle.bundle_identifier() != "com.ea.deadspace.bv" {
        return;
    }
    let known = fingerprint(env, HEALTH, 628) == Some(9316050825940402288)
        && fingerprint(env, SURVIVAL, 668) == Some(2357791680537570417)
        && fingerprint(env, 0xdb38, 84) == Some(662129978744316938)
        && fingerprint(env, 0x701d8, 52) == Some(16618532455101650939)
        && [HEALTH, SURVIVAL]
            .iter()
            .all(|at| word(env, *at) == Some(PUSH));
    if !known {
        echo!("[DStest-assist] unknown native instructions; disabled");
        return;
    }
    let verified = ENEMIES
        .iter()
        .filter(|(vt, h, _)| fingerprint(env, *vt - 8, 0x118) == Some(*h))
        .map(|(vt, _, _)| *vt)
        .collect();
    for at in [HEALTH, SURVIVAL] {
        env.mem.write(MutPtr::<u32>::from_bits(at), BKPT);
        env.cpu.invalidate_cache_range(at, 4);
    }
    let attack = match std::env::var("TOUCHHLE_DS_TEST_ASSIST_ATTACK").as_deref() {
        Ok("off") => Attack::Off,
        Ok("shoot") => Attack::Shoot,
        Ok("melee") => Attack::Melee,
        _ => Attack::Mixed,
    };
    let s = &mut env.framework_state.opengles.ds_test_assist;
    s.armed = true;
    s.attack = attack;
    s.auto_qte = std::env::var("TOUCHHLE_DS_TEST_ASSIST_QTE").as_deref() != Ok("0");
    s.verified_enemies = verified;
    PERMITTED.store(true, Relaxed);
    event(
        "test_assist_config",
        [1, LEASE_US as u32, MAX_OBJECTS, MAX_GROUPS as u32],
    );
    echo!("[DStest-assist] isolated marker verified; default inactive,5s heartbeat lease; SetHealth lock + native QTE/combat; world objects<=4096/groups<=128; no pixel scan");
}
fn clamp_requested(current: u32, requested: u32) -> Option<u32> {
    let current_value = f32::from_bits(current);
    let wanted = f32::from_bits(requested);
    (current_value.is_finite()
        && current_value > 0.0
        && current_value <= 1_000_000.0
        && wanted.is_finite()
        && wanted < current_value)
        .then_some(current)
}
fn disable(env: &mut Environment) {
    DEADLINE.store(0, Relaxed);
    PERMITTED.store(false, Relaxed);
    for at in [HEALTH, SURVIVAL] {
        if word(env, at) == Some(BKPT) {
            env.mem.write(MutPtr::<u32>::from_bits(at), PUSH);
        } else {
            echo!(
                "[DStest-assist] restore conflict at={:#x}; unknown word retained",
                at
            );
        }
        env.cpu.invalidate_cache_range(at, 4);
    }
    if env.framework_state.opengles.ds_test_assist.active && env.window.is_some() {
        release(env);
    }
    env.framework_state.opengles.ds_test_assist.armed = false;
}
pub(crate) fn handle_breakpoint(env: &mut Environment) -> bool {
    let entry = env.cpu.regs()[Cpu::PC];
    if !env.framework_state.opengles.ds_test_assist.armed
        || ![HEALTH, SURVIVAL].contains(&entry)
        || word(env, entry) != Some(BKPT)
    {
        return false;
    }
    let owner = env.cpu.regs()[0];
    let arg = env.cpu.regs()[1];
    let requested = if entry == HEALTH
        && active(clock(), DEADLINE.load(Relaxed))
        && word(env, owner) == Some(PLAYER_VPTR)
    {
        word(env, owner.saturating_add(0x258)).and_then(|old| clamp_requested(old, arg))
    } else {
        None
    };
    if !crate::ds_guest_skin_probe::replay_push(&mut env.cpu, &mut env.mem, entry) {
        disable(env);
        env.cpu
            .branch(GuestFunction::from_addr_and_thumb_flag(entry, false));
        echo!("[DStest-assist] unsupported CPU/stack; original restored");
        return true;
    }
    let s = &mut env.framework_state.opengles.ds_test_assist;
    if entry == HEALTH {
        s.health_calls += 1;
        if let Some(value) = requested {
            env.cpu.regs_mut()[1] = value;
            s.prevented += 1;
            if s.prevented <= 4 || s.prevented.is_power_of_two() {
                echo!(
                    "[DStest-assist] health lock count={} player={:#x} requested={} retained={}",
                    s.prevented,
                    owner,
                    f32::from_bits(arg),
                    f32::from_bits(value)
                );
            }
        }
    } else {
        s.mode = owner;
        s.mode_updates += 1;
    }
    true
}
fn release(env: &mut Environment) {
    use crate::window::ds_fire_control::{self, Input, Source};
    ds_fire_control::enqueue(Input::Release(Source::Diagnostic));
    env.window_mut().ds_cancel_inputs();
    crate::window::DS_CONTEXT_COMBAT_REQUEST.store(false, Relaxed);
}
fn world_snapshot(env: &mut Environment, world: u32, frame: u32) {
    let begin = word(env, world.saturating_add(0x44));
    let end = word(env, world.saturating_add(0x48));
    let Some((begin, total)) = begin.zip(end).and_then(|(b, e)| {
        let n = e.checked_sub(b)?;
        (b >= 0x1000 && n % 8 == 0).then_some((b, n / 8))
    }) else {
        event("assist_world_invalid", [world, 0, 0, frame]);
        return;
    };
    let count = total.min(MAX_OBJECTS);
    let mut groups = BTreeMap::<(u32, u32), (u32, u32)>::new();
    let mut unreadable = 0u32;
    let mut overflow = 0u32;
    let mut enemies = 0u32;
    let mut alive_enemies = 0u32;
    for i in 0..count {
        let Some(obj) = begin.checked_add(i * 8).and_then(|at| word(env, at)) else {
            unreadable += 1;
            continue;
        };
        let Some((vt, kind)) =
            word(env, obj).zip(obj.checked_add(0x28).and_then(|at| word(env, at)))
        else {
            unreadable += 1;
            continue;
        };
        if obj.checked_add(0x10).and_then(|at| word(env, at)) != Some(world) {
            unreadable += 1;
            continue;
        }
        let known = env
            .framework_state
            .opengles
            .ds_test_assist
            .verified_enemies
            .contains(&vt);
        let alive = if known {
            enemies += 1;
            let method = word(env, vt + 0x10c).unwrap_or(0);
            if (0x1000..0x950000).contains(&(method & !1)) {
                let value: u32 =
                    GuestFunction::from_addr_with_thumb_bit(method).call_from_host(env, (obj,));
                (value != 0 && byte(env, obj + 0x40).is_some_and(|f| f & 1 == 0)) as u32
            } else {
                0
            }
        } else {
            0
        };
        alive_enemies += alive;
        if groups.len() >= MAX_GROUPS && !groups.contains_key(&(kind, vt)) {
            overflow += 1;
            continue;
        }
        let g = groups.entry((kind, vt)).or_default();
        g.0 += 1;
        g.1 += alive;
        if known
            && env
                .framework_state
                .opengles
                .ds_test_assist
                .seen_enemies
                .len()
                < 128
            && env
                .framework_state
                .opengles
                .ds_test_assist
                .seen_enemies
                .insert((kind, vt))
        {
            event("assist_new_enemy", [kind, vt, alive, frame]);
            let name = ENEMIES
                .iter()
                .find(|(v, _, _)| *v == vt)
                .map(|(_, _, n)| *n)
                .unwrap_or("unknown");
            echo!(
                "[DStest-assist] new enemy class={} type={} vptr={:#x} alive={}",
                name,
                kind,
                vt,
                alive
            );
        }
    }
    event("assist_world", [total, enemies, alive_enemies, frame]);
    event(
        "assist_world_gaps",
        [total - count, unreadable, overflow, frame],
    );
    for ((kind, vt), (count, alive)) in groups {
        event("assist_object_type", [kind, vt, count, frame]);
        if env
            .framework_state
            .opengles
            .ds_test_assist
            .verified_enemies
            .contains(&vt)
        {
            event("assist_enemy_alive", [kind, vt, alive, frame]);
        }
    }
}
pub(crate) fn observe(env: &mut Environment) {
    let s = &env.framework_state.opengles.ds_test_assist;
    if !s.armed {
        return;
    }
    let frame = crate::window::DS_FRAME_COUNTER.load(Relaxed);
    if s.last_frame == frame {
        return;
    }
    env.framework_state.opengles.ds_test_assist.last_frame = frame;
    let now = clock();
    let on = active(now, DEADLINE.load(Relaxed));
    let s = &mut env.framework_state.opengles.ds_test_assist;
    if s.active != on {
        s.active = on;
        s.shooting = false;
        if on {
            s.started = now;
        } else {
            release(env);
        }
        event("assist_lease", [on as u32, frame as u32, 0, 0]);
        echo!("[DStest-assist] lease active={} frame={}", on, frame);
    }
    if !on || now < env.framework_state.opengles.ds_test_assist.next_tick {
        return;
    }
    env.framework_state.opengles.ds_test_assist.next_tick = now + 100_000;
    let Some((_hud, player)) = super::ds_current_hud_player(env) else {
        return;
    };
    let state = word(env, player + 0x208).unwrap_or(u32::MAX);
    let hp = word(env, player + 0x258).unwrap_or(0);
    if now >= env.framework_state.opengles.ds_test_assist.next_status {
        let s = &mut env.framework_state.opengles.ds_test_assist;
        s.next_status = now + 1_000_000;
        event(
            "assist_player",
            [
                state,
                hp,
                s.prevented.min(u32::MAX as u64) as u32,
                frame as u32,
            ],
        );
        event(
            "assist_actions",
            [
                s.qte as u32,
                s.shots as u32,
                s.melee as u32,
                s.refills as u32,
            ],
        );
        let mode = s.mode;
        let native_mode = word(env, mode.saturating_add(0x88)).unwrap_or(u32::MAX);
        let native_clock = word(env, mode.saturating_add(0x90)).unwrap_or(u32::MAX);
        let native_elapsed = word(env, mode.saturating_add(0x94)).unwrap_or(u32::MAX);
        event(
            "assist_survival_clock",
            [native_mode, native_clock, native_elapsed, frame as u32],
        );
        echo!("[DStest-assist] frame={} state={} hp={} prevented={} native_mode={} native_clock={} native_elapsed={}",frame,state,f32::from_bits(hp),env.framework_state.opengles.ds_test_assist.prevented,native_mode,native_clock,native_elapsed);
    }
    if now >= env.framework_state.opengles.ds_test_assist.next_world {
        env.framework_state.opengles.ds_test_assist.next_world = now + 5_000_000;
        if let Some(world) = word(env, player + 0x10) {
            world_snapshot(env, world, frame as u32);
        }
    }
    if state == 9 || state == 10 {
        if env.framework_state.opengles.ds_test_assist.auto_qte {
            if let Some(_result) = super::ds_try_current_prompt_gesture(env) {
                env.framework_state.opengles.ds_test_assist.qte += 1;
            }
        }
        return;
    }
    if state == 26 || state == 29 || crate::window::ds_pc_input::station_owns_input(state) {
        return;
    }
    let s = &env.framework_state.opengles.ds_test_assist;
    if now < s.next_action || s.attack == Attack::Off {
        return;
    }
    let shoot = match s.attack {
        Attack::Shoot => true,
        Attack::Mixed => (now - s.started) % 10_000_000 < 6_000_000,
        _ => false,
    };
    let changing = s.shooting != shoot;
    env.framework_state.opengles.ds_test_assist.shooting = shoot;
    env.framework_state.opengles.ds_test_assist.next_action =
        now + if shoot { 250_000 } else { 500_000 };
    if changing {
        crate::window::DS_AIM_REQUEST.store(shoot as i32, Relaxed);
        if !shoot {
            crate::window::ds_fire_control::enqueue(
                crate::window::ds_fire_control::Input::Release(
                    crate::window::ds_fire_control::Source::Diagnostic,
                ),
            );
        }
    }
    if shoot {
        if byte(env, player + 0x1e1) == Some(0) {
            crate::window::DS_AIM_REQUEST.store(1, Relaxed);
        }
        if now >= env.framework_state.opengles.ds_test_assist.next_ammo {
            env.framework_state.opengles.ds_test_assist.next_ammo = now + 1_000_000;
            if let Some(slot) = word(env, player + 0x2ec).filter(|n| *n < 9) {
                if let Some(w) = word(env, player + 0x2d4 + slot * 4).filter(|p| *p >= 0x1000) {
                    if word(env, w).is_some_and(|v| (0xa00000..0xb00000).contains(&v))
                        && word(env, w + 0x168).is_some_and(|n| n < 4)
                    {
                        let _: () = GuestFunction::from_addr_and_thumb_flag(0x701d8, false)
                            .call_from_host(env, (w, u32::MAX));
                        env.framework_state.opengles.ds_test_assist.refills += 1;
                    }
                }
            }
        }
        crate::window::DS_FIRE_MODE_REQUEST.store(1, Relaxed);
        env.framework_state.opengles.ds_test_assist.shots += 1;
    } else {
        crate::window::DS_CONTEXT_COMBAT_REQUEST.store(true, Relaxed);
        env.framework_state.opengles.ds_test_assist.melee += 1;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_assist_lease_expires_and_zero_never_arms() {
        assert!(!active(0, 0));
        assert!(active(50, 51));
        assert!(!active(51, 51));
        assert!(!active(52, 51));
    }
    #[test]
    fn test_assist_health_retains_damage_but_allows_healing_and_initialization() {
        assert_eq!(
            clamp_requested(100f32.to_bits(), 20f32.to_bits()),
            Some(100f32.to_bits())
        );
        assert_eq!(
            clamp_requested(100f32.to_bits(), (-10f32).to_bits()),
            Some(100f32.to_bits())
        );
        assert_eq!(clamp_requested(100f32.to_bits(), 200f32.to_bits()), None);
        assert_eq!(clamp_requested(0, 100f32.to_bits()), None);
        assert_eq!(clamp_requested(f32::NAN.to_bits(), 0), None);
        assert_eq!(clamp_requested(100f32.to_bits(), f32::NAN.to_bits()), None);
    }
}

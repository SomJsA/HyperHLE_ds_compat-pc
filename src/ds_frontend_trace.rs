//! Opt-in ordering trace for the original Dead Space frontend. No UI state is
//! changed: checked ARM PUSH/POP instructions are executed after each event.
//! The default guard suppresses at most one draw in each handoff window:
//! layout construction -> animation start, and the first draw after an entry
//! animation starts. This covers the Android/PC cases where the completed
//! layout can be presented before the animator's first visible update.
use crate::{abi::GuestFunction, cpu::Cpu, mem::{ConstVoidPtr, MutPtr}, Environment};

const BKPT: u32 = 0xe120_0070;
const SITES: &[(u32, u32, &str)] = &[
    (0x001b_22d4, 0xe92d_40f0, "layout.begin"),
    (0x001b_83d8, 0xe8bd_80f0, "layout.end"),
    (0x002d_a6e8, 0xe92d_4090, "anim.begin"),
    (0x002d_a728, 0xe8bd_8090, "anim.end"),
    (0x001b_d604, 0xe92d_40f0, "draw"),
    (0x001b_8648, 0xe92d_40f0, "state.begin"),
    (0x002f_e080, 0xe92d_40b0, "difficulty.update"),
    (0x002c_1174, 0xe92d_40f0, "widget.draw"),
    // The registration Popup embedded in LayerMainMenu uses the Renderer
    // overload, rather than the SpriteGraphics overload used by HUD dialogs.
    // Observe both drawing owners so its native buttons receive input too.
    (0x002c_19f4, 0xe92d_40f0, "widget.draw"),
    (0x0026_af84, 0xe92d_40f0, "loading.draw"),
];

#[derive(Default)]
pub(crate) struct State {
    checked: bool,
    armed: bool,
    trace_enabled: bool,
    trace_draw: bool,
    guard_enabled: bool,
    layout_guard_pending: bool,
    skip_next_draw: bool,
    guard_logged: bool,
    frame: u64,
    serial: u64,
    layouts: Vec<(u32, u32)>,
    animations: Vec<(u32, u32)>,
}

fn word(env: &Environment, a: u32) -> Option<u32> {
    let b = env.mem.get_bytes_fallible(ConstVoidPtr::from_bits(a), 4)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

pub(crate) fn prime(env: &mut Environment) {
    if !env.framework_state.opengles.ds_frontend_trace.checked {
        env.framework_state.opengles.ds_frontend_trace.checked = true;
        crate::ds_frontend_controller::deactivate();
        let trace_enabled = std::env::var("TOUCHHLE_DS_FRONTEND_TRACE").as_deref() == Ok("1");
        let guard_enabled = std::env::var("TOUCHHLE_DS_FRONTEND_GUARD").as_deref() != Ok("0");
        let input_enabled = crate::ds_frontend_controller::configured();
        if !trace_enabled && !guard_enabled && !input_enabled {
            return;
        }
        let known = env.bundle.bundle_identifier() == "com.ea.deadspace.bv"
            && (!cfg!(target_os="windows") || !input_enabled || crate::ds_frontend_controller::native_modal_verified(env))
            && (!cfg!(target_os="windows") || word(env,0x0026_b018)==Some(0xeb00_0381))
            && SITES.iter().all(|&(a, op, _)| word(env, a) == Some(op))
            && [0x001b_f25c, 0x002f_e0f8, 0x0019_4158, 0x001e_0488, 0x001d_0b80].iter().all(|&a| word(env,a)==Some(0xe92d_40f0));
        if !known {
            echo!("[DSfrontend] unsupported binary; trace skipped");
            return;
        }
        if input_enabled { crate::ds_frontend_controller::activate(); }
        for &(a, _, event) in SITES {
            if matches!(event,"widget.draw"|"loading.draw")&&(!cfg!(target_os="windows")||!input_enabled){continue;}
            env.mem.write(MutPtr::<u32>::from_bits(a), BKPT);
            env.cpu.invalidate_cache_range(a, 4);
        }
        if cfg!(target_os="windows") && input_enabled {
            // Replace only the bottom touch-to-continue string with PC input
            // sprites. Keep original loading readiness/continuation unchanged.
            env.mem.write(MutPtr::<u32>::from_bits(0x0026_b018),0xe1a0_0000);
            env.cpu.invalidate_cache_range(0x0026_b018,4);
        }
        env.framework_state.opengles.ds_frontend_trace.trace_enabled = trace_enabled;
        env.framework_state.opengles.ds_frontend_trace.armed = true;
        env.framework_state.opengles.ds_frontend_trace.guard_enabled = guard_enabled;
        env.framework_state.opengles.ds_frontend_trace.trace_draw =
            std::env::var("TOUCHHLE_DS_FRONTEND_DRAW_TRACE").as_deref() == Ok("1");
        if trace_enabled {
            echo!("[DSfrontend] trace armed; original ARM instructions verified");
        }
        if guard_enabled {
            echo!("[DSfrontendGuard] enabled; layout/animation handoff draws masked");
        }
        if input_enabled {
            echo!("[DSfrontendInput] controller navigation armed");
        }
    }
}

pub(crate) fn present(env: &mut Environment) {
    // Presentation can run on a guest rendering thread between host input
    // polls. Menu freshness must advance there too, or a closed difficulty
    // page can keep blocking gameplay input indefinitely.
    crate::ds_frontend_controller::observe_frame(crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed));
    prime(env);
    let state = &mut env.framework_state.opengles.ds_frontend_trace;
    if state.armed { state.frame += 1; }
    if crate::ds_frontend_controller::enabled() {
        // This is the one per-frame boundary that is present even when a
        // menu does not issue a color-buffer clear. Drain queued focus changes
        // and page requests here.
        crate::ds_frontend_controller::tick_native(env);
    }
}

pub(crate) fn handle_breakpoint(env: &mut Environment) -> bool {
    if !env.framework_state.opengles.ds_frontend_trace.armed { return false; }
    let pc = env.cpu.regs()[Cpu::PC];
    let Some(&(_, original, event)) = SITES.iter().find(|s| s.0 == pc) else { return false; };
    let owner = env.cpu.regs()[0];
    let arg = env.cpu.regs()[1];
    let caller = env.cpu.regs()[Cpu::LR];
    if event == "loading.draw" {
        crate::ds_frontend_controller::observe_loading(env,owner);
    } else if event == "widget.draw" {
        crate::ds_frontend_controller::observe_ui_handler(env,owner);
    } else if event == "draw" {
        if let Some(page) = word(env, owner.saturating_add(0x50)) {
            crate::ds_frontend_controller::observe_menu(page);
            crate::ds_frontend_controller::observe_widgets(env, owner, page, false);
        }
    } else if event == "difficulty.update" {
        crate::ds_frontend_controller::observe_menu(7);
        crate::ds_frontend_controller::observe_widgets(env, owner, 7, true);
    }
    if event == "draw" {
        let state = &mut env.framework_state.opengles.ds_frontend_trace;
        if state.guard_enabled && (state.layout_guard_pending || state.skip_next_draw) {
            let layout_window = state.layout_guard_pending;
            state.layout_guard_pending = false;
            state.skip_next_draw = false;
            if !state.guard_logged {
                state.guard_logged = true;
                echo!(
                    "[DSfrontendGuard] masked first draw during {}",
                    if layout_window { "layout construction" } else { "entry-animation start" }
                );
            }
            // This is the function entry before the original PUSH executes, so
            // returning through LR leaves the stack and guest state untouched.
            env.cpu.branch(GuestFunction::from_addr_with_thumb_bit(caller));
            return true;
        }
    }
    let layer = match event {
        "layout.end" => env.framework_state.opengles.ds_frontend_trace.layouts.last().map(|s| s.0).unwrap_or(0),
        "anim.begin" | "anim.end" => env.framework_state.opengles.ds_frontend_trace.layouts.last().map(|s| s.0).unwrap_or(owner),
        _ => owner,
    };
    let read = |offset: u32| word(env, layer.saturating_add(offset)).unwrap_or(u32::MAX);
    let owner_state = word(env, owner.saturating_add(0x50)).unwrap_or(u32::MAX);
    let (menu_state, layout_state, elapsed, animator) = (read(0x50), read(0x140), read(0x144), read(0x148));
    let anim_time = word(env, animator.saturating_add(0x14)).unwrap_or(u32::MAX);
    let state = &mut env.framework_state.opengles.ds_frontend_trace;
    state.serial += 1;
    if state.trace_enabled && (!matches!(event, "draw" | "difficulty.update" | "widget.draw" | "loading.draw") || state.trace_draw) {
        echo!("[DSfrontend] n={} frame={} event={} this={:#x} arg={} lr={:#x} layer={:#x} state={} owner_state={} layout={} elapsed={} animator={:#x} anim_time={} layout_depth={} anim_depth={}",
            state.serial, state.frame, event, owner, arg, caller, layer, menu_state, owner_state, layout_state, elapsed, animator, anim_time, state.layouts.len(), state.animations.len());
    }
    match event {
        "layout.begin" => {
            if state.guard_enabled && matches!(arg, 0 | 1) {
                state.layout_guard_pending = true;
            }
            state.layouts.push((owner, arg));
        }
        "layout.end" => { state.layouts.pop(); },
        "anim.begin" => {
            if state.guard_enabled && arg == 0 {
                state.skip_next_draw = true;
            }
            state.animations.push((owner, arg));
        }
        "anim.end" => { state.animations.pop(); },
        // The frontend state machine passes the page id in r1. The offset
        // read above belongs to layout objects and is zero for this event.
        "state.begin" => crate::ds_frontend_controller::set_state(arg),
        _ => (),
    }
    // The intercepted instructions have no flag side effects. Preserve every
    // register and the ARM full-descending stack, including POP's PC exchange.
    let mask = original & 0xffff;
    let regs = *env.cpu.regs();
    let sp = regs[Cpu::SP];
    if original & 0x0010_0000 == 0 {
        let new_sp = sp.wrapping_sub(mask.count_ones() * 4);
        let mut address = new_sp;
        for r in 0..16 {
            if mask & (1 << r) != 0 {
                env.mem.write(MutPtr::<u32>::from_bits(address), regs[r]);
                address = address.wrapping_add(4);
            }
        }
        env.cpu.regs_mut()[Cpu::SP] = new_sp;
        env.cpu.regs_mut()[Cpu::PC] = pc + 4;
    } else {
        let mut address = sp;
        let mut return_to = pc + 4;
        for r in 0..16 {
            if mask & (1 << r) != 0 {
                let value = env.mem.read(crate::mem::ConstPtr::<u32>::from_bits(address));
                if r == Cpu::PC { return_to = value; } else { env.cpu.regs_mut()[r] = value; }
                address = address.wrapping_add(4);
            }
        }
        env.cpu.regs_mut()[Cpu::SP] = address;
        env.cpu.branch(GuestFunction::from_addr_with_thumb_bit(return_to));
    }
    true
}

//! Native Popup/Store/Bench input ownership for the verified ARM build.
//! Widgets and bounds come from the original handlers; activation runs their
//! original methods. No player/HUD state or currency is written by the host.
use super::super::{Action, Point};
use super::{byte, named_widgets, nearest, word, Button, GET_ACTIVE_RECT};
use crate::{
    abi::{CallFromHost, GuestFunction},
    Environment,
};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Mutex,
};

const POPUP_VT: u32 = 0x00a8_9128;
const STORE_VT: u32 = 0x00a7_b648;
const BENCH_VT: u32 = 0x00a7_b828;
const STORE_WINDOW_VT: u32 = 0x00a8_8468;
const STORE_PANEL_VT: u32 = 0x00a8_8588;
const BENCH_GRID_VT: u32 = 0x00a8_8678;
const BENCH_NODE_VT: u32 = 0x00a8_86b8;
const BENCH_WINDOW_VT: u32 = 0x00a6_5908;
const SLIDER_VT:u32=0x00a8_39c8;
const SLIDE_FILL_VT:u32=0x00a8_3a08;
const SLIDER_BUTTON_VT:u32=0x00a8_40a8;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static POPUP: AtomicU32 = AtomicU32::new(0);
static POPUP_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static ROOT_A: AtomicU32 = AtomicU32::new(0);
static ROOT_B: AtomicU32 = AtomicU32::new(0);
static ROOT_A_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static ROOT_B_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static TICKING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Popup,
    Store,
    Bench,
    Pause,
    HudMenu,
    Transition,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Context {
    kind: Kind,
    object: u32,
    handler: u32,
    window: u32,
    world: u32,
    mode: u32,
    ready: bool,
}
impl Context {
    fn same_owner(self, other: Self) -> bool {
        (self.kind, self.object, self.handler, self.world, self.mode)
            == (
                other.kind,
                other.object,
                other.handler,
                other.world,
                other.mode,
            )
    }
}
#[derive(Clone, Copy)]
enum ControlKind {
    Button,
    StoreRow(u32, u32),
    BenchRow(u32, u32),
    BenchNode,
    HudSlider,
}
#[derive(Clone)]
struct Control {
    button: Button,
    kind: ControlKind,
    window: u32,
    clip: Option<[f32; 4]>,
}
struct State {
    context: Option<Context>,
    controls: Vec<Control>,
    focus: usize,
    requests: Vec<(Context, Action)>,
    dirty: bool,
    frame: u64,
    return_focus: Option<(Context,String)>,
    scroll_until: u64,
}
static STATE: Mutex<State> = Mutex::new(State {
    context: None,
    controls: Vec::new(),
    focus: 0,
    requests: Vec::new(),
    dirty: false,
    frame: u64::MAX,
    return_focus: None,
    scroll_until: 0,
});

fn frame() -> u64 {
    crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed)
}
fn recent(stamp: u64) -> bool {
    stamp != u64::MAX && frame().saturating_sub(stamp) <= 2
}
pub(super) fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}
pub(super) fn verified(env: &Environment) -> bool {
    [
        (0x0003_0474, 0xe92d_4080),
        (0x002b_eb8c, 0xe92d_40b0),
        (0x002f_196c, 0xe5d0_0018),
        (0x0029_88ec, 0xe92d_40f0),
        (0x002a_5ce8, 0xe92d_4080),
        (0x001d_f74c, 0xe92d_40f0),
        (0x001c_d9f4, 0xe5d0_1068),
        (0x002a_0ab0, 0xe92d_40f0),
        (0x001c_da9c, 0xe92d_4080),
        (0x0018_c7b0, 0xe595_0234), // RenderHUD pause state test
        (0x0018_c7bc, 0xe301_0294), // verified pause WidgetHandler offset
    ]
    .iter()
    .all(|&(address, opcode)| word(env, address) == Some(opcode))
}
pub(super) fn reset() {
    ACTIVE.store(false, Ordering::Relaxed);
    POPUP.store(0, Ordering::Relaxed);
    POPUP_FRAME.store(u64::MAX, Ordering::Relaxed);
    ROOT_A.store(0, Ordering::Relaxed);
    ROOT_B.store(0, Ordering::Relaxed);
    ROOT_A_FRAME.store(u64::MAX, Ordering::Relaxed);
    ROOT_B_FRAME.store(u64::MAX, Ordering::Relaxed);
    let mut state = STATE.lock().unwrap();
    state.context = None;
    state.controls.clear();
    state.requests.clear();
    state.focus = 0;
    state.dirty = false;
    state.return_focus=None;
}

/// Called only at the checked WidgetHandler::OnDraw breakpoint. Never call
/// guest code here: store the actual drawing owner for the host-call boundary.
pub(super) fn observe_handler(env: &Environment, handler: u32) {
    if !cfg!(target_os = "windows") || word(env, handler + 0x7c).unwrap_or(0) == 0 {
        return;
    }
    let now = frame();
    if word(env, handler) == Some(POPUP_VT) {
        POPUP.store(handler, Ordering::Relaxed);
        POPUP_FRAME.store(now, Ordering::Relaxed);
    }
    if handler == ROOT_A.load(Ordering::Relaxed) {
        ROOT_A_FRAME.store(now, Ordering::Relaxed);
    }
    if handler == ROOT_B.load(Ordering::Relaxed) {
        ROOT_B_FRAME.store(now, Ordering::Relaxed);
    }
}

fn detect(env: &mut Environment) -> Option<Context> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let hud: u32 =
        GuestFunction::from_addr_and_thumb_flag(0x0003_0474, false).call_from_host(env, ());
    let world = word(env, hud.checked_add(4)?).unwrap_or(0);
    let popup = POPUP.load(Ordering::Relaxed);
    if popup >= 0x1000
        && recent(POPUP_FRAME.load(Ordering::Relaxed))
        && word(env, popup) == Some(POPUP_VT)
    {
        let mode = word(env, popup + 0x84).unwrap_or(0);
        let animating: bool = GuestFunction::from_addr_and_thumb_flag(0x002b_eb8c, false)
            .call_from_host(env, (popup,));
        return Some(Context {
            kind: Kind::Popup,
            object: popup,
            handler: popup,
            window: 0,
            world,
            mode,
            ready: mode != 0 && !animating,
        });
    }
    if world < 0x1000 {
        return None;
    }
    // In-game help/options are Hud::SetLayout, not LayerMainMenu. Their
    // current layout is drawn by +24c, with its own animator at +1340.
    if matches!(word(env,hud+0x234),Some(4|5)) {
        let handler=hud+0x24c;
        if ROOT_A.swap(handler,Ordering::Relaxed)!=handler {ROOT_A_FRAME.store(u64::MAX,Ordering::Relaxed);}
        ROOT_B.store(0,Ordering::Relaxed);
        // The native update uses IsFinished (+10 virtual), not IsAnimating.
        // At exactly duration it enters phase 2 and stops updating, leaving
        // IsAnimating's byte set forever. Phase 2 is the native completion gate.
        return Some(Context{kind:Kind::HudMenu,object:hud,handler,window:0,world,
            mode:word(env,hud+0x132c).unwrap_or(0),
            ready:layout_ready(word(env,hud+0x1328).unwrap_or(0),recent(ROOT_A_FRAME.load(Ordering::Relaxed)))});
    }
    // Both 2 and 3 are pause variants; native OnUpdate does not promote 2
    // to 3 when the entry animation ends. Readiness comes from the animator
    // and the actual pause handler draw, not an assumed numeric transition.
    if matches!(word(env,hud+0x234),Some(2|3)) {
        let handler=hud+0x1294;
        if ROOT_A.swap(handler,Ordering::Relaxed)!=handler {ROOT_A_FRAME.store(u64::MAX,Ordering::Relaxed);}
        ROOT_B.store(0,Ordering::Relaxed);
        let animator=word(env,hud+0x127c).unwrap_or(0);
        let animating=if animator>=0x1000 {
            let active:bool=GuestFunction::from_addr_and_thumb_flag(0x002f_196c,false).call_from_host(env,(animator,));active
        } else {true};
        return Some(Context{kind:Kind::Pause,object:hud,handler,window:0,world,mode:0,
            ready:pause_ready(word(env,hud+0x234).unwrap_or(0),recent(ROOT_A_FRAME.load(Ordering::Relaxed)),animating)});
    }
    let player = word(env, world + 0xe0).filter(|p| *p >= 0x1000)?;
    if word(env, player) != Some(0x00a7_a4f8) {
        return None;
    }
    let phase = word(env, player + 0x208)?;
    if !(19..=21).contains(&phase) {
        return None;
    }
    let store = word(env, player + 0x528).unwrap_or(0);
    let bench = word(env, player + 0x524).unwrap_or(0);
    if store >= 0x1000 && word(env, store) == Some(STORE_VT) {
        let handler = store + 0x1e0;
        if ROOT_A.swap(handler, Ordering::Relaxed) != handler {
            ROOT_A_FRAME.store(u64::MAX, Ordering::Relaxed);
        }
        ROOT_B.store(0, Ordering::Relaxed);
        let animator = word(env, store + 0x314).unwrap_or(0);
        let animating = if animator >= 0x1000 {
            let value: bool = GuestFunction::from_addr_and_thumb_flag(0x002f_196c, false)
                .call_from_host(env, (animator,));
            value
        } else {
            false
        };
        return Some(Context {
            kind: Kind::Store,
            object: store,
            handler,
            window: word(env, store + 0x264).unwrap_or(0),
            world,
            // +0x1d4 is elapsed time, not a mode. Including that timer in
            // ownership discarded focus/queued input on every rendered frame.
            mode: word(
                env,
                word(env, word(env, store + 0x264).unwrap_or(0) + 0xa4).unwrap_or(0) + 0x70,
            )
            .filter(|n| *n < 3)
            .unwrap_or(0),
            ready: phase == 20
                && word(env, store + 0x1d0) == Some(4)
                && recent(ROOT_A_FRAME.load(Ordering::Relaxed))
                && !animating,
        });
    }
    if bench >= 0x1000 && word(env, bench) == Some(BENCH_VT) {
        let a = bench + 0x2dc;
        let b = bench + 0x360;
        if ROOT_A.swap(a, Ordering::Relaxed) != a {
            ROOT_A_FRAME.store(u64::MAX, Ordering::Relaxed);
        }
        if ROOT_B.swap(b, Ordering::Relaxed) != b {
            ROOT_B_FRAME.store(u64::MAX, Ordering::Relaxed);
        }
        let af = ROOT_A_FRAME.load(Ordering::Relaxed);
        let bf = ROOT_B_FRAME.load(Ordering::Relaxed);
        // Bench::OnDraw loads this exact active WidgetHandler pointer, then
        // invokes its native OnDraw virtual. It is not the BenchWindow widget.
        let handler = word(env, bench + 0x2d8)
            .filter(|p| *p == a || *p == b)
            .unwrap_or(0);
        let mode = word(env, bench + 0x214).unwrap_or(0);
        return Some(Context {
            kind: Kind::Bench,
            object: bench,
            handler,
            window: 0,
            world,
            mode,
            ready: phase == 20
                && matches!(mode, 6 | 7)
                && recent(if handler == a { af } else { bf }),
        });
    }
    // A native station transition owns input even if its widgets are not ready.
    Some(Context {
        kind: Kind::Transition,
        object: player,
        handler: 0,
        window: 0,
        world,
        mode: phase,
        ready: false,
    })
}

fn native_rect(env: &mut Environment, method: u32, widget: u32) -> Option<[f32; 4]> {
    if widget < 0x1000 {
        return None;
    }
    let buffer = env.mem.alloc(16);
    let _: () = GuestFunction::from_addr_and_thumb_flag(method, false)
        .call_from_host(env, (buffer.to_bits(), widget));
    let rect = std::array::from_fn(|i| {
        f32::from_bits(word(env, buffer.to_bits() + i as u32 * 4).unwrap_or(0))
    });
    env.mem.free(buffer);
    rect.iter().all(|v| v.is_finite()).then_some(rect)
}

fn rows(env: &mut Environment, panel: u32, vector: u32, bench: bool, window: u32) -> Vec<Control> {
    let Some((begin, end)) = word(env, panel + vector).zip(word(env, panel + vector + 4)) else {
        return Vec::new();
    };
    let Some(bytes) = end
        .checked_sub(begin)
        .filter(|n| *n % 8 == 0 && *n <= 8 * 1024)
    else {
        return Vec::new();
    };
    let Some(clip) = native_rect(env, 0x001d_f74c, window).filter(|r| r[2] > 0. && r[3] > 0.)
    else {
        return Vec::new();
    };
    let scroll_x = word(env, window + 0x8c).unwrap_or(0) as i32 as f32;
    let scroll_y = word(env, window + 0x90).unwrap_or(0) as i32 as f32;
    let mut offset = 0.;
    let mut out = Vec::new();
    for index in 0..bytes / 8 {
        let widget = word(env, begin + index * 8).unwrap_or(0);
        let vtable = word(env, widget).unwrap_or(0);
        if vtable != if bench { 0x00a8_8628 } else { 0x00a8_8538 }
            || byte(env,widget+0x24)!=Some(1) {
            continue;
        }
        let Some(mut rect) = native_rect(env, GET_ACTIVE_RECT, widget) else {
            continue;
        };
        // The native scrolling panel forwards pointer coordinates into its
        // local space. GetActiveRect on each row returns that LOCAL rect.
        // Reconstruct the same draw origin and cumulative cell pitch; never
        // use a callback delegate (+0x60/+0x5c) as a ScrollingWindow pointer.
        rect[0] += clip[0] + scroll_x + if bench { offset } else { 0. };
        rect[1] += clip[1] + scroll_y + if bench { 0. } else { offset };
        let pitch = if bench {
            let size: i32 = GuestFunction::from_addr_and_thumb_flag(0x002a_5394, false)
                .call_from_host(env, (widget,));
            size.max(1) as f32
        } else {
            rect[3].max(1.)
        };
        offset += pitch;
        out.push(Control {
            button: Button {
                id: format!("ROW_{index}"),
                widget,
                vtable,
                rect,
            },
            kind: if bench {
                ControlKind::BenchRow(panel, index)
            } else {
                ControlKind::StoreRow(panel, index)
            },
            window,
            clip: Some(clip),
        });
    }
    out
}

fn collect(env: &mut Environment, context: Context) -> Vec<Control> {
    if context.handler == 0 || (context.kind==Kind::HudMenu && !context.ready) {
        return Vec::new();
    }
    let named = named_widgets(env, context.handler);
    let mut controls: Vec<Control> = named
        .iter()
        .filter(|w| {
            w.active
                && (context.kind!=Kind::Pause || matches!(w.id.as_str(),"BTN_PLAY_PAUSE"|"BTN_RESTART"|"BTN_OPTIONS"|"BTN_HELP"|"BTN_QUIT"))
                && ((context.kind==Kind::HudMenu && hud_slider_type(w.vtable))
                    || (w.id.starts_with("BTN_") && word(env,w.vtable+0x2c)
                        .is_some_and(|f|(0x1000..0x8c0000).contains(&(f&!1)))))
        })
        .map(|w| Control {
            button: Button {
                id: w.id.clone(),
                widget: w.widget,
                vtable: w.vtable,
                rect: w.rect,
            },
            kind: if context.kind==Kind::HudMenu && hud_slider_type(w.vtable) {ControlKind::HudSlider}else{ControlKind::Button},
            window: 0,
            clip: None,
        })
        .collect();
    if context.kind == Kind::Store {
        let window = if word(env, context.window) == Some(STORE_WINDOW_VT) {
            context.window
        } else {
            named
                .iter()
                .find(|w| w.vtable == STORE_WINDOW_VT)
                .map(|w| w.widget)
                .unwrap_or(0)
        };
        let panel = word(env, window + 0x70)
            .filter(|p| word(env, *p) == Some(STORE_PANEL_VT))
            .or_else(|| word(env, window + 0xa0).filter(|p| word(env, *p) == Some(STORE_PANEL_VT)))
            .unwrap_or(0);
        // StoreTabs owns three LayoutButtons outside the root handler's name
        // map. Enumerate its real shared_ptr members, not fabricated buttons or
        // direct tab-state writes, so native delegates retain list/popup policy.
        if let Some(tabs)=word(env,window+0xa4).filter(|p|*p>=0x1000) {
            // Apple's legacy DLC service is unavailable. Native SetEnabled
            // blocks pointer hit-testing too, before any network request starts.
            let dlc=word(env,tabs+0x68).unwrap_or(0);
            if word(env,dlc).is_some_and(|vt|word(env,vt+0x2c)==Some(0x001d_0b80)) && byte(env,dlc+0x24)==Some(1) {
                let _:()=GuestFunction::from_addr_and_thumb_flag(0x001c_d994,false).call_from_host(env,(dlc,false));
                echo!("[DSnativeUI] disabled offline DLC tab through native SetEnabled");
            }
            if word(env,tabs+0x70)==Some(2) {
                let shop=word(env,tabs+0x58).unwrap_or(0);
                if byte(env,shop+0x24)==Some(1){let _:()=GuestFunction::from_addr_and_thumb_flag(0x001d_0b80,false).call_from_host(env,(shop,));}
            }
            for (offset,id) in [(0x58,"BTN_SHOP"),(0x60,"BTN_INVENTORY")] {
                let widget=word(env,tabs+offset).unwrap_or(0);
                let vtable=word(env,widget).unwrap_or(0);
                if byte(env,widget+0x24)==Some(1) && word(env,vtable+0x2c)==Some(0x001d_0b80) {
                    if let Some(rect)=native_rect(env,GET_ACTIVE_RECT,widget) {
                        controls.push(Control{button:Button{id:id.into(),widget,vtable,rect},kind:ControlKind::Button,window:0,clip:None});
                    }
                }
            }
        }
        if panel != 0 {
            controls.extend(rows(env, panel, 0x68, false, window));
        }
    }
    if context.kind == Kind::Bench {
        // The main BenchWindow exposes its native BenchPanel row vector.
        if let Some(window) = named.iter().find(|w| w.vtable == BENCH_WINDOW_VT) {
            let panel = word(env, window.widget + 0x70)
                .filter(|p| word(env, *p) == Some(0x00a8_85d8))
                .or_else(|| {
                    word(env, window.widget + 0xa0).filter(|p| word(env, *p) == Some(0x00a8_85d8))
                })
                .unwrap_or(0);
            if panel >= 0x1000 && word(env, panel) == Some(0x00a8_85d8) {
                controls.extend(rows(env, panel, 0x60, true, window.widget));
            }
        }
        // Grid nodes use an independent native string->widget hash map.
        // All weapon grids remain allocated in the handler. Their child
        // buttons can stay enabled while the parent grid is hidden: collecting
        // them all overlapped 86 nodes and could select another weapon's node.
        for grid in named.iter().filter(|w| {
            w.vtable == BENCH_GRID_VT && w.active
                && word(env,w.widget+0x58)==Some(context.object)
                && word(env,w.widget+0x5c)==word(env,context.object+0x1d0)
                // BenchGrid::OnEvent's first gate is Entity::visible (+0x54),
                // not LayoutWidget::enabled. Match that exact native gate.
                && word(env,w.widget+0x3c).and_then(|entity|byte(env,entity+0x54)).is_some_and(|v|v!=0)
        }) {
            for node in super::named_widgets_map(env, grid.widget + 0x78, 0) {
                if node.vtable == BENCH_NODE_VT && node.active
                    && word(env,node.widget+0x3c).and_then(|entity|byte(env,entity+0x54)).is_some_and(|v|v!=0) {
                    controls.push(Control {
                        button: Button {
                            id: format!("NODE_{}", node.id),
                            widget: node.widget,
                            vtable: node.vtable,
                            rect: node.rect,
                        },
                        kind: ControlKind::BenchNode,
                        window: 0,
                        clip: None,
                    });
                }
            }
        }
    }
    if context.kind==Kind::HudMenu {
        if let Some(window)=named.iter().find(|w|w.id=="OPTIONS_WINDOW" && w.vtable==0x00a8_4978) {
            controls.extend(hud_options_controls(env,window.widget));
        }
    }
    let buffer = env.mem.alloc(16);
    for control in &mut controls {
        if control.window != 0 || word(env, control.button.widget) != Some(control.button.vtable) {
            continue;
        }
        let _: () = GuestFunction::from_addr_and_thumb_flag(GET_ACTIVE_RECT, false)
            .call_from_host(env, (buffer.to_bits(), control.button.widget));
        for i in 0..4 {
            control.button.rect[i] =
                f32::from_bits(word(env, buffer.to_bits() + i as u32 * 4).unwrap_or(0));
        }
    }
    env.mem.free(buffer);
    controls.retain(|c| {
        c.button.rect.iter().all(|n| n.is_finite())
            && c.button.rect[2] > 0.
            && c.button.rect[3] > 0.
    });
    controls.sort_by(|a, b| {
        a.button.rect[1]
            .total_cmp(&b.button.rect[1])
            .then(a.button.rect[0].total_cmp(&b.button.rect[0]))
    });
    controls.dedup_by_key(|c| c.button.widget);
    controls
}

fn pause_ready(hud_state:u32,drawn:bool,animating:bool)->bool {
    matches!(hud_state,2|3) && drawn && !animating
}
fn layout_ready(layout_state:u32,drawn:bool)->bool {
    // IsAnimating=false also means "not started yet". Hud::UpdateLayout
    // transitions 1->2 only after entry finishes, and 3->0 after exit.
    // Scanning phase 1 latched offscreen arrows before sliders were enabled.
    layout_state==2 && drawn
}

fn selected(state: &State) -> Option<&Control> {
    state.controls.get(state.focus)
}

fn confirmation_control(state:&State)->Option<Control> {
    // Fixed E/A confirms and Q/X cancels, independently of pointer hover.
    // Content rows/nodes still use their native purchase/upgrade selection.
    selected(state).filter(|c|!state.context.is_some_and(|c|c.kind==Kind::HudMenu) && !super::fixed_action(&c.button.id)).cloned().or_else(||
        state.controls.iter().find(|c|matches!(c.button.id.as_str(),"BTN_OK"|"BTN_YES"|"BTN_PURCHASE"|"BTN_UPGRADE")).cloned())
}

fn hud_slider_type(vtable:u32)->bool {matches!(vtable,SLIDER_VT|SLIDE_FILL_VT)}

fn translate_rect(mut rect:[f32;4],x:i32,y:i32)->[f32;4] {rect[0]+=x as f32;rect[1]+=y as f32;rect}

fn option_child_type(vtable:u32)->bool {
    matches!(vtable,SLIDER_BUTTON_VT|0x00a8_40f8|0x00a8_4148|0x00a8_4198|0x00a8_41e8)
}
fn option_value_type(vtable:u32)->bool {hud_slider_type(vtable)||vtable==0x00a8_4468}
fn option_origin(env:&mut Environment,window:u32)->Option<([f32;4],i32,i32)> {
    let clip=native_rect(env,0x001d_f74c,window)?;
    Some((clip,clip[0] as i32+word(env,window+0x8c)? as i32,
        clip[1] as i32+word(env,window+0x90)? as i32))
}
fn option_child_rect(env:&mut Environment,widget:u32,window:u32)->Option<[f32;4]> {
    let rect=native_rect(env,GET_ACTIVE_RECT,widget)?;
    if window==0 {return Some(rect);}
    let (_,x,y)=option_origin(env,window)?;Some(translate_rect(rect,x,y))
}
fn hud_options_controls(env:&mut Environment,window:u32)->Vec<Control> {
    // OptionsWindow draws OptionsPanel (+70); OnEvent iterates its shared_ptr
    // vector +5c/+60. These controls are absent from the root handler map.
    // Their empty parent active rect must be reconstructed from real children,
    // translated by the same scrolling viewport origin used by native Draw.
    let panel=word(env,window+0x70).filter(|p|word(env,*p)==Some(0x00a8_4588)).unwrap_or(0);
    let Some((begin,end))=word(env,panel+0x5c).zip(word(env,panel+0x60)) else{return vec![]};
    let Some(bytes)=end.checked_sub(begin).filter(|n|*n<=8*64 && n%8==0) else{return vec![]};
    let Some((clip,_,_))=option_origin(env,window) else{return vec![]};
    let mut out=Vec::new();
    for index in 0..bytes/8 {
        let widget=word(env,begin+index*8).unwrap_or(0);let vt=word(env,widget).unwrap_or(0);
        if !option_value_type(vt)||byte(env,widget+0x24)!=Some(1){continue;}
        let mut rects=Vec::new();
        for offset in [0x58,0x60] {
            let child=word(env,widget+offset).unwrap_or(0);
            if word(env,child).is_some_and(option_child_type) {
                if let Some(rect)=option_child_rect(env,child,window){rects.push(rect);}
            }
        }
        if rects.len()!=2{continue;}
        let x=rects[0][0].min(rects[1][0]);let y=rects[0][1].min(rects[1][1]);
        let right=(rects[0][0]+rects[0][2]).max(rects[1][0]+rects[1][2]);
        let bottom=(rects[0][1]+rects[0][3]).max(rects[1][1]+rects[1][3]);
        if right<=clip[0] || x>=clip[0]+clip[2] || bottom<=clip[1] || y>=clip[1]+clip[3]{continue;}
        out.push(Control{button:Button{id:format!("OPTION_{index}"),widget,vtable:vt,rect:[x,y,right-x,bottom-y]},
            kind:ControlKind::HudSlider,window,clip:Some(clip)});
    }
    out
}
fn adjust_hud_slider(env:&mut Environment,_handler:u32,control:&Control,right:bool) {
    if !word(env,control.button.widget).is_some_and(option_value_type){return;}
    // Minus/Plus and Selector arrows inherit the native pointer behavior;
    // SendAction is a no-op. Internal taps preserve native clamping/callbacks.
    // WidgetHandler +4/+8 are DRAW WIDTH/HEIGHT, never translation offsets.
    let mut points=Vec::new();
    for offset in [0x58,0x60] {
        let child=word(env,control.button.widget+offset).unwrap_or(0);
        if !word(env,child).is_some_and(option_child_type){continue;}
        if let Some(rect)=option_child_rect(env,child,control.window) {
            if visible(rect){points.push((Point::new(rect[0]+rect[2]/2.,rect[1]+rect[3]/2.),byte(env,child+0x24)==Some(1)));}
        }
    }
    if let Some(point)=slider_point(&mut points,right) {
        if let Some(window)=env.window.as_mut(){window.ds_queue_ui_tap(point.x,point.y);}
        echo!("[DSnativeUI] slider {} right={} tap=({}, {}) before={}",control.button.id,right,point.x,point.y,
            f32::from_bits(word(env,control.button.widget+0x78).unwrap_or(0)));
    }
}

fn slider_point(points:&mut [(Point,bool)],right:bool)->Option<Point> {
    if points.len()!=2{return None;}
    points.sort_by(|a,b|a.0.x.total_cmp(&b.0.x));
    let (point,enabled)=points[usize::from(right)];enabled.then_some(point)
}
pub(super) fn queue(action: Action) {
    let mut state = STATE.lock().unwrap();
    state.enqueue(action);
}
impl State {
    /// A nested confirmation temporarily owns input. Returning to the same
    /// native owner should retain the row/node, including after item-list
    /// refresh, rather than jumping to row zero outside a scrolled viewport.
    fn return_selection(&mut self,next:Context)->Option<String> {
        // Popup animations hide/disable the root before the popup's first
        // draw, and can redraw that root before it is interactive again. Save
        // at the first loss of readiness; consume only after the same root is
        // ready. Consuming on its first hidden draw lost scrolled-row focus.
        if let Some(old)=self.context.filter(|old|old.ready && matches!(old.kind,Kind::Store|Kind::Bench)) {
            if !old.same_owner(next) || !next.ready {
                if let Some(id)=selected(self).map(|c|c.button.id.clone()) {self.return_focus=Some((old,id));}
            }
        }
        if matches!(next.kind,Kind::Store|Kind::Bench) && next.ready {
            return self.return_focus.take().and_then(|(owner,id)|owner.same_owner(next).then_some(id));
        }
        if !matches!(next.kind,Kind::Popup|Kind::Store|Kind::Bench) {self.return_focus=None;}
        None
    }
    fn enqueue(&mut self, action: Action) {
        if let Some(context) = self.context.filter(|c| c.ready) {
            if self.requests.len() < 16 {
                self.requests.push((context, action));
            }
        }
    }
}
pub(super) fn hover(point: Point) {
    let mut state = STATE.lock().unwrap();
    if let Some(index) = state.controls.iter().position(|c| {
        let r = c.button.rect;
        point.x >= r[0] && point.x <= r[0] + r[2] && point.y >= r[1] && point.y <= r[1] + r[3]
    }) {
        if !super::fixed_action(&state.controls[index].button.id) {
            state.focus = index;
            state.dirty = true;
        }
    }
}
fn visible(rect: [f32; 4]) -> bool {
    rect.iter().all(|n| n.is_finite())
        && rect[2] > 0.
        && rect[3] > 0.
        && rect[0] < 480.
        && rect[1] < 320.
        && rect[0] + rect[2] > 0.
        && rect[1] + rect[3] > 0.
}
pub(super) fn cursor() -> Option<Point> {
    let state = STATE.lock().unwrap();
    state.context.filter(|c| c.ready)?;
    selected(&state)
        .filter(|c| !super::fixed_action(&c.button.id))
        .filter(|c| visible(c.button.rect))
        .map(|c| c.button.center())
}
pub(super) fn focus_rect() -> Option<[f32; 4]> {
    let state = STATE.lock().unwrap();
    state.context.filter(|c| c.ready)?;
    selected(&state)
        // Fixed E/Q/A/X actions retain their adjacent key sprites, but never
        // acquire the directional content selector (including popup defaults).
        .filter(|c| !super::fixed_action(&c.button.id))
        .filter(|c| visible(c.button.rect))
        .map(|c| clipped(c))
}
fn clipped(c: &Control) -> [f32; 4] {
    let r = c.button.rect;
    let Some(p) = c.clip else { return r };
    let x = r[0].max(p[0]);
    let y = r[1].max(p[1]);
    [
        x,
        y,
        (r[0] + r[2]).min(p[0] + p[2]) - x,
        (r[1] + r[3]).min(p[1] + p[3]) - y,
    ]
}
pub(super) fn can_cancel() -> bool {
    STATE
        .lock()
        .unwrap()
        .controls
        .iter()
        .any(|c| matches!(c.button.id.as_str(), "BTN_BACK" | "BTN_NO" | "BTN_PLAY_PAUSE"))
}
pub(super) fn action_rects() -> Vec<([f32; 4], bool)> {
    let state = STATE.lock().unwrap();
    let mut out = Vec::new();
    if !state.context.is_some_and(|c| c.ready) {
        return out;
    }
    let confirm = ["BTN_OK", "BTN_PURCHASE", "BTN_UPGRADE"]
        .iter()
        .find_map(|id| state.controls.iter().find(|c| c.button.id == *id))
        .or_else(|| selected(&state));
    if let Some(c) = confirm {
        out.push((c.button.rect, false));
    }
    if let Some(c) = state
        .controls
        .iter()
        .find(|c| matches!(c.button.id.as_str(), "BTN_BACK" | "BTN_NO" | "BTN_PLAY_PAUSE"))
    {
        out.push((c.button.rect, true));
    }
    out.retain(|(rect, _)| visible(*rect));
    out
}

fn select_native(env: &mut Environment, control: &Control)->bool {
    match control.kind {
        ControlKind::StoreRow(panel, index) => {
            let _: () = GuestFunction::from_addr_and_thumb_flag(0x0029_88ec, false)
                .call_from_host(env, (panel, index));
        }
        ControlKind::BenchRow(panel, index) => {
            // Pointer selection normally updates each button before the panel
            // callback. Calling only SetSelectedItem updated the stats but left
            // the first weapon's native selected border lit indefinitely.
            if let Some((begin,end))=word(env,panel+0x60).zip(word(env,panel+0x64)) {
                if end>=begin && end-begin<=8*128 && (end-begin)%8==0 {
                    for i in 0..(end-begin)/8 {
                        let widget=word(env,begin+i*8).unwrap_or(0);
                        if word(env,widget)==Some(0x00a8_8628) {
                            let _:()=GuestFunction::from_addr_and_thumb_flag(0x001c_da9c,false).call_from_host(env,(widget,i==index));
                        }
                    }
                }
            }
            let _: () = GuestFunction::from_addr_and_thumb_flag(0x002a_5ce8, false)
                .call_from_host(env, (panel, index));
        }
        _ => (),
    }
    ensure_row_visible(env, control)
}

fn row_in_view(c:&Control)->bool {
    let Some(clip)=c.clip else{return true;};let r=c.button.rect;
    let axis=usize::from(!matches!(c.kind,ControlKind::BenchRow(..)));
    // Native StoreItem active bounds include 4 px of top border. At the
    // native last-row scroll clamp its final 3 px cannot enter the clip.
    // Treat that border as visible, or the pending navigation waits forever.
    r[axis]>=clip[axis]-4. && r[axis]+r[axis+2]<=clip[axis]+clip[axis+2]+4.
}

fn ensure_row_visible(env: &mut Environment, control: &Control)->bool {
    let window = control.window;
    if row_in_view(control){return false;}
    if window < 0x1000 {
        return false;
    }
    let buffer = env.mem.alloc(16);
    let _: () = GuestFunction::from_addr_and_thumb_flag(0x001d_f74c, false)
        .call_from_host(env, (buffer.to_bits(), window));
    let mut clip = [0.; 4];
    for i in 0..4 {
        clip[i] = f32::from_bits(word(env, buffer.to_bits() + i as u32 * 4).unwrap_or(0));
    }
    env.mem.free(buffer);
    if clip.iter().any(|n| !n.is_finite()) || clip[2] <= 16. || clip[3] <= 24. {
        return false;
    }
    let row = control.button.rect;
    let horizontal = matches!(control.kind, ControlKind::BenchRow(..));
    let axis = if horizontal { 0 } else { 1 };
    if row[axis] >= clip[axis] && row[axis] + row[axis + 2] <= clip[axis] + clip[axis + 2] {
        return false;
    }
    let center = [clip[0] + clip[2] / 2., clip[1] + clip[3] / 2.];
    // Move only the selected row's overflow. A fixed 65%-of-page swipe jumped
    // several rows when the first offscreen row needed a single cell of scroll.
    let delta = if row[axis] < clip[axis] { clip[axis]-row[axis] }
        else { clip[axis]+clip[axis+2]-row[axis]-row[axis+2] };
    let mut end = center;
    end[axis] += delta.clamp(-clip[axis+2]*0.65,clip[axis+2]*0.65);
    env.window_mut().ds_queue_action(format!(
        "swipe {} {} {} {}",
        center[0], center[1], end[0], end[1]
    ));
    echo!("[DSnativeUI] reveal {} delta={} axis={}",control.button.id,delta,axis);
    true
}
fn activate(env: &mut Environment, context: Context, control: Control) {
    if !context.ready
        || word(env, control.button.widget) != Some(control.button.vtable)
        || byte(env, control.button.widget + 0x24) != Some(1)
    {
        return;
    }
    match control.kind {
        ControlKind::BenchNode => {
            // E/Enter/A is the fixed purchase action, not a focus target.
            // Select this node through its native validator, then activate the
            // native purchase control. The game retains connectivity, price,
            // cart validation and its confirmation popup; no currency writes.
            let was: bool = GuestFunction::from_addr_and_thumb_flag(0x001c_d9f4, false)
                .call_from_host(env, (control.button.widget,));
            if !was {
                let accepted: bool = GuestFunction::from_addr_and_thumb_flag(0x002a_0ab0, false)
                    .call_from_host(env, (control.button.widget, 0u32));
                if !accepted {
                    return;
                }
                let _: () = GuestFunction::from_addr_and_thumb_flag(0x001c_da9c, false)
                    .call_from_host(env, (control.button.widget, true));
            }
            let purchase = STATE
                .lock()
                .unwrap()
                .controls
                .iter()
                .find(|c| c.button.id == "BTN_PURCHASE")
                .cloned();
            if let Some(purchase) = purchase {
                activate(env, context, purchase);
            }
        }
        ControlKind::Button => {
            let function = word(env, control.button.vtable + 0x2c).unwrap_or(0);
            if (0x1000..0x8c0000).contains(&(function & !1)) {
                let _: () = GuestFunction::from_addr_with_thumb_bit(function)
                    .call_from_host(env, (control.button.widget,));
            }
        }
        _ => {select_native(env, &control);},
    }
    echo!(
        "[DSnativeUI] activated {:?} {}",
        context.kind,
        control.button.id
    );
}

pub(super) fn tick(env: &mut Environment) {
    if !cfg!(target_os = "windows") || TICKING.swap(true, Ordering::Relaxed) {
        return;
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            TICKING.store(false, Ordering::Relaxed);
        }
    }
    let _guard = Guard;
    let context = detect(env);
    let was_active = ACTIVE.swap(context.is_some(), Ordering::Relaxed);
    if context.is_some() && !was_active {
        // A gameplay request queued before the native popup opened must not
        // execute behind it, or wake up after it closes.
        use crate::window;
        for flag in [
            &window::DS_EKEY_INTERACT,
            &window::DS_FKEY_KINESIS,
            &window::DS_CKEY_STASIS,
            &window::DS_RKEY_RELOAD,
            &window::DS_XKEY_ROTATE,
            &window::DS_LOCATOR_REQUEST,
            &window::DS_QUICK_TURN_REQUEST,
            &window::DS_CONTEXT_COMBAT_REQUEST,
        ] {
            flag.store(false, Ordering::Relaxed);
        }
        window::DS_FIRE_MODE_REQUEST.store(0, Ordering::Relaxed);
        *window::DS_PC_INPUT.lock().unwrap() = window::ds_pc_input::Input::new();
    }
    let Some(context) = context else {
        let mut state = STATE.lock().unwrap();
        state.context = None;
        state.controls.clear();
        state.requests.clear();
        state.return_focus=None;
        return;
    };
    {
        let state = STATE.lock().unwrap();
        if state.frame == frame()
            && state
                .context
                .is_some_and(|old| old.same_owner(context) && old.ready == context.ready)
            && state.requests.is_empty()
            && !state.dirty
        {
            return;
        }
    }
    let controls = collect(env, context);
    let (requests, dirty, previous) = {
        let mut state = STATE.lock().unwrap();
        let changed = state.context.is_none_or(|old| !old.same_owner(context));
        let became_ready=context.ready && state.context.is_none_or(|old|!old.ready || !old.same_owner(context));
        let restored_id=state.return_selection(context);
        let restore_report=restored_id.clone();
        let selected_id=restored_id.or_else(||if changed {None}else{selected(&state).map(|c|c.button.id.clone())});
        if changed || (became_ready && context.kind==Kind::HudMenu) {
            echo!(
                "[DSnativeUI] owner {:?} object={:#x} handler={:#x} mode={} controls={}",
                context.kind,
                context.object,
                context.handler,
                context.mode,
                controls.len()
            );
            echo!(
                "[DSnativeUI] controls {:?}",
                controls
                    .iter()
                    .map(|c| (
                        &c.button.id,
                        c.button.widget,
                        c.button.vtable,
                        c.button.rect,
                        c.window
                    ))
                    .collect::<Vec<_>>()
            );
        }
        if changed {
            let kind=match context.kind {Kind::Popup=>0,Kind::Store=>1,Kind::Bench=>2,Kind::Pause=>3,Kind::HudMenu=>4,Kind::Transition=>5};
            crate::ds_diagnostics::event("native_modal_kind",[kind,context.mode as u64,context.ready as u64,0]);
            state.scroll_until=0;
        }
        state.context = Some(context);
        state.controls = controls;
        state.frame = frame();
        state.focus = selected_id
            .and_then(|id| state.controls.iter().position(|c| c.button.id == id).or_else(|| {
                // Selling the last stack can remove its row entirely. Keep the
                // nearest surviving row, instead of selecting an invalid item.
                let wanted=id.strip_prefix("ROW_")?.parse::<u32>().ok()?;
                state.controls.iter().enumerate().filter_map(|(slot,c)|match c.kind {
                    ControlKind::StoreRow(_,index)|ControlKind::BenchRow(_,index)=>Some((slot,index.abs_diff(wanted))),
                    _=>None,
                }).min_by_key(|&(_,distance)|distance).map(|(slot,_)|slot)
            }))
            .or_else(||state.controls.iter().position(|c|match c.kind {
                ControlKind::StoreRow(_,index)=>word(env,c.window+0xbc)==Some(index),
                ControlKind::BenchRow(_,index)=>word(env,c.window+0xa8)==Some(index),
                _=>false,
            }))
            .unwrap_or_else(|| {
                state
                    .controls
                    .iter()
                    .position(|c| {
                        if context.kind == Kind::Popup {
                            c.button.id == "BTN_OK"
                        } else {
                            matches!(
                                c.kind,
                                ControlKind::StoreRow(_, _)
                                    | ControlKind::BenchRow(_, _)
                                    | ControlKind::BenchNode
                            )
                        }
                    })
                    .or_else(|| {
                        state
                            .controls
                            .iter()
                            .position(|c| !super::fixed_action(&c.button.id))
                    })
                    .unwrap_or(usize::MAX)
            });
        if became_ready && selected(&state).is_some_and(|c|matches!(c.kind,ControlKind::StoreRow(..)|ControlKind::BenchRow(..))) {
            state.dirty=true;
        }
        if let Some(id)=restore_report {
            if let Some(control)=selected(&state) {
                echo!("[DSnativeUI] restored {:?} requested={} focus={} rect={:?} clip={:?}",context.kind,id,control.button.id,control.button.rect,control.clip);
            }
        }
        (
            std::mem::take(&mut state.requests),
            std::mem::take(&mut state.dirty),
            selected(&state).cloned(),
        )
    };
    if dirty {
        if let Some(control) = previous {
            if select_native(env,&control){STATE.lock().unwrap().scroll_until=frame()+12;}
        }
    }
    // Match frontend cancellation semantics: a cancel received in this frame
    // must win over a simultaneous confirm, including a purchase confirmation.
    let cancel = requests
        .iter()
        .find(|(owner, action)| owner.same_owner(context) && *action == Action::Cancel)
        .copied();
    let requests = cancel.map(|request| vec![request]).unwrap_or(requests);
    for (owner, action) in requests {
        if !owner.same_owner(context) || !context.ready {
            continue;
        }
        if matches!(context.kind,Kind::Store|Kind::Bench) && action!=Action::Cancel {
            let mut state=STATE.lock().unwrap();
            let hidden=selected(&state).filter(|c|matches!(c.kind,ControlKind::StoreRow(..)|ControlKind::BenchRow(..)))
                .filter(|c|!row_in_view(c)).cloned();
            if frame()<state.scroll_until || hidden.is_some() {
                state.requests.clear();state.requests.push((context,action));
                if frame()>=state.scroll_until {
                    state.scroll_until=frame()+12;drop(state);
                    if let Some(control)=hidden {ensure_row_visible(env,&control);}
                }
                continue;
            }
        }
        let mut activation = None;
        let mut selection = None;
        {
            let mut state = STATE.lock().unwrap();
            match action {
                Action::Confirm => {
                    let control = confirmation_control(&state);
                    activation = if control.as_ref().is_some_and(|c| {
                        matches!(
                            c.kind,
                            ControlKind::StoreRow(_, _) | ControlKind::BenchRow(_, _)
                        )
                    }) {
                        state
                            .controls
                            .iter()
                            .find(|c| {
                                matches!(c.button.id.as_str(), "BTN_PURCHASE" | "BTN_UPGRADE")
                            })
                            .cloned()
                            .or(control)
                    } else {
                        control
                    };
                }
                Action::Cancel => {
                    activation = state
                        .controls
                        .iter()
                        .find(|c| matches!(c.button.id.as_str(), "BTN_NO" | "BTN_BACK" | "BTN_PLAY_PAUSE"))
                        .cloned();
                }
                Action::Delete => {}, // Delete/Start is owned only by a native profile-delete control.
                _ => {
                    if context.kind==Kind::HudMenu {
                        if matches!(action,Action::Up|Action::Down) {
                            let choices:Vec<_>=state.controls.iter().enumerate().filter(|(_,c)|!super::fixed_action(&c.button.id)).map(|(i,_)|i).collect();
                            let current=choices.iter().position(|i|*i==state.focus).unwrap_or(0);
                            let next=if action==Action::Up {current.saturating_sub(1)}else{(current+1).min(choices.len().saturating_sub(1))};
                            if let Some(index)=choices.get(next){state.focus=*index;}
                        } else if matches!(action,Action::Left|Action::Right) {
                            let selected=selected(&state).cloned();
                            if let Some(control)=selected {
                                if matches!(control.kind,ControlKind::HudSlider) {drop(state);adjust_hud_slider(env,context.handler,&control,action==Action::Right);continue;}
                                let id=if action==Action::Left {"BTN_ARROW_LEFT"}else{"BTN_ARROW_RIGHT"};
                                activation=if control.button.id.starts_with("BTN_ARROW_") {state.controls.iter().find(|c|c.button.id==id).cloned()}else{Some(control)};
                            }
                        }
                    } else {
                    let is_row = selected(&state).is_some_and(|c| {
                        matches!(
                            c.kind,
                            ControlKind::StoreRow(_, _) | ControlKind::BenchRow(_, _)
                        )
                    });
                    if context.kind == Kind::Store
                        && is_row
                        && matches!(action, Action::Left | Action::Right)
                    {
                        let tabs = ["BTN_SHOP", "BTN_INVENTORY"];
                        let current = word(env, context.window + 0xa4)
                            .and_then(|p| word(env, p + 0x70))
                            .unwrap_or(0) as i32;
                        let step = if action == Action::Left { -1 } else { 1 };
                        let mut next = current + step;
                        while (0..2).contains(&next) {
                            if let Some(tab) = state
                                .controls
                                .iter()
                                .find(|c| c.button.id == tabs[next as usize])
                            {
                                activation = Some(tab.clone());
                                break;
                            }
                            next += step;
                        }
                    } else {
                        let buttons: Vec<Button> =
                            state.controls.iter().map(|c| c.button.clone()).collect();
                        state.focus = nearest(&buttons, state.focus, action);
                        selection = selected(&state).cloned();
                    }
                    }
                }
            }
        }
        if let Some(control) = selection {
            if select_native(env,&control){STATE.lock().unwrap().scroll_until=frame()+12;}
        }
        if let Some(control) = activation {
            activate(env, context, control);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_last_row_native_border_cannot_deadlock_navigation() {
        let mut c=Control{button:Button{id:"ROW_5".into(),widget:0x1000,vtable:0,rect:[19.,239.,468.,27.]},
            kind:ControlKind::StoreRow(0x2000,5),window:0x3000,clip:Some([13.,153.,438.,110.])};
        assert!(row_in_view(&c));
        c.button.rect[1]=270.;assert!(!row_in_view(&c));
        c.button.rect[1]=236.;assert!(row_in_view(&c));
    }
    #[test]
    fn cancel_hover_does_not_redirect_fixed_confirmation() {
        let mut state=State{context:None,controls:Vec::new(),focus:0,requests:Vec::new(),dirty:false,frame:0,return_focus:None,scroll_until:0};
        state.controls=["BTN_NO","BTN_OK"].into_iter().map(|id|Control{button:Button{id:id.into(),widget:0,vtable:0,rect:[0.,0.,30.,30.]},kind:ControlKind::Button,window:0,clip:None}).collect();
        state.focus=0;
        assert_eq!(confirmation_control(&state).unwrap().button.id,"BTN_OK");
        state.context=Some(context(Kind::HudMenu,true));state.controls[0].button.id="BTN_TOGGLE".into();
        assert_eq!(confirmation_control(&state).unwrap().button.id,"BTN_OK");
    }

    #[test]
    fn disabled_plus_cannot_turn_right_into_minus() {
        let mut points=[(Point::new(400.,100.),false),(Point::new(200.,100.),true)];
        assert_eq!(slider_point(&mut points,true),None);
        assert_eq!(slider_point(&mut points,false),Some(Point::new(200.,100.)));
        assert_eq!(slider_point(&mut points[..1],true),None);
    }

    #[test]
    fn options_slidefill_and_translated_footer_use_native_parent_space() {
        assert!(hud_slider_type(SLIDE_FILL_VT));assert!(!hud_slider_type(SLIDER_BUTTON_VT));
        assert_eq!(translate_rect([202.,24.,29.,31.],0,54),[202.,78.,29.,31.]);
    }

    #[test]
    fn an_unstarted_options_animator_is_not_an_interactive_layout() {
        assert!(!layout_ready(1,true));assert!(!layout_ready(3,true));
        assert!(layout_ready(2,true));assert!(!layout_ready(2,false));
    }

    #[test]
    fn native_pause_two_is_ready_after_animation_without_waiting_for_three() {
        assert!(pause_ready(2,true,false));assert!(pause_ready(3,true,false));
        assert!(!pause_ready(2,true,true));assert!(!pause_ready(2,false,false));
        assert!(!pause_ready(0,true,false));
    }
    fn context(kind: Kind, ready: bool) -> Context {
        Context {
            kind,
            object: 0x1000,
            handler: 0x2000,
            window: 0,
            world: 0x3000,
            mode: 2,
            ready,
        }
    }
    #[test]
    fn opening_press_cannot_replay_into_a_new_popup_and_transitions_drop_actions() {
        let root = context(Kind::Store, true);
        let popup = context(Kind::Popup, true);
        let mut state = State {
            context: Some(root),
            controls: Vec::new(),
            focus: 0,
            requests: Vec::new(),
            dirty: false,
            frame: 0,
            return_focus: None,
            scroll_until:0,
        };
        state.enqueue(Action::Confirm);
        assert_eq!(state.requests.len(), 1);
        assert!(!state.requests[0].0.same_owner(popup));
        state.requests.clear();
        state.context = Some(context(Kind::Popup, false));
        state.enqueue(Action::Confirm);
        assert!(state.requests.is_empty());
        state.context = Some(popup);
        state.enqueue(Action::Confirm);
        assert!(state.requests[0].0.same_owner(popup));
        state.requests.clear();
        state.context = Some(context(Kind::Transition, false));
        state.enqueue(Action::Cancel);
        assert!(state.requests.is_empty());
    }
    #[test]
    fn nested_confirmation_restores_its_row_but_does_not_cross_worlds() {
        let root=context(Kind::Store,true);
        let popup=context(Kind::Popup,true);
        let mut state=State {context:Some(root),controls:vec![Control {
            button:Button{id:"ROW_8".into(),widget:0x4000,vtable:0x5000,rect:[19.,184.,100.,27.]},
            kind:ControlKind::StoreRow(0x6000,8),window:0x7000,clip:None,
        }],focus:0,requests:Vec::new(),dirty:false,frame:0,return_focus:None,scroll_until:0};
        assert_eq!(state.return_selection(popup),None);
        state.context=Some(popup);
        let dismissing=Context{mode:0,..popup};
        assert_eq!(state.return_selection(dismissing),None);
        state.context=Some(dismissing);
        assert_eq!(state.return_selection(root).as_deref(),Some("ROW_8"));
        assert!(state.return_focus.is_none());
        state.context=Some(root);
        state.return_selection(popup);
        state.context=Some(popup);
        assert_eq!(state.return_selection(Context{world:0x9000,..root}),None);
        assert!(state.return_focus.is_none());
    }
    #[test]
    fn confirmation_animation_cannot_consume_focus_before_root_is_ready() {
        let root=context(Kind::Store,true);
        let hidden=Context{ready:false,..root};
        let popup=context(Kind::Popup,true);
        let mut state=State {context:Some(root),controls:vec![Control {
            button:Button{id:"ROW_8".into(),widget:0x4000,vtable:0x5000,rect:[19.,184.,100.,27.]},
            kind:ControlKind::StoreRow(0x6000,8),window:0x7000,clip:None,
        }],focus:0,requests:Vec::new(),dirty:false,frame:0,return_focus:None,scroll_until:0};
        assert_eq!(state.return_selection(hidden),None);
        state.context=Some(hidden);
        state.controls.clear();
        assert_eq!(state.return_selection(popup),None);
        state.context=Some(popup);
        assert_eq!(state.return_selection(hidden),None);
        assert!(state.return_focus.is_some());
        state.context=Some(hidden);
        assert_eq!(state.return_selection(root).as_deref(),Some("ROW_8"));
        assert!(state.return_focus.is_none());
    }
}

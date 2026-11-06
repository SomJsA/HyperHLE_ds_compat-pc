//! Native WidgetHandler navigation for the verified Dead Space ARM binary.
//! GetActiveRect resolves floating-point bounds through the layout entities.
//! GetLayoutWidget uses UTF-32 key / shared_ptr nodes at WidgetHandler+0x38.
use super::{Action, Point};
use crate::{
    abi::{CallFromHost, GuestFunction},
    mem::ConstVoidPtr,
    Environment,
};
use std::sync::Mutex;
#[path = "ds_frontend_modal.rs"]
mod modal;

pub(super) fn modal_active()->bool {modal::active()}
pub(super) fn modal_verified(env:&Environment)->bool {modal::verified(env)}
pub(super) fn observe_handler(env:&Environment,handler:u32){modal::observe_handler(env,handler);}

const CARD_PAGE_SIZE: usize = 2;
const CARD_PAGE_SETTLE_TICKS: u8 = 6;
const CARD_PAGE_STALE_TICKS: u8 = 24;
const GET_ACTIVE_RECT: u32 = 0x001e_0488;
const SET_SELECTED: u32 = 0x001c_da9c;
const SURVIVAL_ITEM_SEND_ACTION: u32 = 0x001d_66e4;
const CHAPTER_ITEM_SEND_ACTION: u32 = 0x001d_7200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardKind {
    Survival,
    Chapter,
}
impl CardKind {
    fn for_page(page: u32) -> Option<Self> {
        match page {
            10 | 11 => Some(Self::Survival),
            12 => Some(Self::Chapter),
            _ => None,
        }
    }
    fn window_name(page: u32) -> Option<&'static str> {
        match page {
            10 => Some("SURVIVAL_TIMED_WINDOW"),
            11 => Some("SURVIVAL_ENDLESS_WINDOW"),
            12 => Some("SURVIVAL_CHAPTER_WINDOW"),
            _ => None,
        }
    }
    fn send_action(self) -> u32 {
        match self {
            Self::Survival => SURVIVAL_ITEM_SEND_ACTION,
            Self::Chapter => CHAPTER_ITEM_SEND_ACTION,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Survival => "survival",
            Self::Chapter => "chapter",
        }
    }
}

#[derive(Clone, Debug)]
struct Button {
    id: String,
    widget: u32,
    vtable: u32,
    rect: [f32; 4],
}
impl Button {
    fn center(&self) -> Point {
        Point::new(
            self.rect[0] + self.rect[2] / 2.0,
            self.rect[1] + self.rect[3] / 2.0,
        )
    }
    fn selector(&self) -> bool {
        self.card_index().is_some()
            || matches!(
                self.id.as_str(),
                "BTN_PROFILE1"
                    | "BTN_PROFILE2"
                    | "BTN_PROFILE3"
                    | "BTN_EASY"
                    | "BTN_NORMAL"
                    | "BTN_HARD"
                    | "BTN_NIGHTMARE"
                    | "BTN_STORY"
                    | "BTN_CHAPTER"
                    | "BTN_ENDLESS"
                    | "BTN_TIMED"
            )
    }
    fn card_index(&self) -> Option<usize> {
        self.id.strip_prefix("CARD_")?.parse().ok()
    }
}

#[derive(Clone, Debug)]
struct NamedWidget {
    id: String,
    widget: u32,
    vtable: u32,
    rect: [f32; 4],
    active: bool,
}

#[derive(Clone, Debug)]
struct CardWindow {
    kind: CardKind,
    window: u32,
    page: u32,
    page_count: u32,
    cards: Vec<Button>,
}

#[derive(Clone, Debug)]
struct PagePending {
    window: u32,
    target_page: u32,
    target_focus: usize,
    settle_ticks: u8,
    stale_ticks: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingStep {
    Wait,
    Ready(usize),
    Abandon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardStep {
    Stay,
    Move(usize),
    Turn { action: Action, target_focus: usize },
}

#[derive(Clone, Debug)]
struct SelectionWork {
    previous: Option<(u32, u32)>,
    selected: Option<(u32, u32)>,
    mode: Option<MenuWork>,
}

#[derive(Clone, Debug)]
enum Activation {
    Button(Button),
    Card { kind: CardKind, button: Button },
}

#[derive(Clone, Debug)]
struct MenuWork {
    owner: u32,
    difficulty: bool,
    page: u32,
    activation: Activation,
}

struct Live {
    owner: u32,
    owner_type: u32,
    difficulty: bool,
    page: u32,
    frame: u64,
    buttons: Vec<Button>,
    card_window: Option<CardWindow>,
    focus: usize,
    cursor: bool,
    logged: bool,
    geometry_logged: bool,
    requests: Vec<(u32, u32, Action)>,
    pending: Option<PagePending>,
    selection_dirty: bool,
    synced_widget: Option<(u32,u32)>,
}
static LIVE: Mutex<Live> = Mutex::new(Live {
    owner: 0,
    owner_type: 0,
    difficulty: false,
    page: super::UNKNOWN,
    frame: 0,
    buttons: Vec::new(),
    card_window: None,
    focus: 0,
    cursor: false,
    logged: false,
    geometry_logged: false,
    requests: Vec::new(),
    pending: None,
    selection_dirty: false,
    synced_widget: None,
});

pub(super) fn reset() {
    modal::reset();
    let mut live = LIVE.lock().unwrap();
    live.owner = 0;
    live.buttons.clear();
    live.card_window = None;
    live.requests.clear();
    live.pending = None;
    live.selection_dirty = false;
    live.synced_widget = None;
    live.cursor = false;
}

fn word(env: &Environment, at: u32) -> Option<u32> {
    Some(u32::from_le_bytes(
        env.mem
            .get_bytes_fallible(ConstVoidPtr::from_bits(at), 4)?
            .try_into()
            .ok()?,
    ))
}
fn byte(env: &Environment, at: u32) -> Option<u8> {
    env.mem
        .get_bytes_fallible(ConstVoidPtr::from_bits(at), 1)?
        .first()
        .copied()
}

fn initial_rect(env: &Environment, widget: u32, handler: u32) -> [f32; 4] {
    let mut rect = [0.; 4];
    for i in 0..4 {
        rect[i] = f32::from_bits(word(env, widget + 0x44 + i as u32 * 4).unwrap_or(0));
    }
    rect[0] += word(env, handler + 4).unwrap_or(0) as i32 as f32;
    rect[1] += word(env, handler + 8).unwrap_or(0) as i32 as f32;
    rect
}

fn named_widgets(env: &Environment, handler: u32) -> Vec<NamedWidget> {
    named_widgets_map(env, handler+0x38, handler)
}

fn named_widgets_map(env: &Environment, map: u32, handler: u32) -> Vec<NamedWidget> {
    let mut out = Vec::new();
    let Some(table) = word(env, map) else {
        return out;
    };
    let Some(count) = word(env, map + 4).filter(|n| (1..=1024).contains(n)) else {
        return out;
    };
    let sentinel = word(env, table + count * 4).unwrap_or(0);
    for bucket in 0..count {
        let mut node = word(env, table + bucket * 4).unwrap_or(0);
        for _ in 0..128 {
            if node == 0 || node == sentinel {
                break;
            }
            let Some(begin) = word(env, node) else { break };
            let Some(end) = word(env, node + 4) else {
                break;
            };
            let len = end.saturating_sub(begin);
            if (4..=160).contains(&len) && len % 4 == 0 {
                if let Some(raw) = env
                    .mem
                    .get_bytes_fallible(ConstVoidPtr::from_bits(begin), len)
                {
                    let id: Option<String> = raw
                        .chunks_exact(4)
                        .map(|c| char::from_u32(u32::from_le_bytes(c.try_into().unwrap())))
                        .collect();
                    if let Some(id) = id {
                        let widget = word(env, node + 0x14).unwrap_or(0);
                        let vtable = word(env, widget).unwrap_or(0);
                        if widget != 0 && vtable != 0 {
                            out.push(NamedWidget {
                                id,
                                widget,
                                vtable,
                                rect: if handler==0 {[0.;4]}else{initial_rect(env, widget, handler)},
                                active: byte(env, widget + 0x24) == Some(1),
                            });
                        }
                    }
                }
            }
            let next = word(env, node + 0x1c).unwrap_or(0);
            if next == node {
                break;
            }
            node = next;
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

fn read_card_window(
    env: &Environment,
    handler: u32,
    page: u32,
    named: &[NamedWidget],
) -> Option<CardWindow> {
    // Android's native touch UI already owns card paging and focus. Virtual
    // card focus/auto-page is a PC controller feature only.
    if cfg!(target_os = "android") {
        return None;
    }
    let kind = CardKind::for_page(page)?;
    let name = CardKind::window_name(page)?;
    let window = named.iter().find(|w| w.id == name)?.widget;
    let panel = word(env, window + 0x70).filter(|p| *p != 0)?;
    let begin = word(env, panel + 0x60).filter(|p| *p != 0)?;
    let end = word(env, panel + 0x64)?;
    let bytes = end.checked_sub(begin)?;
    if bytes == 0 || bytes % 8 != 0 {
        return None;
    }
    let count = (bytes / 8) as usize;
    if !(1..=64).contains(&count) {
        return None;
    }

    let native_page = word(env, window + 0xa8)?;
    let actual_pages = ((count + CARD_PAGE_SIZE - 1) / CARD_PAGE_SIZE) as u32;
    let stored_pages = word(env, window + 0xac).unwrap_or(actual_pages);
    let page_count = stored_pages.clamp(1, actual_pages);
    let current_page = native_page.min(page_count - 1);
    let mut cards = Vec::with_capacity(count);
    for i in 0..count {
        let Some(at) = begin.checked_add((i as u32) * 8) else {
            break;
        };
        let Some(item) = word(env, at).filter(|p| *p != 0) else {
            continue;
        };
        let Some(vtable) = word(env, item).filter(|v| *v != 0) else {
            continue;
        };
        cards.push(Button {
            id: format!("CARD_{i}"),
            widget: item,
            vtable,
            rect: initial_rect(env, item, handler),
        });
    }
    // The survival/chapter item widgets expose a shared base rect on the
    // current build. The panel is still two columns, so use the verified
    // two-column slots only when every item reports the same geometry.
    let shared_rect = cards.first().map(|card| card.rect);
    if shared_rect.is_some_and(|rect| cards.iter().all(|card| card.rect == rect)) {
        for (i, card) in cards.iter_mut().enumerate() {
            card.rect = if i % CARD_PAGE_SIZE == 0 {
                [90.0, 58.0, 155.0, 170.0]
            } else {
                [270.0, 58.0, 155.0, 170.0]
            };
        }
    }
    if cards.is_empty() {
        return None;
    }
    Some(CardWindow {
        kind,
        window,
        page: current_page,
        page_count,
        cards,
    })
}

fn scan_widgets(env: &Environment, handler: u32, page: u32) -> (Vec<Button>, Option<CardWindow>) {
    let named = named_widgets(env, handler);
    let mut buttons: Vec<Button> = named
        .iter()
        .filter(|w| w.active && w.id.starts_with("BTN_"))
        .map(|w| Button {
            id: w.id.clone(),
            widget: w.widget,
            vtable: w.vtable,
            rect: w.rect,
        })
        .collect();
    let card_window = read_card_window(env, handler, page, &named);
    if let Some(card) = &card_window {
        buttons.extend(card.cards.iter().cloned());
    }
    buttons.sort_by(|a, b| {
        a.rect[1]
            .total_cmp(&b.rect[1])
            .then(a.rect[0].total_cmp(&b.rect[0]))
            .then(a.id.cmp(&b.id))
    });
    buttons.dedup_by(|a, b| a.id == b.id);
    (buttons, card_window)
}

fn menu_ready(env: &Environment, owner: u32, page: u32, difficulty: bool) -> bool {
    if difficulty {
        word(env, owner + 0x48) == Some(0)
    } else {
        word(env, owner + 0x50) == Some(page) && word(env, owner + 0x140) == Some(2)
    }
}

fn native_card_focus(env: &Environment, card: &CardWindow) -> Option<usize> {
    card.cards.iter().find_map(|button| {
        let index = button.card_index()?;
        (word(env, button.widget + 0x68).unwrap_or(0) != 0
            && word(env, button.widget + 0x60) == Some(4))
        .then_some(index)
    })
}

fn card_button(card: &CardWindow, index: usize) -> Option<&Button> {
    card.cards
        .iter()
        .find(|button| button.card_index() == Some(index))
}

fn restore_card_geometry(card:&CardWindow,geometry:&mut [(u32,[f32;4])]) {
    // Item GetActiveRect is local to its repeated parent slot. Calling it
    // outside that draw transform returns the same origin for every card;
    // do not overwrite the verified two-column global slots with [0,0,...].
    let rects:Vec<_>=card.cards.iter().filter_map(|b|geometry.iter().find(|(w,_)|*w==b.widget).map(|(_,r)|*r)).collect();
    if rects.len()>1 && rects.iter().all(|r|*r==rects[0]) {
        for (widget,rect) in geometry {
            if let Some(button)=card.cards.iter().find(|b|b.widget==*widget) {*rect=button.rect;}
        }
    }
}

fn card_indices(card: &CardWindow) -> Vec<usize> {
    card.cards.iter().filter_map(Button::card_index).collect()
}

fn card_on_page(index: usize, page: u32) -> bool {
    index / CARD_PAGE_SIZE == page as usize
}

fn default_card_focus(
    selected: Option<usize>,
    previous: Option<usize>,
    page: u32,
    indices: &[usize],
) -> Option<usize> {
    selected
        .filter(|i| card_on_page(*i, page) && indices.contains(i))
        .or_else(|| previous.filter(|i| card_on_page(*i, page) && indices.contains(i)))
        .or_else(|| indices.iter().copied().find(|i| card_on_page(*i, page)))
        .or_else(|| indices.first().copied())
}

fn focus_card(live: &Live, index: usize) -> Option<usize> {
    live.buttons
        .iter()
        .position(|button| button.card_index() == Some(index))
}

fn set_card_focus(live: &mut Live, index: usize) -> bool {
    let Some(focus) = focus_card(live, index) else {
        return false;
    };
    let changed = live.focus != focus;
    live.focus = focus;
    changed
}

fn card_navigation(
    action: Action,
    focus: usize,
    current_page: u32,
    page_count: u32,
    indices: &[usize],
) -> CardStep {
    if !indices.contains(&focus) || !card_on_page(focus, current_page) {
        return CardStep::Stay;
    }
    let start = current_page as usize * CARD_PAGE_SIZE;
    let end = start.saturating_add(CARD_PAGE_SIZE);
    match action {
        Action::Right => {
            if let Some(next) = indices.iter().copied().find(|i| *i > focus && *i < end) {
                return CardStep::Move(next);
            }
            if current_page + 1 < page_count {
                if let Some(target) = indices
                    .iter()
                    .copied()
                    .find(|i| *i >= end && card_on_page(*i, current_page + 1))
                {
                    return CardStep::Turn {
                        action,
                        target_focus: target,
                    };
                }
            }
        }
        Action::Left => {
            if let Some(previous) = indices
                .iter()
                .rev()
                .copied()
                .find(|i| *i < focus && *i >= start)
            {
                return CardStep::Move(previous);
            }
            if current_page > 0 {
                if let Some(target) = indices
                    .iter()
                    .rev()
                    .copied()
                    .find(|i| *i < start && card_on_page(*i, current_page - 1))
                {
                    return CardStep::Turn {
                        action,
                        target_focus: target,
                    };
                }
            }
        }
        _ => {}
    }
    CardStep::Stay
}

fn advance_pending(pending: &mut PagePending, current_page: u32) -> PendingStep {
    if current_page == pending.target_page {
        if pending.settle_ticks > 0 {
            pending.settle_ticks -= 1;
            PendingStep::Wait
        } else {
            PendingStep::Ready(pending.target_focus)
        }
    } else if pending.stale_ticks >= CARD_PAGE_STALE_TICKS {
        PendingStep::Abandon
    } else {
        pending.stale_ticks += 1;
        PendingStep::Wait
    }
}

pub(super) fn observe(env: &Environment, owner: u32, page: u32, difficulty: bool) {
    let ready = if difficulty {
        word(env, owner + 0x48) == Some(0)
    } else {
        word(env, owner + 0x140) == Some(2)
    };
    let (mut buttons, card_window) = if ready {
        scan_widgets(env, owner + if difficulty { 0x50 } else { 0xb8 }, page)
    } else {
        (Vec::new(), None)
    };
    let mut live = LIVE.lock().unwrap();
    let first_controls=live.buttons.is_empty() && !buttons.is_empty();
    let changed = live.owner != owner || live.page != page || live.difficulty != difficulty;
    let unfocused = !changed && live.focus == usize::MAX;
    let old_page = live.card_window.as_ref().map(|card| card.page);
    let old_card_focus = live.buttons.get(live.focus).and_then(Button::card_index);
    let selected_id = if changed {
        None
    } else {
        live.buttons.get(live.focus).map(|button| button.id.clone())
    };
    if !changed {
        for button in &mut buttons {
            if let Some(old) = live.buttons.iter().find(|old| old.widget == button.widget) {
                button.rect = old.rect;
            }
        }
    }
    if changed {
        live.cursor = false;
        live.requests.clear();
        live.pending = None;
        live.selection_dirty = false;
        live.synced_widget = None;
        live.logged = false;
        live.geometry_logged = false;
    }
    live.owner = owner;
    live.owner_type = word(env, owner).unwrap_or(0);
    live.page = page;
    crate::ds_diagnostics::menu(crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),page);
    live.difficulty = difficulty;
    live.frame = super::SCENE_DIALOG_FRAME.load(std::sync::atomic::Ordering::Relaxed);
    live.buttons = buttons;

    if let Some(card) = card_window {
        let selected = native_card_focus(env, &card);
        let indices = card_indices(&card);
        let pending_target = live
            .pending
            .as_ref()
            .filter(|pending| pending.window == card.window && card.page == pending.target_page)
            .map(|pending| pending.target_focus);
        let pending_waiting = live
            .pending
            .as_ref()
            .is_some_and(|pending| pending.window == card.window);
        let target = if pending_target.is_some() {
            pending_target
        } else if pending_waiting || unfocused {
            None
        } else {
            let previous = if old_page == Some(card.page) {
                old_card_focus
            } else {
                None
            };
            default_card_focus(selected, previous, card.page, &indices)
        };
        if let Some(index) = target {
            if set_card_focus(&mut live, index) {
                live.selection_dirty = true;
            }
        }
        live.card_window = Some(card);
    } else {
        live.card_window = None;
        live.focus = if unfocused {
            usize::MAX
        } else {
            selected_id
                .and_then(|id| live.buttons.iter().position(|button| button.id == id))
                .or_else(|| (matches!(page,6|9)).then(||live.buttons.iter().position(|button| {
                    selector_value(page,&button.id).is_some_and(|(offset,mode,sub)|word(env,owner+offset)==Some(mode)
                        && sub.is_none_or(|value|word(env,owner+0x354)==Some(value)))
                })).flatten())
                .or_else(|| {
                    live.buttons.iter().position(|button| {
                        matches!(
                            button.id.as_str(),
                            "BTN_PLAY" | "BTN_NORMAL" | "BTN_STORY" | "BTN_PROFILE1" | "BTN_NO"
                        )
                    })
                })
                .unwrap_or(0)
        };
    }

    if page==5 && first_controls {
        if let Some(start)=live.buttons.iter().position(|b|b.id=="BTN_PLAY") {
            live.focus=start;live.cursor=true;live.selection_dirty=true;
        }
    }
    if !live.logged && !live.buttons.is_empty() {
        live.logged = true;
        echo!(
            "[DSfrontendWidgets] page={page} owner={owner:#x} buttons={:?}",
            live.buttons.iter().map(|b| &b.id).collect::<Vec<_>>()
        );
        echo!(
            "[DSfrontendWidgets] raw page={page} bounds={:?}",
            live.buttons.iter().map(|b| (&b.id, b.rect)).collect::<Vec<_>>()
        );
    }
    if let Some(card) = &live.card_window {
        echo!(
            "[DSfrontendCards] page={} kind={} cards={} visible_page={}/{}",
            live.page,
            card.kind.label(),
            card.cards.len(),
            card.page + 1,
            card.page_count
        );
    }
}

pub(super) fn queue(action: Action) {
    if modal::active(){modal::queue(action);return;}
    let mut live = LIVE.lock().unwrap();
    let owner = live.owner;
    let page = live.page;
    if live.requests.len() < 16 {
        live.requests.push((owner, page, action));
    }
}

pub(super) fn hover(point: Point) {
    if modal::active(){modal::hover(point);return;}
    let mut live = LIVE.lock().unwrap();
    live.cursor = false;
    live.focus = live
        .buttons
        .iter()
        .position(|button| {
            point.x >= button.rect[0]
                && point.x <= button.rect[0] + button.rect[2]
                && point.y >= button.rect[1]
                && point.y <= button.rect[1] + button.rect[3]
        })
        .unwrap_or(usize::MAX);
}

pub(super) fn cursor() -> Option<Point> {
    if modal::active(){return modal::cursor();}
    if !super::capture_menu() {
        return None;
    }
    let live = LIVE.lock().unwrap();
    if live.cursor {
        live.buttons.get(live.focus).map(Button::center)
    } else {
        None
    }
}

pub(super) fn can_cancel() -> bool {
    if modal::active(){return modal::can_cancel();}
    LIVE.lock().unwrap().buttons.iter().any(|button| matches!(button.id.as_str(), "BTN_BACK" | "BTN_NO"))
}

pub(super) fn action_rects()->Vec<([f32;4],bool)>{
    if modal::active(){return modal::action_rects();}
    if !super::capture_menu(){return Vec::new();}
    let live=LIVE.lock().unwrap();let mut out=Vec::new();
    let confirm=["BTN_OK","BTN_CONTINUE"].iter().find_map(|id|live.buttons.iter().find(|b|b.id==*id))
        .or_else(||live.buttons.get(live.focus).filter(|b|!matches!(b.id.as_str(),"BTN_BACK"|"BTN_NO")));
    // Card panels draw their confirm tick in the shared right footer; the
    // root map exposes paging arrows but no BTN_OK. Never anchor confirm to
    // a hovered page arrow, or make E turn a page instead of accepting a card.
    if live.card_window.is_some(){out.push(([391.,271.,60.,49.],false));}
    else if let Some(button)=confirm {out.push((button.rect,false));}
    if let Some(button)=live.buttons.iter().find(|b|matches!(b.id.as_str(),"BTN_BACK"|"BTN_NO")){out.push((button.rect,true));}
    out
}

pub(super) fn navigation_rect()->Option<[f32;4]>{
    if modal::active(){return modal::focus_rect();}
    if !super::capture_menu(){return None;}
    let live=LIVE.lock().unwrap();
    if live.buttons.len()<2{return None;}
    live.buttons.get(live.focus).filter(|b|b.selector()).or_else(||live.buttons.iter().find(|b|b.selector()))
        .or_else(||live.buttons.get(live.focus)).map(|b|b.rect)
}

pub(super) fn badges() -> Vec<(Point, bool)> {
    let live = LIVE.lock().unwrap();
    let mut out = Vec::new();
    if let Some(button) = live.buttons.get(live.focus) {
        out.push((
            Point::new(
                (button.rect[0] + button.rect[2] - 12.).clamp(2., 466.),
                (button.rect[1] - 11.).max(2.),
            ),
            false,
        ));
    }
    if let Some(button) = live
        .buttons
        .iter()
        .find(|button| button.id == "BTN_BACK" || button.id == "BTN_NO")
    {
        out.push((
            Point::new(button.rect[0] + 2., (button.rect[1] - 11.).max(2.)),
            true,
        ));
    }
    out
}

pub(super) fn focus_rect() -> Option<[f32; 4]> {
    if modal::active(){return modal::focus_rect();}
    if !super::capture_menu() {
        return None;
    }
    let live = LIVE.lock().unwrap();
    live.cursor
        .then(|| live.buttons.get(live.focus).map(|button| button.rect))
        .flatten()
}

pub(super) fn fixed_action(id:&str)->bool {
    matches!(id,"BTN_OK"|"BTN_BACK"|"BTN_NO"|"BTN_YES"|"BTN_PURCHASE"|"BTN_UPGRADE"|"BTN_CANCEL"|"BTN_DELETE"|"BTN_HOME"|"BTN_PLAY_PAUSE")
}

pub(super) fn delete_rect()->Option<[f32;4]> {
    if !super::capture_menu() || modal::active(){return None;}
    LIVE.lock().unwrap().buttons.iter().find(|b|matches!(b.id.as_str(),"BTN_DELETE"|"BTN_HOME")).map(|b|b.rect)
}

fn mode_value(id:&str)->Option<(u32,Option<u32>)> {
    Some(match id {
        "BTN_STORY"=>(2,None), "BTN_CHAPTER"=>(3,None),
        "BTN_ENDLESS"=>(1,Some(1)), "BTN_TIMED"=>(1,Some(2)), _=>return None,
    })
}

fn selector_value(page:u32,id:&str)->Option<(u32,u32,Option<u32>)> {
    if page==9 {return mode_value(id).map(|(mode,sub)|(0x350,mode,sub));}
    if page==6 {return Some((0x1a8,match id {"BTN_PROFILE1"=>0,"BTN_PROFILE2"=>1,"BTN_PROFILE3"=>2,_=>return None},None));}
    None
}

fn confirm_button(buttons:&[Button],focus:usize)->Option<Button> {
    // Mouse hover may focus the delete widget even though directions skip it.
    // E/Enter/A must still mean confirm; only Delete/Start owns deletion.
    buttons.iter().find(|b|b.id=="BTN_OK")
        .filter(|_|buttons.get(focus).is_none_or(|b|b.selector() || fixed_action(&b.id)))
        .or_else(||buttons.get(focus).filter(|b|b.id!="BTN_DELETE")).cloned()
}

fn nearest(list: &[Button], focus: usize, action: Action) -> usize {
    let Some(at) = list.get(focus).map(Button::center) else {
        return list.iter().position(|b|!fixed_action(&b.id)).unwrap_or(usize::MAX);
    };
    list.iter()
        .enumerate()
        .filter(|(_,button)| !fixed_action(&button.id))
        .filter_map(|(i, button)| {
            let point = button.center();
            let (dx, dy) = (point.x - at.x, point.y - at.y);
            let (along, across) = match action {
                Action::Up => (-dy, dx.abs()),
                Action::Down => (dy, dx.abs()),
                Action::Left => (-dx, dy.abs()),
                Action::Right => (dx, dy.abs()),
                _ => return None,
            };
            (along > 4. && along >= across * 0.5).then_some((i, along + across * 2.0))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|value| value.0)
        .unwrap_or(focus)
}

fn arrow_id(action: Action) -> Option<&'static str> {
    match action {
        Action::Right => Some("BTN_ARROW_RIGHT"),
        Action::Left => Some("BTN_ARROW_LEFT"),
        _ => None,
    }
}

fn take_selection_work(live: &mut Live) -> Option<SelectionWork> {
    if !live.selection_dirty {
        return None;
    }
    live.selection_dirty = false;
    let target = live
        .buttons
        .get(live.focus)
        .filter(|button| button.card_index().is_some() || button.selector())
        .map(|button| (button.widget, button.vtable));
    let previous = live.synced_widget;
    live.synced_widget = target;
    let previous_widget = (previous != target).then_some(previous).flatten();
    let selected_widget = target;
    if previous_widget.is_none() && selected_widget.is_none() {
        return None;
    }
    Some(SelectionWork {
        previous: previous_widget,
        selected: selected_widget,
        mode: live.buttons.get(live.focus).filter(|b|selector_value(live.page,&b.id).is_some())
            .map(|b|MenuWork{owner:live.owner,difficulty:live.difficulty,page:live.page,activation:Activation::Button(b.clone())}),
    })
}

fn apply_selection(env: &mut Environment, selection: &SelectionWork) {
    // SetSelected only paints a border. Page 9 BTN_OK reads owner+350/+354;
    // profile confirm/delete reads owner+1a8. Both must follow native events.
    // Run the original mode button event once when that native selection differs;
    // sending it again for an already-selected mode would act like a double click.
    if let Some(work)=&selection.mode {
        if let Activation::Button(button)=&work.activation {
            if let Some((offset,mode,submode))=selector_value(work.page,&button.id) {
                if word(env,work.owner+offset)!=Some(mode)
                    || submode.is_some_and(|sub|word(env,work.owner+0x354)!=Some(sub)) {
                    activate(env,work);
                }
            }
        }
    }
    if let Some((widget, vtable)) = selection.previous {
        if word(env, widget) == Some(vtable) {
            let _: () = GuestFunction::from_addr_and_thumb_flag(SET_SELECTED, false)
                .call_from_host(env, (widget, false));
        }
    }
    if let Some((widget, vtable)) = selection.selected {
        if word(env, widget) == Some(vtable) {
            let _: () = GuestFunction::from_addr_and_thumb_flag(SET_SELECTED, false)
                .call_from_host(env, (widget, true));
        }
    }
}

fn activate(env: &mut Environment, work: &MenuWork) {
    if !menu_ready(env, work.owner, work.page, work.difficulty) {
        return;
    }
    let button = match &work.activation {
        Activation::Button(button) => button,
        Activation::Card { button, .. } => button,
    };
    if word(env, button.widget) != Some(button.vtable) || byte(env, button.widget + 0x24) != Some(1)
    {
        return;
    }
    match &work.activation {
        Activation::Button(button) => {
            let function = word(env, button.widget)
                .and_then(|vtable| word(env, vtable + 0x2c))
                .unwrap_or(0);
            if !(0x1000..0x8c0000).contains(&(function & !1)) {
                return;
            }
            let _: () = GuestFunction::from_addr_with_thumb_bit(function)
                .call_from_host(env, (button.widget,));
            echo!("[DSfrontendInput] native page={} {}", work.page, button.id);
        }
        Activation::Card { kind, button } => {
            let _: () = GuestFunction::from_addr_and_thumb_flag(kind.send_action(), false)
                .call_from_host(env, (button.widget,));
            echo!(
                "[DSfrontendInput] native page={} {} {} card={}",
                work.page,
                kind.label(),
                button.id,
                button.card_index().unwrap_or(usize::MAX)
            );
        }
    }
}

pub(super) fn tick(env: &mut Environment) {
    modal::tick(env);
    if modal::active(){return;}
    if !super::capture_menu() {
        let mut live = LIVE.lock().unwrap();
        live.requests.clear();
        live.pending = None;
        return;
    }
    let (owner, owner_type, difficulty, page, snapshot) = {
        let live = LIVE.lock().unwrap();
        (
            live.owner,
            live.owner_type,
            live.difficulty,
            live.page,
            live.buttons.clone(),
        )
    };
    if !menu_ready(env, owner, page, difficulty)
        || owner_type == 0
        || word(env, owner) != Some(owner_type)
    {
        LIVE.lock().unwrap().requests.clear();
        return;
    }

    // Resolve geometry on the normal host-call boundary, never inside a breakpoint.
    let rect_buffer = env.mem.alloc(16);
    let mut geometry = Vec::new();
    for button in snapshot {
        if word(env, button.widget) != Some(button.vtable) {
            continue;
        }
        let _: () = GuestFunction::from_addr_and_thumb_flag(GET_ACTIVE_RECT, false)
            .call_from_host(env, (rect_buffer.to_bits(), button.widget));
        let mut rect = [0.; 4];
        for i in 0..4 {
            rect[i] = f32::from_bits(word(env, rect_buffer.to_bits() + i as u32 * 4).unwrap_or(0));
        }
        geometry.push((button.widget, rect));
    }
    env.mem.free(rect_buffer);
    {
        let mut live = LIVE.lock().unwrap();
        if live.owner == owner {
            if let Some(card)=&live.card_window {restore_card_geometry(card,&mut geometry);}
            let unfocused = live.focus == usize::MAX;
            let selected = live.buttons.get(live.focus).map(|button| button.id.clone());
            for button in &mut live.buttons {
                if let Some((_, rect)) =
                    geometry.iter().find(|(widget, _)| *widget == button.widget)
                {
                    button.rect = *rect;
                }
            }
            // BTN_OK is a separate checkmark widget on the frontend pages and
            // GetActiveRect can report zero while it is still visibly drawn.
            // Keep its verified bottom-right hit rect navigable.
            for button in &mut live.buttons {
                if button.id == "BTN_OK"
                    && (button.rect.iter().any(|value| !value.is_finite())
                        || button.rect[2] <= 0.
                        || button.rect[3] <= 0.
                        || button.rect[0] >= 480.
                        || button.rect[1] >= 320.)
                {
                    button.rect = [391.0, 271.0, 60.0, 49.0];
                }
            }
            live.buttons.retain(|button| {
                button.rect.iter().all(|value| value.is_finite())
                    && button.rect[2] > 0.
                    && button.rect[3] > 0.
                    && button.rect[0] < 480.
                    && button.rect[1] < 320.
                    && button.rect[0] + button.rect[2] > 0.
                    && button.rect[1] + button.rect[3] > 0.
            });
            live.focus = if unfocused {
                usize::MAX
            } else {
                selected
                    .and_then(|id| live.buttons.iter().position(|button| button.id == id))
                    .unwrap_or(0)
            };
            if !live.geometry_logged && !live.buttons.is_empty() {
                live.geometry_logged = true;
                echo!(
                    "[DSfrontendWidgets] transformed page={} bounds={:?}",
                    live.page,
                    live.buttons
                        .iter()
                        .map(|button| (&button.id, button.rect))
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    let pending = { LIVE.lock().unwrap().pending.clone() };
    let mut pending_resolved = false;
    if let Some(mut pending) = pending {
        let current_page = word(env, pending.window + 0xa8).unwrap_or(u32::MAX);
        match advance_pending(&mut pending, current_page) {
            PendingStep::Wait => {
                let mut live = LIVE.lock().unwrap();
                live.pending = Some(pending);
                live.requests.clear();
                return;
            }
            PendingStep::Abandon => {
                let mut live = LIVE.lock().unwrap();
                live.pending = None;
                live.requests.clear();
                return;
            }
            PendingStep::Ready(target) => {
                let mut live = LIVE.lock().unwrap();
                live.pending = None;
                live.requests.clear();
                if set_card_focus(&mut live, target) {
                    live.selection_dirty = true;
                }
                pending_resolved = true;
            }
        }
    }

    let (selection, activation) = {
        let mut live = LIVE.lock().unwrap();
        let mut activation = None;
        if !pending_resolved {
            let requests = std::mem::take(&mut live.requests);
            let mut chosen = None;
            for (request_owner, request_page, action) in requests {
                if request_owner != live.owner
                    || request_page != live.page
                    || live.buttons.is_empty()
                {
                    continue;
                }
                // One menu action per frame. Cancel wins if Q/X and E/A arrive in
                // the same frame, so a cancel cannot be overwritten by confirm.
                if action == Action::Cancel {
                    chosen = Some((request_owner, request_page, action));
                    break;
                }
                if chosen.is_none() {
                    chosen = Some((request_owner, request_page, action));
                }
            }
            if let Some((work_owner, work_page, action)) = chosen {
                match action {
                    Action::Delete => {
                        if live.buttons.get(live.focus).is_some_and(|b|selector_value(live.page,&b.id).is_some()) {live.selection_dirty=true;}
                        if let Some(button)=live.buttons.iter().find(|b|matches!(b.id.as_str(),"BTN_DELETE"|"BTN_HOME")).cloned() {
                            activation=Some(MenuWork{owner:work_owner,difficulty:live.difficulty,page:work_page,activation:Activation::Button(button)});
                        }
                    }
                    Action::Cancel => {
                        let index = live
                            .buttons
                            .iter()
                            .position(|button| button.id == "BTN_BACK")
                            .or_else(|| {
                                live.buttons.iter().position(|button| button.id == "BTN_NO")
                            });
                        if let Some(button) = index.and_then(|i| live.buttons.get(i)).cloned() {
                            activation = Some(MenuWork {
                                owner: work_owner,
                                difficulty: live.difficulty,
                                page: work_page,
                                activation: Activation::Button(button),
                            });
                        }
                    }
                    Action::Confirm => {
                        // Mouse hover can change focus without a direction key.
                        // Synchronize its mode through the same native event
                        // before BTN_OK; never merely paint the selected border.
                        if live.buttons.get(live.focus).is_some_and(|b|selector_value(live.page,&b.id).is_some()) {
                            live.selection_dirty=true;
                        }
                        let card_focus = live.buttons.get(live.focus).and_then(Button::card_index)
                            .or_else(||live.card_window.as_ref().and_then(|card|native_card_focus(env,card)));
                        if let Some(card_focus) = card_focus {
                            if let Some(card) = live.card_window.clone() {
                                let native_page = word(env, card.window + 0xa8).unwrap_or(u32::MAX);
                                if live.pending.is_none()
                                    && native_page == card.page
                                    && card_on_page(card_focus, card.page)
                                {
                                    if let Some(button) = card_button(&card, card_focus).cloned() {
                                        activation = Some(MenuWork {
                                            owner: work_owner,
                                            difficulty: live.difficulty,
                                            page: work_page,
                                            activation: Activation::Card {
                                                kind: card.kind,
                                                button,
                                            },
                                        });
                                    }
                                }
                            }
                        } else if let Some(button) = confirm_button(&live.buttons,live.focus) {
                            activation = Some(MenuWork {
                                owner: work_owner,
                                difficulty: live.difficulty,
                                page: work_page,
                                activation: Activation::Button(button),
                            });
                        }
                    }
                    Action::Left | Action::Right => {
                        let card_focus = live.buttons.get(live.focus).and_then(Button::card_index);
                        if let Some(card_focus) = card_focus {
                            if let Some(card) = live.card_window.clone() {
                                let native_page = word(env, card.window + 0xa8).unwrap_or(u32::MAX);
                                if native_page == card.page {
                                    let indices = card_indices(&card);
                                    match card_navigation(
                                        action,
                                        card_focus,
                                        card.page,
                                        card.page_count,
                                        &indices,
                                    ) {
                                        CardStep::Stay => {}
                                        CardStep::Move(target) => {
                                            if set_card_focus(&mut live, target) {
                                                live.selection_dirty = true;
                                            }
                                            live.cursor = true;
                                        }
                                        CardStep::Turn {
                                            action,
                                            target_focus,
                                        } => {
                                            if let Some(id) = arrow_id(action) {
                                                if let Some(button) = live
                                                    .buttons
                                                    .iter()
                                                    .find(|button| button.id == id)
                                                    .cloned()
                                                {
                                                    live.pending = Some(PagePending {
                                                        window: card.window,
                                                        target_page: (target_focus
                                                            / CARD_PAGE_SIZE)
                                                            as u32,
                                                        target_focus,
                                                        settle_ticks: CARD_PAGE_SETTLE_TICKS,
                                                        stale_ticks: 0,
                                                    });
                                                    live.cursor = true;
                                                    activation = Some(MenuWork {
                                                        owner: work_owner,
                                                        difficulty: live.difficulty,
                                                        page: work_page,
                                                        activation: Activation::Button(button),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            let old_focus = live.focus;
                            live.focus = nearest(&live.buttons, live.focus, action);
                            live.cursor = true;
                            if old_focus != live.focus
                                && live
                                    .buttons
                                    .get(live.focus)
                                    .is_some_and(|button| {
                                        button.card_index().is_some() || button.selector()
                                    })
                            {
                                live.selection_dirty = true;
                            }
                        }
                    }
                    Action::Up | Action::Down => {
                        let old_focus = live.focus;
                        live.focus = nearest(&live.buttons, live.focus, action);
                        live.cursor = true;
                        if old_focus != live.focus
                            && live
                                .buttons
                                .get(live.focus)
                                .is_some_and(|button| {
                                    button.card_index().is_some() || button.selector()
                                })
                        {
                            live.selection_dirty = true;
                        }
                    }
                }
            }
        } else {
            live.requests.clear();
        }
        (take_selection_work(&mut live), activation)
    };
    if let Some(selection) = selection {
        apply_selection(env, &selection);
    }
    if let Some(work) = activation {
        activate(env, &work);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_navigation_uses_live_bounds_without_activating_buttons() {
        let button = |x, y| Button {
            id: "BTN_TEST".into(),
            widget: 0,
            vtable: 0,
            rect: [x, y, 20., 20.],
        };
        let list = [
            button(0., 0.),
            button(100., 0.),
            button(0., 100.),
            button(100., 100.),
        ];
        assert_eq!(nearest(&list, 0, Action::Right), 1);
        assert_eq!(nearest(&list, 0, Action::Down), 2);
        assert_eq!(nearest(&list, 3, Action::Up), 1);
        assert_eq!(nearest(&list, 0, Action::Left), 0);
    }

    #[test]
    fn bottom_row_alignment_does_not_turn_down_into_delete() {
        let button = |id: &str, rect| Button {
            id: id.into(),
            widget: 0,
            vtable: 0,
            rect,
        };
        let list = [
            button("BTN_BACK", [30., 271., 60., 49.]),
            button("BTN_DELETE", [168., 275., 145., 44.]),
            button("BTN_OK", [391., 271., 60., 49.]),
        ];
        assert_eq!(nearest(&list, 0, Action::Down), 0);
        assert_eq!(nearest(&list, 0, Action::Right), 0);
    }

    #[test]
    fn native_selection_fields_are_scoped_to_the_verified_pages() {
        assert_eq!(selector_value(9,"BTN_CHAPTER"),Some((0x350,3,None)));
        assert_eq!(selector_value(9,"BTN_TIMED"),Some((0x350,1,Some(2))));
        assert_eq!(selector_value(6,"BTN_PROFILE3"),Some((0x1a8,2,None)));
        assert_eq!(selector_value(9,"BTN_PROFILE3"),None);
        assert_eq!(selector_value(7,"BTN_NORMAL"),None); // difficulty selects differently
        assert!(fixed_action("BTN_DELETE"));
        assert!(fixed_action("BTN_PLAY_PAUSE"));
    }

    #[test]
    fn hovering_delete_never_changes_confirm_into_delete() {
        let button=|id:&str|Button{id:id.into(),widget:0,vtable:0,rect:[0.,0.,30.,30.]};
        let buttons=[button("BTN_DELETE"),button("BTN_OK"),button("BTN_PROFILE2")];
        assert_eq!(confirm_button(&buttons,0).unwrap().id,"BTN_OK");
        assert_eq!(confirm_button(&buttons,2).unwrap().id,"BTN_OK");
        assert!(confirm_button(&buttons[..1],0).is_none());
        let popup=[button("BTN_NO"),button("BTN_OK")];
        assert_eq!(confirm_button(&popup,0).unwrap().id,"BTN_OK");
    }

    #[test]
    fn repeated_local_card_rectangles_do_not_replace_global_column_slots() {
        let cards:Vec<_>=(0..2).map(|i|Button{id:format!("CARD_{i}"),widget:100+i,vtable:1,rect:[90.+i as f32*180.,58.,155.,170.]}).collect();
        let card=CardWindow{kind:CardKind::Chapter,window:1,page:0,page_count:1,cards};
        let mut local=[(100,[0.,0.,154.,142.]),(101,[0.,0.,154.,142.])];
        restore_card_geometry(&card,&mut local);
        assert_eq!(local[0].1,[90.,58.,155.,170.]);assert_eq!(local[1].1,[270.,58.,155.,170.]);
        let mut global=[(100,[85.,55.,150.,160.]),(101,[265.,55.,150.,160.])];let original=global;
        restore_card_geometry(&card,&mut global);assert_eq!(global,original);
    }

    #[test]
    fn card_pages_map_to_the_verified_native_windows() {
        assert_eq!(CardKind::for_page(10), Some(CardKind::Survival));
        assert_eq!(CardKind::window_name(10), Some("SURVIVAL_TIMED_WINDOW"));
        assert_eq!(CardKind::for_page(11), Some(CardKind::Survival));
        assert_eq!(CardKind::window_name(11), Some("SURVIVAL_ENDLESS_WINDOW"));
        assert_eq!(CardKind::for_page(12), Some(CardKind::Chapter));
        assert_eq!(CardKind::window_name(12), Some("SURVIVAL_CHAPTER_WINDOW"));
        assert_eq!(CardKind::Survival.send_action(), 0x001d_66e4);
        assert_eq!(CardKind::Chapter.send_action(), 0x001d_7200);
    }

    #[test]
    fn card_focus_prefers_native_selection_then_restores_previous_page_focus() {
        let indices = [0, 1, 2, 3, 4];
        assert_eq!(default_card_focus(Some(2), Some(3), 1, &indices), Some(2));
        assert_eq!(default_card_focus(Some(0), Some(3), 1, &indices), Some(3));
        assert_eq!(default_card_focus(None, Some(0), 1, &indices), Some(2));
        assert_eq!(default_card_focus(None, None, 2, &indices), Some(4));
    }

    #[test]
    fn card_navigation_turns_only_at_page_edges() {
        let indices = [0, 1, 2, 3, 4];
        assert_eq!(
            card_navigation(Action::Right, 0, 0, 3, &indices),
            CardStep::Move(1)
        );
        assert_eq!(
            card_navigation(Action::Right, 1, 0, 3, &indices),
            CardStep::Turn {
                action: Action::Right,
                target_focus: 2
            }
        );
        assert_eq!(
            card_navigation(Action::Left, 2, 1, 3, &indices),
            CardStep::Turn {
                action: Action::Left,
                target_focus: 1
            }
        );
        assert_eq!(
            card_navigation(Action::Left, 3, 1, 3, &indices),
            CardStep::Move(2)
        );
        assert_eq!(
            card_navigation(Action::Right, 4, 2, 3, &indices),
            CardStep::Stay
        );
        assert_eq!(
            card_navigation(Action::Left, 0, 0, 3, &indices),
            CardStep::Stay
        );
    }

    #[test]
    fn card_navigation_does_not_act_across_a_stale_page() {
        let indices = [0, 1, 2, 3];
        assert_eq!(
            card_navigation(Action::Right, 1, 1, 2, &indices),
            CardStep::Stay
        );
        assert_eq!(
            card_navigation(Action::Confirm, 1, 0, 2, &indices),
            CardStep::Stay
        );
    }

    #[test]
    fn pending_page_waits_for_stability_and_can_abandon() {
        let mut pending = PagePending {
            window: 1,
            target_page: 1,
            target_focus: 2,
            settle_ticks: 2,
            stale_ticks: 0,
        };
        assert_eq!(advance_pending(&mut pending, 0), PendingStep::Wait);
        assert_eq!(advance_pending(&mut pending, 1), PendingStep::Wait);
        assert_eq!(advance_pending(&mut pending, 1), PendingStep::Wait);
        assert_eq!(advance_pending(&mut pending, 1), PendingStep::Ready(2));

        let mut stale = PagePending {
            window: 1,
            target_page: 1,
            target_focus: 2,
            settle_ticks: 0,
            stale_ticks: CARD_PAGE_STALE_TICKS,
        };
        assert_eq!(advance_pending(&mut stale, 0), PendingStep::Abandon);
    }
}

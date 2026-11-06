//! PC-only QTE/action prompt metadata. The native gesture/action still runs
//! unchanged; the host overlay replaces its touch glyph with E/A keycaps.
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CombatAction {
    Slash,
    Stomp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromptKind {
    Tap,
    Swipe,
    Combat(CombatAction),
    Death,
    DeathCancel,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Prompt {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub kind: PromptKind,
    pub secondary: Option<[f32; 4]>,
}

fn prompt() -> &'static Mutex<Option<Prompt>> {
    static PROMPT: std::sync::OnceLock<Mutex<Option<Prompt>>> = std::sync::OnceLock::new();
    PROMPT.get_or_init(|| Mutex::new(None))
}

pub(crate) fn publish(value: Option<Prompt>) {
    if let Ok(mut slot) = prompt().lock() {
        if slot.as_ref().map(|p|p.kind)!=value.as_ref().map(|p|p.kind) {
            let code=match value.map(|p|p.kind) {None=>0,Some(PromptKind::Tap)=>1,Some(PromptKind::Swipe)=>2,Some(PromptKind::Combat(CombatAction::Slash))=>3,Some(PromptKind::Combat(CombatAction::Stomp))=>4,Some(PromptKind::Death)=>5,Some(PromptKind::DeathCancel)=>6};
            crate::ds_diagnostics::event("native_prompt_kind",[code,0,0,0]);
        }
        *slot = value;
    }
}

pub(crate) fn snapshot() -> Option<Prompt> {
    prompt().lock().ok().and_then(|slot| *slot)
}

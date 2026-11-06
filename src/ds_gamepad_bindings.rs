//! Digital gamepad bindings. Triggers and stick axes keep their analogue semantics.
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InputMode { Auto, Keyboard, Gamepad }
impl InputMode {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        match value { "auto" => Ok(Self::Auto), "keyboard" => Ok(Self::Keyboard), "gamepad" => Ok(Self::Gamepad), _ => Err("invalid input mode") }
    }
}

pub const ACTIONS: [(&str, &str); 10] = [
    ("interact", "A"), ("kinesis", "B"), ("reload", "X"), ("stasis", "Y"),
    ("melee", "LB"), ("primary", "RB"), ("cursor", "START"), ("pause", "BACK"),
    ("locator", "LS"), ("quick_turn", "RS"),
];

/// Canonical physical button for a semantic action. The runtime maps a
/// physical controller button through this table before the normal button
/// handlers run.
pub fn action_button(action: &str) -> Option<&'static str> {
    Some(match action {
        "interact" => "A",
        "kinesis" => "B",
        "reload" => "X",
        "stasis" => "Y",
        "melee" => "LB",
        "primary" => "RB",
        "cursor" => "START",
        "pause" => "BACK",
        "locator" => "LS",
        "quick_turn" => "RS",
        _ => return None,
    })
}

pub fn parse(text: &str) -> Result<HashMap<String, String>, String> {
    let mut assignments: HashMap<&str, String> = ACTIONS.iter().map(|(a, b)| (*a, b.to_string())).collect();
    // Keep canonical dispatch identities stable, change physical defaults.
    assignments.insert("locator", "RS".into());
    assignments.insert("quick_turn", "LS".into());
    let mut seen = std::collections::HashSet::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let (action, button) = line.split_once('=').ok_or("expected action=button")?;
        let action = action.trim(); let button = button.trim().to_ascii_uppercase();
        if !assignments.contains_key(action) { return Err(format!("unknown gamepad action: {action}")); }
        if !seen.insert(action) { return Err(format!("duplicate gamepad action: {action}")); }
        if !ACTIONS.iter().any(|(_, b)| *b == button) { return Err(format!("unsupported button: {button}")); }
        assignments.insert(action, button);
    }
    let mut result = HashMap::new();
    for (action, semantic) in ACTIONS {
        let physical = assignments.remove(action).unwrap();
        if result.insert(physical.clone(), semantic.to_string()).is_some() {
            return Err(format!("gamepad button conflict: {physical}"));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn swap_including_cursor_preserves_bijection() {
        let map = parse("interact=START\ncursor=A\n").unwrap();
        assert_eq!(map["START"], "A"); assert_eq!(map["A"], "START");
        assert_eq!(map.len(), 10);
        let map = parse("locator=RS\nquick_turn=LS").unwrap();
        assert_eq!(map["RS"], "LS");
        assert_eq!(map["LS"], "RS");
    }
    #[test]
    fn rejects_conflicts_duplicates_and_analogue_axes() {
        for text in ["interact=B", "cursor=A", "primary=RT", "aim=LT", "reload=X\nreload=X", "bad"] {
            assert!(parse(text).is_err(), "accepted {text}");
        }
    }
    #[test]
    fn blank_is_complete_default() {
        let map = parse("# empty").unwrap();
        assert_eq!(map["LS"], "RS"); // L3 -> canonical quick turn
        assert_eq!(map["RS"], "LS"); // R3 -> canonical locator
        for (action, button) in ACTIONS {
            if !["locator", "quick_turn"].contains(&action) { assert_eq!(map[button], button); }
        }
    }
    #[test]
    fn input_modes_are_explicit() {
        assert!(InputMode::parse("gamepad").unwrap() == InputMode::Gamepad);
        assert!(InputMode::parse("mouse").is_err());
    }
    #[test]
    fn semantic_actions_have_stable_canonical_buttons() {
        for (action, button) in ACTIONS {
            assert_eq!(action_button(action), Some(button));
        }
        assert_eq!(action_button("unknown"), None);
    }
}

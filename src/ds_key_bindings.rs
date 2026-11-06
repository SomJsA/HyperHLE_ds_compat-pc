//! Semantic PC bindings. Host keys are translated before both Down and Up.
use std::collections::HashMap;

pub const ACTIONS: [(&str, &str); 17] = [
    ("forward", "W"),
    ("left", "A"),
    ("back", "S"),
    ("right", "D"),
    ("interact", "E"),
    ("kinesis", "F"),
    ("stasis", "C"),
    ("reload", "R"),
    ("melee", "Q"),
    ("rotate", "X"),
    ("locator", "G"),
    ("quick_turn", "Z"),
    ("weapon1", "1"),
    ("weapon2", "2"),
    ("weapon3", "3"),
    ("weapon4", "4"),
    ("weapon5", "5"),
];

pub fn parse(text: &str) -> Result<HashMap<String, String>, String> {
    let mut by_action: HashMap<&str, String> =
        ACTIONS.iter().map(|(a, k)| (*a, k.to_string())).collect();
    let mut seen = std::collections::HashSet::new();
    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (action, key) = line
            .split_once('=')
            .ok_or_else(|| format!("line {}: expected action=key", line_no + 1))?;
        let action = action.trim();
        if !by_action.contains_key(action) || !seen.insert(action.to_string()) {
            return Err(format!("unknown or repeated action: {action}"));
        }
        let key = key.trim().to_ascii_uppercase();
        // Ctrl is an explicit locator choice; other modifier/function keys
        // remain reserved for capture, menus and diagnostics.
        if !(action == "locator" && key == "CTRL") && (key.len() != 1
            || !key.as_bytes()[0].is_ascii_alphanumeric()
            || ["M", "N", "P", "V"].contains(&key.as_str()))
        {
            return Err(format!("unsupported/reserved key: {key}"));
        }
        *by_action.get_mut(action).unwrap() = key;
    }
    let mut result = HashMap::new();
    for (action, semantic) in ACTIONS {
        let key = by_action.remove(action).unwrap();
        if result.insert(key.clone(), semantic.to_string()).is_some() {
            return Err(format!("key assigned to multiple actions: {key}"));
        }
    }
    Ok(result)
}

/// Explicit assignments win. The three factory locator keys are aliases
/// while locator uses that family; a custom locator binding replaces them.
pub fn resolve<'a>(map: Option<&'a HashMap<String, String>>, key: &'a str) -> Option<&'a str> {
    if let Some(value) = map.and_then(|m| m.get(key)) { return Some(value); }
    let family = map.map_or(true, |m| ["CTRL", "G", "B"].iter().any(|k| m.get(*k).is_some_and(|v| v == "G")));
    if family && ["CTRL", "G", "B"].contains(&key) { return Some("G"); }
    if map.is_some() && ACTIONS.iter().any(|(_, default)| *default == key) { None } else { Some(key) }
}

pub fn locator_keys(map: Option<&HashMap<String, String>>) -> Vec<String> {
    let aliases: Vec<_> = ["CTRL", "G", "B"].into_iter()
        .filter(|key| resolve(map, key) == Some("G"))
        .map(|key| if key == "CTRL" { "Ctrl".into() } else { key.into() }).collect();
    if !aliases.is_empty() { return aliases; }
    vec![map.and_then(|m| m.iter().find(|(_, value)| value.as_str() == "G").map(|(key, _)| key.clone())).unwrap_or_else(|| "G".into())]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locator_aliases_follow_assignments_and_custom_bindings() {
        let defaults = parse("").unwrap();
        for key in ["CTRL", "G", "B"] { assert_eq!(resolve(Some(&defaults), key), Some("G")); }
        let custom = parse("locator=T").unwrap();
        assert_eq!(resolve(Some(&custom), "T"), Some("G"));
        assert_eq!(resolve(Some(&custom), "G"), None);
        assert_eq!(resolve(Some(&custom), "B"), Some("B"));
        let reused = parse("interact=B").unwrap();
        assert_eq!(resolve(Some(&reused), "B"), Some("E"));
        assert_eq!(locator_keys(Some(&reused)), ["Ctrl", "G"]);
        assert_eq!(resolve(Some(&parse("locator=CTRL").unwrap()), "CTRL"), Some("G"));
    }
    #[test]
    fn defaults_and_swap() {
        let defaults = parse("").unwrap();
        assert_eq!(defaults["E"], "E");
        assert_eq!(defaults["G"], "G");
        assert_eq!(defaults["Z"], "Z");
        assert_eq!(defaults["C"], "C");
        let shortcuts = parse("locator=Z\nquick_turn=G").unwrap();
        assert_eq!(shortcuts["Z"], "G");
        assert_eq!(shortcuts["G"], "Z");
        let map = parse("interact=R\nreload=E").unwrap();
        assert_eq!(map["R"], "E");
        assert_eq!(map["E"], "R");
    }
    #[test]
    fn conflicts_and_reserved() {
        for text in [
            "interact=R",
            "interact=F5",
            "interact=M",
            "wat=K",
            "interact=K\ninteract=J",
            "interact=é",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }
    #[test]
    fn unused_key_replaces_original() {
        let map = parse("# custom\ninteract=k").unwrap();
        assert_eq!(map["K"], "E");
        assert!(!map.contains_key("E"));
    }
}

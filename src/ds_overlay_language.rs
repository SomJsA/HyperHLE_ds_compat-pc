//! Selected-language text for the host HD overlay.
//!
//! The guest IPA remains read-only. The launcher extracts only the selected
//! string bank (and, for the incomplete Japanese bank, the English bank) to a
//! private data file. The host overlay reads those files at startup so cards
//! use the same language as the guest instead of the historical Chinese table.

use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Default)]
pub(crate) struct Catalog {
    pub(crate) language: String,
    pub(crate) primary: HashMap<String, String>,
    pub(crate) english: HashMap<String, String>,
}

pub(crate) fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let language = std::env::var("TOUCHHLE_DS_TEXT_LANGUAGE")
            .unwrap_or_else(|_| "zh-tieba".to_owned());
        let primary = std::env::var("TOUCHHLE_DS_TEXT_BANK")
            .ok()
            .and_then(|path| read_bank(&path))
            .unwrap_or_default();
        let english = std::env::var("TOUCHHLE_DS_TEXT_FALLBACK_BANK")
            .ok()
            .and_then(|path| read_bank(&path))
            .unwrap_or_default();
        echo!(
            "[DSoverlay-language] language={} primary_entries={} english_entries={}",
            language,
            primary.len(),
            english.len()
        );
        Catalog { language, primary, english }
    })
}

fn fnv1(bytes: &[u8]) -> u32 {
    let mut value = 0x811c9dc5u32;
    for byte in bytes { value = value.wrapping_mul(0x01000193) ^ u32::from(*byte); }
    value
}

fn read_bank(path: &str) -> Option<HashMap<String, String>> {
    let path = path.trim().trim_matches('"');
    if path.is_empty() { return None; }
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 2 * 1024 * 1024 || bytes.get(..8) != Some(b"SBIN\x02\0\0\0") { return None; }
    let mut cursor = 8usize;
    let mut strings = None;
    let mut data = None;
    let mut ohdr = None;
    while cursor < bytes.len() {
        let header_end = cursor.checked_add(12)?;
        if header_end > bytes.len() { return None; }
        let tag: [u8; 4] = bytes[cursor..cursor + 4].try_into().ok()?;
        let length = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().ok()?) as usize;
        let end = header_end.checked_add(length)?;
        let aligned = (end + 3) & !3;
        if aligned > bytes.len()
            || fnv1(&bytes[header_end..end]) != u32::from_le_bytes(bytes[cursor + 8..cursor + 12].try_into().ok()?)
        { return None; }
        match &tag {
            b"STRS" => strings = Some(bytes[header_end..end].to_vec()),
            b"DATA" => data = Some(bytes[header_end..end].to_vec()),
            b"OHDR" => ohdr = Some(bytes[header_end..end].to_vec()),
            _ => {}
        }
        cursor = aligned;
    }
    let strings = unpack_strings(&strings?)?;
    let data = data?;
    let ohdr = ohdr?;
    if ohdr.len() != 12 { return None; }
    let list = u32::from_le_bytes(ohdr[8..12].try_into().ok()?) as usize >> 3;
    if list.checked_add(8)? > data.len() { return None; }
    let kind = u32::from_le_bytes(data[list..list + 4].try_into().ok()?);
    let count = u32::from_le_bytes(data[list + 4..list + 8].try_into().ok()?) as usize;
    if kind != 16 || list.checked_add(8 + count.checked_mul(8)?)? != data.len() { return None; }
    let mut result = HashMap::with_capacity(count);
    for index in 0..count {
        let at = list + 8 + index * 8;
        let name = u16::from_le_bytes(data[at..at + 2].try_into().ok()?) as usize;
        let value = u16::from_le_bytes(data[at + 2..at + 4].try_into().ok()?) as usize;
        if name == 0 || name > strings.len() || value > strings.len() { return None; }
        // A few original banks encode an intentionally empty value as ID 0.
        // Keep the key and let the overlay's normal placeholder/fallback rule
        // select English for that entry.
        let text = if value == 0 { String::new() } else { strings[value - 1].clone() };
        result.insert(strings[name - 1].clone(), text);
    }
    Some(result)
}

fn unpack_strings(payload: &[u8]) -> Option<Vec<String>> {
    if payload.len() < 4 { return None; }
    let count = u32::from_le_bytes(payload[..4].try_into().ok()?) as usize;
    let mut cursor = 4usize;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let end = cursor.checked_add(4)?;
        if end > payload.len() { return None; }
        let length = u32::from_le_bytes(payload[cursor..end].try_into().ok()?) as usize;
        cursor = end;
        let next = cursor.checked_add(length)?;
        if next > payload.len() { return None; }
        result.push(String::from_utf8(payload[cursor..next].to_vec()).ok()?);
        cursor = next;
    }
    (cursor == payload.len()).then_some(result)
}

pub(crate) fn is_placeholder(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed.eq_ignore_ascii_case("YES") || trimmed.eq_ignore_ascii_case("NO") || trimmed.contains("XXXXXX")
}

pub(crate) fn label(language: &str) -> &'static str {
    let language = language.to_ascii_lowercase();
    if language.starts_with("zh") { "目标：" }
    else if language.starts_with("ja") { "目標：" }
    else if language.starts_with("ko") { "목표:" }
    else if language.starts_with("fr") { "Objectif :" }
    else if language.starts_with("de") { "Ziel:" }
    else if language.starts_with("it") { "Obiettivo:" }
    else if language.starts_with("es") { "Objetivo:" }
    else { "Objective:" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_cover_supported_overlay_languages() {
        assert_eq!(label("zh-tieba"), "目标：");
        assert_eq!(label("ja-original"), "目標：");
        assert_eq!(label("ko-original"), "목표:");
        assert_eq!(label("de-original"), "Ziel:");
        assert!(is_placeholder("YES"));
        assert!(is_placeholder("XXXXXXUI_HELPXXXXXX"));
        assert!(!is_placeholder("Transmission entrante"));
    }
}

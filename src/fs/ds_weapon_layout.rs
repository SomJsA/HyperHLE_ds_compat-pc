//! Length-preserving PC HUD geometry patch. The caller verifies the complete
//! original resource SHA-256 before calling this; no game data is written.

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}
fn fnv1(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5u32, |hash, byte| hash.wrapping_mul(0x01000193) ^ u32::from(*byte))
}

// Native Entity offsets and vertex-list object IDs, in the requested order:
// Plasma, Ripper, Linegun, Corer, Burst. Story and Survival share the design.
const ENTITIES: [[(usize, u32); 5]; 2] = [
    [(86932, 1448), (86972, 1449), (86892, 1447), (87012, 1450), (87052, 1451)],
    [(135080, 2319), (135120, 2320), (135440, 2328), (135160, 2321), (135680, 2334)],
];

struct LayoutSections {
    data: std::ops::Range<usize>,
    data_checksum: usize,
    objects: std::ops::Range<usize>,
}

fn layout_sections(original: &[u8]) -> Option<LayoutSections> {
    if original.get(..8)? != b"SBIN\x02\0\0\0" { return None; }
    let mut cursor = 8usize;
    let (mut data, mut objects) = (None, None);
    while cursor < original.len() {
        let size = u32_at(original, cursor + 4)? as usize;
        let end = cursor.checked_add(12)?.checked_add(size)?;
        let payload = original.get(cursor + 12..end)?;
        if fnv1(payload) != u32_at(original, cursor + 8)? { return None; }
        match original.get(cursor..cursor + 4)? {
            b"DATA" => { if data.is_some() {return None;} data = Some((cursor + 12, end, cursor + 8)); },
            b"OHDR" => { if objects.is_some() {return None;} objects = Some((cursor + 12, end)); },
            _ => (),
        }
        cursor = end.checked_add(3)? & !3;
    }
    if cursor != original.len() { return None; }
    let (begin, end, checksum) = data?;
    let (table, table_end) = objects?;
    Some(LayoutSections { data: begin..end, data_checksum: checksum, objects: table..table_end })
}

fn apply_geometry(original: &[u8], sections: &LayoutSections, edits: Vec<(usize, f32)>) -> Vec<u8> {
    let mut output = original.to_vec();
    for (at, value) in edits {
        output[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let hash = fnv1(&output[sections.data.clone()]);
    let checksum = sections.data_checksum;
    output[checksum..checksum + 4].copy_from_slice(&hash.to_le_bytes());
    output
}

pub(super) fn patch(original: &[u8]) -> Option<Vec<u8>> {
    let sections = layout_sections(original)?;
    let (begin, end) = (sections.data.start, sections.data.end);
    let (table, table_end) = (sections.objects.start, sections.objects.end);
    let mut edits = Vec::new();
    for layout in ENTITIES {
        for (slot, (entity, reference)) in layout.into_iter().enumerate() {
            if u32_at(original.get(begin..end)?, entity + 36)? != reference { return None; }
            let entry_at = table.checked_add(reference as usize * 4)?;
            if entry_at + 4 > table_end { return None; }
            let entry = u32_at(original, entry_at)?;
            if entry & 7 != 2 { return None; }
            let vertices = (entry >> 3) as usize;
            let body = original.get(begin..end)?;
            if u32_at(body, vertices)? != 0x0005_0010 || u32_at(body, vertices + 4)? != 2 { return None; }
            // Keep the native sublayout/icon/action and anchors. Change only
            // its parent rectangle, so rendering and hit testing use one layout.
            let top = 10.0 + slot as f32 * 60.0;
            for (vertex, x, y) in [(0, -80.0f32, top), (1, -6.0f32, top + 60.0)] {
                let at = begin + vertices + 8 + vertex * 16;
                if at + 16 > end { return None; }
                edits.push((at, x)); edits.push((at + 4, y));
            }
        }
    }
    // The original top-right RIG toggle occupies the new first weapon slot.
    // Move its entire native rectangle inward; keep size, y, anchors and action.
    for (entity, reference) in [(87332, 1458u32), (135400, 2327u32)] {
        if u32_at(original.get(begin..end)?, entity + 36)? != reference {return None;}
        let entry_at = table + reference as usize * 4;
        if entry_at + 4 > table_end {return None;}
        let entry = u32_at(original, entry_at)?;
        if entry & 7 != 2 {return None;}
        let vertices = (entry >> 3) as usize;
        let body = original.get(begin..end)?;
        if u32_at(body, vertices)? != 0x0005_0010 || u32_at(body, vertices + 4)? != 2 {return None;}
        for (vertex, expected, x) in [(0, -78.0f32, -178.0f32), (1, 2.0f32, -98.0f32)] {
            let at = begin + vertices + 8 + vertex * 16;
            if at + 16 > end || f32::from_bits(u32_at(original, at)?) != expected {return None;}
            edits.push((at, x));
        }
    }
    Some(apply_geometry(original, &sections, edits))
}

// World 1.0.1's HUD_TUTORIAL_SLIDE_TEXT_BG is used by the native tutorial
// layouts. Extend the four lower vertices to enclose three lines of text.
pub(super) fn patch_world_tutorial(original: &[u8]) -> Option<Vec<u8>> {
    const BORDER: [(usize, u32, u32, f32); 4] = [
        (86472, 1077, 1456, 62.0), // BG_LINES_LEFT
        (86512, 1079, 1457, 62.0), // BG_LINES_RIGHT
        (86552, 287, 1458, 63.0),  // ARM_LEFT
        (86592, 289, 1459, 63.0),  // ARM_RIGHT
    ];
    let sections = layout_sections(original)?;
    let data = original.get(sections.data.clone())?;
    let objects = original.get(sections.objects.clone())?;
    let mut edits = Vec::with_capacity(BORDER.len());
    for (entity, name, reference, bottom) in BORDER {
        // The first two u16 fields identify the named STRETCH entity.
        if u32_at(data, entity)? != ((64 << 16) | name)
            || u32_at(data, entity + 36)? != reference
        {
            return None;
        }
        let entry = u32_at(objects, reference as usize * 4)?;
        if entry & 7 != 2 {
            return None;
        }
        let vertices = (entry >> 3) as usize;
        if u32_at(data, vertices)? != 0x0005_0010 || u32_at(data, vertices + 4)? != 2 {
            return None;
        }
        let y = vertices + 8 + 16 + 4;
        if f32::from_bits(u32_at(data, y)?) != bottom {
            return None;
        }
        edits.push((sections.data.start + y, bottom + 14.0));
    }
    Some(apply_geometry(original, &sections, edits))
}

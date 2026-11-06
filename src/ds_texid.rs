/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */
//! Stable identity for uploaded textures (Dead Space compatibility aid).
//!
//! Dead Space recycles GL texture *names*: a single name can hold completely
//! different artwork at different points of one run. Measured on a real dump
//! (2026-10-08): `tex 2` occurred four times with four unrelated pictures
//! (UI icon, lens flare, logo, corridor wall), `tex 5` twice, `tex 13` twice.
//!
//! That makes every "target this one texture" debug switch unreliable when it
//! matches on the GL name (`TOUCHHLE_DS_REDTEX`, `_SKIP_TEX`,
//! `_PROJ_Z_SCALE_TEX`). This module supplies a *content* identity instead:
//!
//! * [`content_key`] is a deterministic 32-bit fingerprint of one upload's
//!   shape (width, height, level, internal format, byte length) and bytes, so
//!   the same artwork yields the same key on every run and in every dump.
//! * [`record`] / [`lookup`] track, per live GL name, the key of the content
//!   that was uploaded into it *most recently*. That is exactly the content a
//!   draw using that name will sample, so a draw-time lookup is unambiguous
//!   even though the name itself gets reused later.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// What a live GL texture name currently holds.
#[derive(Clone)]
pub struct TexIdent {
    /// Content fingerprint of the current upload (0 = unhashed/unknown).
    pub key: u32,
    /// Dump stem for the current upload, if one was assigned.
    pub stem: String,
}

static LIVE: OnceLock<Mutex<HashMap<u32, TexIdent>>> = OnceLock::new();

fn live() -> &'static Mutex<HashMap<u32, TexIdent>> {
    LIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Note which content has just been uploaded into `name`.
pub fn record(name: u32, ident: TexIdent) {
    if let Ok(mut map) = live().lock() {
        map.insert(name, ident);
    }
}

/// What does `name` currently hold? `None` if we never saw an upload to it.
pub fn lookup(name: u32) -> Option<TexIdent> {
    live().lock().ok().and_then(|map| map.get(&name).cloned())
}

fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    let mut hash = seed;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Document the format tag convention so both call sites agree:
/// * compressed uploads pass the compressed `internalformat`
/// * uncompressed uploads pass the client `format`
///
/// Deterministic 32-bit identity of one texture upload.
///
/// The geometry and format are part of the hash on purpose: the same bytes
/// interpreted as a different format are a different picture. Texture uploads
/// are at most a few megabytes per run, so hashing every one of them is cheap
/// compared to the PVRTC decode that already happens.
pub fn content_key(
    width: u32,
    height: u32,
    level: i32,
    format_tag: u32,
    data: &[u8],
) -> u32 {
    const SEED: u64 = 0xcbf2_9ce4_8422_2325;
    let mut hash = fnv1a(SEED, &width.to_le_bytes());
    hash = fnv1a(hash, &height.to_le_bytes());
    hash = fnv1a(hash, &level.to_le_bytes());
    hash = fnv1a(hash, &format_tag.to_le_bytes());
    hash = fnv1a(hash, &(data.len() as u64).to_le_bytes());
    hash = fnv1a(hash, data);
    // Fold to 32 bits: short enough to paste on a command line, and still far
    // more than enough to separate the few hundred textures of one game.
    (hash ^ (hash >> 32)) as u32
}

/// Parse `"3f2a1b1c,0x1234"` into keys. Values are **hexadecimal** (the form
/// the dump manifest prints); a leading `0x` is accepted and ignored.
pub fn parse_keys(spec: &str) -> Vec<u32> {
    spec.split(',')
        .filter_map(|part| {
            let text = part.trim();
            let text = text
                .strip_prefix("0x")
                .or_else(|| text.strip_prefix("0X"))
                .unwrap_or(text);
            u32::from_str_radix(text, 16).ok()
        })
        .collect()
}

/// The room overlay atlas is also used by opaque props and additive light
/// shafts. Their 36-index batches must keep the guest's depth test and write
/// mask; the historical cover/wall override otherwise draws them through the
/// player. Keep this classification shared by desktop GL and native GLES.
pub fn overlay_effect_keeps_depth(
    key: u32,
    count: i32,
    blend_enabled: bool,
    blend_src: u32,
    blend_dst: u32,
) -> bool {
    matches!(key, 0x56c6_2e8a | 0x7027_023c)
        && count == 36
        && (!blend_enabled || (blend_src == 0x0302 && blend_dst == 1))
}

#[cfg(test)]
mod overlay_effect_depth_tests {
    use super::overlay_effect_keeps_depth;

    #[test]
    fn captured_chapter_three_shafts_keep_depth_for_both_resource_variants() {
        for key in [0x56c6_2e8a, 0x7027_023c] {
            assert!(overlay_effect_keeps_depth(key, 36, true, 0x0302, 1));
        }
    }

    #[test]
    fn cover_and_cutout_passes_keep_their_existing_override_policy() {
        assert!(!overlay_effect_keeps_depth(0x56c6_2e8a, 6, true, 0x0306, 0));
        assert!(!overlay_effect_keeps_depth(
            0x56c6_2e8a,
            6,
            true,
            0x0302,
            0x0303
        ));
        assert!(!overlay_effect_keeps_depth(
            0x56c6_2e8a,
            36,
            true,
            0x0302,
            0x0303
        ));
        assert!(!overlay_effect_keeps_depth(
            0x15f5_4e17,
            36,
            true,
            0x0302,
            1
        ));
    }

    #[test]
    fn captured_chapter_one_opaque_props_keep_depth_and_ignore_stale_blend_factors() {
        for key in [0x56c6_2e8a, 0x7027_023c] {
            for (src, dst) in [(0x0302, 0x0303), (0x0302, 1), (1, 0)] {
                assert!(overlay_effect_keeps_depth(key, 36, false, src, dst));
            }
            assert!(!overlay_effect_keeps_depth(
                key,
                6,
                false,
                0x0302,
                0x0303
            ));
            assert!(!overlay_effect_keeps_depth(key, 72, false, 1, 0));
        }
        assert!(!overlay_effect_keeps_depth(0xd98f_a9a2, 36, false, 1, 0));
    }
}

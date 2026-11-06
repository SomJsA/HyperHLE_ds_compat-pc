/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Text layout and font rasterization abstraction.
//!
//! This is implemented using the [rusttype] library. All usage of that library
//! should be confined to this module.
//!
//! TODO: Less terrible text layout. RustType doesn't do text layout so this
//! code has its own, not particularly good implementation. We might want to
//! switch to something like cosmic-text in future, but that has a _lot_ more
//! dependencies.

use crate::frameworks::core_graphics::cg_affine_transform::CGAffineTransform;
use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::paths;
use rusttype::{vector, GlyphId, Point, Scale};
use std::io::Read;

mod arabic;

pub struct Font {
    /// `None` only for the phantom-default `Font` returned via
    /// [`Default`] when the objc runtime needs a placeholder for a
    /// missing/wrong-typed host object. Real `Font`s constructed via
    /// the `mono_regular()` / `from_file()` / etc. helpers always set
    /// this to `Some(...)`.
    font: Option<rusttype::Font<'static>>,
    /// Raw font file bytes — needed for TrueType table access (CGFontCopyTableForTag).
    raw_data: Option<Vec<u8>>,
    /// Glyph-id translation used only when this `Font` *stands in* for a guest
    /// font: the guest computes glyph ids with its own font's cmap (Dead Space
    /// uses its bundled EurostileLTStd.ttf for the menu digits), so drawing
    /// those ids in a substitute face picks the wrong shapes - measured
    /// 2026-10-10: `档案 1` became `档案 .(` and `05:04:33` became `-27-1700`.
    /// The map restores the intent: guest glyph id -> character -> this font's
    /// glyph id.
    gid_remap: Option<std::collections::HashMap<u16, char>>,
}

impl Default for Font {
    fn default() -> Self {
        Font {
            font: None,
            raw_data: None,
            gid_remap: None,
        }
    }
}

/// One-line breadcrumb for every text-drawing entry point, so a single run can
/// answer "which API does this string come through?". Needed because Dead
/// Space's in-world interaction prompt (the Chinese text inside the cyan ring at
/// a door) is missing in our build while the ring itself draws, and the obvious
/// candidate - CoreText - turned out never to be called at all
/// (measured 2026-10-10: `[ctdraw]` fired 0 times in a whole chapter-12 run).
pub(crate) fn note_text_path(tag: &str, len: usize) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    if N.fetch_add(1, Ordering::Relaxed) < 60 {
        log!("[textpath] {} len={}", tag, len);
    }
}

    impl Font {
    /// Panic with a clear message if someone tries to use a phantom/default
    /// `Font` (one without an actual `rusttype::Font` loaded) — this
    /// indicates a programming bug elsewhere, since the only path that
    /// produces such a `Font` is the `objc::ObjC::borrow` fallback used
    /// for missing/wrong-typed host objects.
    fn rt(&self) -> &rusttype::Font<'static> {
        self.font.as_ref().expect(
            "tried to use methods on an uninitialised Font \
             (likely a phantom-fallback host object)",
        )
    }
}

pub enum TextAlignment {
    Left,
    Center,
    Right,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum WrapMode {
    Word,
    Char,
}

fn scale(font_size: f32) -> Scale {
    // iPhone OS's interpretation of font size is slightly different, reason
    // unknown. This is not the same as the Windows pt vs Mac pt issue.
    // This scale factor has been eyeball'd, it's not exact.
    Scale::uniform(font_size * 1.125)
}

/// Helper for [Font::draw], used for the `draw_glyph` callback.
pub struct RasterGlyph<'a> {
    origin: (f32, f32),
    dimensions: (i32, i32),
    pixels: &'a [f32],
}
impl RasterGlyph<'_> {
    /// Get the x and y co-ordinates the glyph should be drawn at.
    pub fn origin(&self) -> (f32, f32) {
        self.origin
    }
    /// Get the dimensions, in pixels, of the glyph.
    pub fn dimensions(&self) -> (i32, i32) {
        self.dimensions
    }
    /// Get the coverage at the given co-ordinates within the glyph.
    pub fn pixel_at(&self, coords: (i32, i32)) -> f32 {
        let (width, height) = self.dimensions;
        if (0..width).contains(&coords.0) && (0..height).contains(&coords.1) {
            self.pixels[coords.1 as usize * width as usize + coords.0 as usize]
        } else {
            0.0 // safety in case of rounding errors
        }
    }
}

/// Does `TOUCHHLE_DS_FONT_OVERRIDE` replace this bundled font?
///
/// `TOUCHHLE_DS_FONT_OVERRIDE_FOR` selects the scope:
/// * `all` (default) - every face, including the Latin/system ones
/// * `cjk`           - only the CJK families (NotoSansSC/JP), which is the
///                     narrow setting when only Chinese text should change
fn override_font_applies(filename: &str) -> bool {
    // Android has no convenient way for the player to launch the app with an
    // environment variable, so the arm64 test build carries the requested CJK
    // face as an APK asset and enables it by default. Desktop keeps the
    // explicit environment-variable opt-in.
    if std::env::var_os("TOUCHHLE_DS_FONT_OVERRIDE").is_none()
        && !cfg!(target_os = "android")
    {
        return false;
    }
    match std::env::var("TOUCHHLE_DS_FONT_OVERRIDE_FOR")
        .unwrap_or_else(|_| "all".to_string())
        .as_str()
    {
        "cjk" => filename.contains("NotoSansSC") || filename.contains("NotoSansJP"),
        _ => true,
    }
}

/// Bytes of the override font, or `None` to use the bundled one.
///
/// Read from an absolute host path (and logged once per font so a typo in the
/// path is visible in the run log instead of silently doing nothing).
fn override_font_bytes(filename: &str) -> Option<Vec<u8>> {
    if !override_font_applies(filename) {
        return None;
    }
    if let Ok(path) = std::env::var("TOUCHHLE_DS_FONT_OVERRIDE") {
        let path = path.trim().trim_matches('"').to_string();
        if !path.is_empty() {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    log!("[DSfont] {filename} replaced by TOUCHHLE_DS_FONT_OVERRIDE={path}");
                    return Some(bytes);
                }
                Err(e) => {
                    log!(
                        "[DSfont] TOUCHHLE_DS_FONT_OVERRIDE={path} unreadable ({e}); \
                         trying Android bundled override"
                    );
                }
            }
        }
    }

    #[cfg(target_os = "android")]
    {
        let path = "touchHLE_ds_fonts/SweiShearSansCJKsc-Medium.ttf";
        let mut file = match paths::ResourceFile::open(path) {
            Ok(file) => file,
            Err(e) => {
                log!("[DSfont] Android bundled override {path} unreadable ({e}); using {filename}");
                return None;
            }
        };
        let mut bytes = Vec::new();
        match file.get().read_to_end(&mut bytes) {
            Ok(_) => {
                log!("[DSfont] {filename} replaced by Android bundled override {path}");
                Some(bytes)
            }
            Err(e) => {
                log!("[DSfont] Android bundled override {path} unreadable ({e}); using {filename}");
                None
            }
        }
    }

    #[cfg(not(target_os = "android"))]
    None
}

/// Replacement bytes for a font the *guest* supplied (CGFontCreateWithData-
/// Provider), or `None` to use the guest's own font data.
///
/// This is the path that carries the game's own typeface (Dead Space ships
/// `EurostileLTStd.ttf` / `RussellSquare.ttf` in the bundle), so without this
/// hook `TOUCHHLE_DS_FONT_OVERRIDE` only swaps the *emulator's* fonts.
///
/// BUT: it is **off by default and must stay that way** unless the glyph ids
/// are translated as well. Measured 2026-10-10 (user screenshot): with this hook
/// enabled the 选择档案 screen rendered `档案 .(` and `-27-1700` instead of
/// `档案 1` and `05:04:33` - the game looks glyphs up with *its own* font's cmap
/// and we then drew those indices with a different font, so index 0x31 meant a
/// different shape. Enable only with `TOUCHHLE_DS_FONT_OVERRIDE_GUEST=1`, and
/// only when the glyph-index translation below exists.
pub fn guest_font_override_bytes() -> Option<Vec<u8>> {
    if std::env::var_os("TOUCHHLE_DS_FONT_OVERRIDE_GUEST").is_none() {
        return None;
    }
    let path = std::env::var("TOUCHHLE_DS_FONT_OVERRIDE").ok()?;
    let path = path.trim().trim_matches('"').to_string();
    if path.is_empty() {
        return None;
    }
    match std::fs::read(&path) {
        Ok(bytes) => {
            log!("[DSfont] guest-supplied font data replaced by TOUCHHLE_DS_FONT_OVERRIDE={path}");
            Some(bytes)
        }
        Err(e) => {
            log!("[DSfont] TOUCHHLE_DS_FONT_OVERRIDE={path} unreadable ({e})");
            None
        }
    }
}

impl Font {
    pub fn glyph_id_for_char(&self, c: u16) -> GlyphId {
        self.rt().glyph(char::from_u32(c as u32).unwrap()).id()
    }

    /// Returns the total number of glyphs in the font.
    pub fn glyph_count(&self) -> u32 {
        self.rt().glyph_count() as u32
    }

    /// Returns the units-per-em value from the font. rusttype normalizes to
    /// 1.0 scale = 1 em, so we read it from the raw 'head' table if available,
    /// otherwise default to 2048.
    pub fn units_per_em(&self) -> u16 {
        if let Some(data) = self.table_data(0x68656164 /* 'head' */) {
            if data.len() >= 20 {
                // unitsPerEm is at offset 18 in the 'head' table (big-endian u16)
                let upm = u16::from_be_bytes([data[18], data[19]]);
                if upm > 0 {
                    return upm;
                }
            }
        }
        2048
    }

    /// Returns the advance width (in design units) for a glyph.
    pub fn glyph_advance(&self, glyph_id: GlyphId) -> i32 {
        // Read the advance straight out of the raw `hmtx` table. rusttype's
        // `h_metrics()` disagrees with `hmtx` for some fonts (observed on the
        // app's RussellSquare: every advance came out ~1.2077x too large),
        // which scrambles UI text layout, so `hmtx` is authoritative here.
        if let Some(advance) = self.hmtx_metrics(glyph_id.0 as u32) {
            return advance.0 as i32;
        }
        // Fallback: rusttype's scaled metrics.
        let upm = self.rt().units_per_em() as f32;
        let g = self.rt().glyph(glyph_id).scaled(Scale::uniform(upm));
        g.h_metrics().advance_width.round() as i32
    }

    /// (advance_width, left_side_bearing) from the raw `hmtx`/`hhea` tables,
    /// in font design units. Glyphs past `numberOfHMetrics` reuse the last
    /// advance, per the OpenType specification.
    fn hmtx_metrics(&self, glyph_id: u32) -> Option<(u16, i16)> {
        let hhea = self.table_data(0x68686561 /* 'hhea' */)?;
        if hhea.len() < 36 {
            return None;
        }
        let number_of_h_metrics = u16::from_be_bytes([hhea[34], hhea[35]]) as u32;
        if number_of_h_metrics == 0 {
            return None;
        }
        let hmtx = self.table_data(0x686D7478 /* 'hmtx' */)?;
        let index = glyph_id.min(number_of_h_metrics - 1);
        let offset = (index * 4) as usize;
        if hmtx.len() < offset + 4 {
            return None;
        }
        let advance = u16::from_be_bytes([hmtx[offset], hmtx[offset + 1]]);
        let lsb = i16::from_be_bytes([hmtx[offset + 2], hmtx[offset + 3]]);
        Some((advance, lsb))
    }

    /// Returns the left side bearing (in design units) for a glyph.
    pub fn glyph_left_side_bearing(&self, glyph_id: GlyphId) -> i32 {
        let upm = self.rt().units_per_em() as f32;
        let g = self.rt().glyph(glyph_id).scaled(Scale::uniform(upm));
        g.h_metrics().left_side_bearing.round() as i32
    }

    /// Exact glyph bounding box read straight out of the `glyf` table, in the
    /// `head` table design units. `CGFontGetGlyphBBoxes` is documented to
    /// return design units, and the app uses it to place each glyph quad, so
    /// rusttype rasterised pixel bbox (rounded, and scaled by rusttype own
    /// units-per-em) is not accurate enough - the same class of mismatch that
    /// forced `glyph_advance` to read `hmtx` directly.
    fn glyf_bbox(&self, glyph_id: u32) -> Option<(f32, f32, f32, f32)> {
        let head = self.table_data(0x68656164 /* head */)?;
        let maxp = self.table_data(0x6D617870 /* maxp */)?;
        let loca = self.table_data(0x6C6F6361 /* loca */)?;
        let glyf = self.table_data(0x676C7966 /* glyf */)?;
        if head.len() < 52 || maxp.len() < 6 {
            return None;
        }
        let index_to_loc_format = i16::from_be_bytes([head[50], head[51]]);
        let num_glyphs = u16::from_be_bytes([maxp[4], maxp[5]]) as u32;
        if glyph_id >= num_glyphs {
            return None;
        }
        let gid = glyph_id as usize;
        let (start, end) = if index_to_loc_format == 0 {
            let o = gid * 2;
            if loca.len() < o + 4 {
                return None;
            }
            (
                u16::from_be_bytes([loca[o], loca[o + 1]]) as usize * 2,
                u16::from_be_bytes([loca[o + 2], loca[o + 3]]) as usize * 2,
            )
        } else {
            let o = gid * 4;
            if loca.len() < o + 8 {
                return None;
            }
            (
                u32::from_be_bytes([loca[o], loca[o + 1], loca[o + 2], loca[o + 3]]) as usize,
                u32::from_be_bytes([loca[o + 4], loca[o + 5], loca[o + 6], loca[o + 7]]) as usize,
            )
        };
        if end <= start || glyf.len() < end || end - start < 10 {
            return None;
        }
        let n_contours = i16::from_be_bytes([glyf[start], glyf[start + 1]]);
        if n_contours < 0 {
            return None;
        }
        let x_min = i16::from_be_bytes([glyf[start + 2], glyf[start + 3]]) as f32;
        let y_min = i16::from_be_bytes([glyf[start + 4], glyf[start + 5]]) as f32;
        let x_max = i16::from_be_bytes([glyf[start + 6], glyf[start + 7]]) as f32;
        let y_max = i16::from_be_bytes([glyf[start + 8], glyf[start + 9]]) as f32;
        Some((x_min, y_min, x_max - x_min, y_max - y_min))
    }

    /// Returns the bounding box of a glyph in design units.
    /// Returns (x_min, y_min, width, height). If the glyph has no outline,
    /// returns a zero rect.
    pub fn glyph_bbox(&self, glyph_id: GlyphId) -> (f32, f32, f32, f32) {
        if let Some(bb) = self.glyf_bbox(glyph_id.0 as u32) {
            return bb;
        }
        let upm = self.rt().units_per_em() as f32;
        let g = self.rt().glyph(glyph_id)
            .scaled(Scale::uniform(upm))
            .positioned(Point { x: 0.0, y: 0.0 });
        match g.pixel_bounding_box() {
            Some(bb) => {
                let x = bb.min.x as f32;
                let y = bb.min.y as f32;
                let w = (bb.max.x - bb.min.x) as f32;
                let h = (bb.max.y - bb.min.y) as f32;
                (x, y, w, h)
            }
            None => (0.0, 0.0, 0.0, 0.0),
        }
    }

    /// Returns raw bytes of the font data (for table access).
    pub fn raw_data(&self) -> Option<&[u8]> {
        self.raw_data.as_deref()
    }

    /// Parse and return the raw data for a specific TrueType/OpenType table
    /// identified by its 4-byte tag (e.g. b"head" → 0x68656164).
    pub fn table_data(&self, tag: u32) -> Option<Vec<u8>> {
        let data = self.raw_data.as_ref()?;
        parse_table_from_raw(data, tag)
    }

    /// Returns a list of all table tags present in the font file.
    pub fn table_tags(&self) -> Vec<u32> {
        let Some(data) = self.raw_data.as_ref() else {
            return Vec::new();
        };
        parse_table_tags_from_raw(data)
    }

    /// Look up a glyph by its PostScript name. Returns `None` if the 'post'
    /// table is unavailable or the name is not found.
    pub fn glyph_for_name(&self, name: &str) -> Option<GlyphId> {
        // First try the 'post' table's glyph names (post formats 1/2/4/5).
        if let Some(post_data) = self.table_data(0x706F7374 /* 'post' */) {
            if let Some(gid) = glyph_id_from_post_table(&post_data, name, self.glyph_count()) {
                return Some(gid);
            }
        }
        // Many fonts ship a `post` table in format 3, which carries no glyph
        // names at all — yet apps still ask for glyphs by name. Fall back to
        // mapping the standard Adobe glyph name to a character and looking
        // that up in the cmap; otherwise every such request resolves to
        // .notdef and the UI shows empty boxes instead of letters/digits.
        standard_glyph_name_to_char(name).map(|c| self.glyph_id_for_char(c as u32 as u16))
    }

    /// Returns the PostScript name for a glyph index, or None.
    pub fn glyph_name(&self, glyph_id: GlyphId) -> Option<String> {
        let post_data = self.table_data(0x706F7374 /* 'post' */)?;
        glyph_name_from_post_table(&post_data, glyph_id.0 as u32, self.glyph_count())
    }

    fn from_resource_file(filename: &str) -> Font {
        // Dead Space compatibility: `TOUCHHLE_DS_FONT_OVERRIDE=<path.ttf>`
        // substitutes one user-supplied face for the emulator's own fonts, so a
        // different typeface can be judged in-game without touching the game's
        // assets at all. `TOUCHHLE_DS_FONT_OVERRIDE_FOR=cjk` (or `all`) decides
        // whether only the CJK families are swapped; the default is `all`,
        // because a CJK face is normally wanted for every script at once.
        if let Some(bytes) = override_font_bytes(filename) {
            let raw_copy = bytes.clone();
            let Some(font) = rusttype::Font::try_from_vec(bytes) else {
                panic!("Couldn't parse the font named by TOUCHHLE_DS_FONT_OVERRIDE");
            };
            return Font {
                font: Some(font),
                raw_data: Some(raw_copy),
                ..Font::default()
            };
        }
        let mut bytes = Vec::new();
        let path = format!("{}/{}", paths::FONTS_DIR, filename);
        if let Err(e) = paths::ResourceFile::open(&path)
            .and_then(|mut f| f.get().read_to_end(&mut bytes).map_err(|e| e.to_string()))
        {
            panic!(
                "Couldn't read bundled font file {path:?}: {e}. Perhaps the directory is missing?"
            );
        }

        let raw_copy = bytes.clone();
        let Some(font) = rusttype::Font::try_from_vec(bytes) else {
            panic!("Couldn't parse bundled font file {path:?}. This probably means the file is corrupt. Try re-downloading it.");
        };

        Font {
            font: Some(font),
            raw_data: Some(raw_copy),
            ..Font::default()
        }
    }

    pub fn from_vec(bytes: Vec<u8>) -> Option<Font> {
        let raw_copy = bytes.clone();
        let Some(font) = rusttype::Font::try_from_vec(bytes) else {
            log!(
                "Warning: Font::from_vec: couldn't parse {} bytes of font data; \
                 returning None (the data is likely not a supported font format).",
                raw_copy.len()
            );
            return None;
        };

        Some(Font {
            font: Some(font),
            raw_data: Some(raw_copy),
            ..Font::default()
        })
    }

    /// Build the stand-in font used when `TOUCHHLE_DS_FONT_OVERRIDE_GUEST=1`
    /// replaces a font the *guest* supplied, keeping a translation table from
    /// the guest font's glyph ids to characters.
    ///
    /// Why not simply use the replacement bytes: the guest resolves characters
    /// to glyph ids with **its own** font's cmap (Dead Space draws its menu
    /// digits from the bundled EurostileLTStd.ttf). Drawing those ids in a
    /// substitute face picks unrelated shapes - measured 2026-10-10 with the
    /// user's face: `档案 1` rendered as `档案 .(` and `05:04:33` as
    /// `-27-1700`. Walking the guest cmap once gives id -> character, which is
    /// then resolved in the replacement face, so the *shape* follows the user's
    /// font while the *text* stays correct.
    pub fn stand_in_for_guest(guest_bytes: &[u8], override_bytes: Vec<u8>) -> Option<Font> {
        let guest = rusttype::Font::try_from_vec(guest_bytes.to_vec())?;
        // Preferred: parse the guest font's cmap directly (complete, and not
        // subject to rusttype's reader dropping entries - see the note on
        // `cmap_gid_to_char`).
        let mut gid_remap = cmap_gid_to_char(guest_bytes).unwrap_or_default();
        if gid_remap.is_empty() {
            // Fallback: walk a few Unicode ranges through rusttype.
            let ranges = [
                0x20u32..0x7F,
                0xA0u32..0x300,
                0x3000u32..0x3100,
                0x4E00u32..0xA000,
                0xFF00u32..0xFFF0,
            ];
            for range in ranges {
                for cp in range {
                    if let Some(c) = char::from_u32(cp) {
                        let id = guest.glyph(c).id();
                        // Glyph 0 is .notdef - the character is not in this font.
                        if id.0 != 0 {
                            gid_remap.entry(id.0).or_insert(c);
                        }
                    }
                }
            }
        }
        let raw_copy = override_bytes.clone();
        let font = rusttype::Font::try_from_vec(override_bytes)?;
        log!(
            "[DSfont] stand-in for guest font: {} glyph ids translated",
            gid_remap.len()
        );
        Some(Font {
            font: Some(font),
            raw_data: Some(raw_copy),
            gid_remap: Some(gid_remap),
        })
    }

    pub fn mono_regular() -> Font {
        Self::from_resource_file("LiberationMono-Regular.ttf")
    }
    pub fn mono_bold() -> Font {
        Self::from_resource_file("LiberationMono-Bold.ttf")
    }
    pub fn mono_bold_italic() -> Font {
        Self::from_resource_file("LiberationMono-BoldItalic.ttf")
    }
    pub fn mono_italic() -> Font {
        Self::from_resource_file("LiberationMono-Italic.ttf")
    }
    pub fn sans_regular() -> Font {
        Self::from_resource_file("LiberationSans-Regular.ttf")
    }
    pub fn sans_bold() -> Font {
        Self::from_resource_file("LiberationSans-Bold.ttf")
    }
    pub fn sans_bold_italic() -> Font {
        Self::from_resource_file("LiberationSans-BoldItalic.ttf")
    }
    pub fn sans_italic() -> Font {
        Self::from_resource_file("LiberationSans-Italic.ttf")
    }
    pub fn serif_regular() -> Font {
        Self::from_resource_file("LiberationSerif-Regular.ttf")
    }
    pub fn serif_bold() -> Font {
        Self::from_resource_file("LiberationSerif-Bold.ttf")
    }
    pub fn serif_bold_italic() -> Font {
        Self::from_resource_file("LiberationSerif-BoldItalic.ttf")
    }
    pub fn serif_italic() -> Font {
        Self::from_resource_file("LiberationSerif-Italic.ttf")
    }
    pub fn sans_regular_ja() -> Font {
        Self::from_resource_file("NotoSansJP-Regular.otf")
    }
    pub fn sans_bold_ja() -> Font {
        Self::from_resource_file("NotoSansJP-Bold.otf")
    }
    pub fn sans_regular_ar() -> Font {
        Self::from_resource_file("NotoSansArabic-Regular.ttf")
    }
    pub fn sans_bold_ar() -> Font {
        Self::from_resource_file("NotoSansArabic-Bold.ttf")
    }
pub fn sans_regular_zh() -> Font {
    Self::from_resource_file("NotoSansSC-Regular.otf")
}

pub fn sans_bold_zh() -> Font {
    Self::from_resource_file("NotoSansSC-Bold.otf")
}

    pub fn ascent(&self, font_size: f32) -> f32 {
        let v_metrics = self.rt().v_metrics(scale(font_size));
        v_metrics.ascent
    }
    pub fn descent(&self, font_size: f32) -> f32 {
        let v_metrics = self.rt().v_metrics(scale(font_size));
        v_metrics.descent
    }

    pub fn line_gap(&self, font_size: f32) -> f32 {
        let v_metrics = self.rt().v_metrics(scale(font_size));
        v_metrics.line_gap
    }

    fn line_height_and_gap(&self, font_size: f32) -> (f32, f32) {
        let v_metrics = self.rt().v_metrics(scale(font_size));
        (v_metrics.ascent - v_metrics.descent, v_metrics.line_gap)
    }

    /// Calculate the width of a line. This does not handle newlines!
    fn calculate_line_width(&self, font_size: f32, line: &str) -> f32 {
        let mut line_x_min: f32 = 0.0;
        let mut line_x_max: f32 = 0.0;

        // Apply Arabic shaping/reordering so the measured width matches what
        // will actually be drawn (see [arabic::shape_line_for_display]).
        let line = arabic::shape_line_for_display(line);

        for glyph in self.rt().layout(&line, scale(font_size), Default::default()) {
            let position = glyph.position();
            let h_metrics = glyph.unpositioned().h_metrics();

            // This method used to use pixel_bounding_box() for metrics, but
            // now uses h_metrics() in order to support whitespace characters.
            // This definition of character width was chosen because it gave
            // similar results to the old implementation, not because it's
            // optimal; maybe it could be improved.
            let glyph_x_min = position.x.min(position.x + h_metrics.left_side_bearing);
            let glyph_x_max = position.x + h_metrics.advance_width;

            line_x_min = line_x_min.min(glyph_x_min);
            line_x_min = line_x_min.min(glyph_x_max);
            line_x_max = line_x_max.max(glyph_x_min);
            line_x_max = line_x_max.max(glyph_x_max);
        }

        // This rounding is also to emulate pixel_bounding_box(), same caveat
        // applies.
        line_x_max.ceil() - line_x_min.floor()
    }

    /// Break text into lines with known widths.
    pub fn break_lines<'a>(
        &self,
        font_size: f32,
        text: &'a str,
        wrap: Option<(f32, WrapMode)>,
    ) -> Vec<(f32, &'a str)> {
        let mut lines = Vec::new();

        for line in text.lines() {
            let Some((wrap_width, wrap_mode)) = wrap else {
                lines.push((self.calculate_line_width(font_size, line), line));
                continue;
            };

            let unwrapped_line = line;

            // Find points at which the line could be wrapped
            let mut wrap_points = Vec::new();
            match wrap_mode {
                WrapMode::Word => {
                    let mut word_start = 0;

                    loop {
                        let Some(i) = line[word_start..].find(|c: char| c.is_whitespace()) else {
                            break;
                        };
                        let word_end = word_start + i;
                        // Include any additional whitespace in the word,
                        // so that the next word begins with non-whitespace.
                        let Some(i) = line[word_end..].find(|c: char| !c.is_whitespace()) else {
                            break;
                        };
                        wrap_points.push(word_end + i);
                        word_start = word_end + i;
                    }
                }
                WrapMode::Char => {
                    let mut char_end = 1;
                    while char_end < line.len() {
                        if line.is_char_boundary(char_end) {
                            wrap_points.push(char_end);
                        }
                        char_end += 1;
                    }
                }
            };
            wrap_points.push(line.len());

            let mut next_wrap_point_idx = 0;
            let mut line_start = 0;

            fn trim_wrapped_line(wrap_mode: WrapMode, line: &str) -> &str {
                // Spaces before a word wrap point are ignored for
                // wrapping purposes.
                if wrap_mode == WrapMode::Word {
                    line.trim_end()
                } else {
                    line
                }
            }

            while next_wrap_point_idx < wrap_points.len() {
                // Find optimal line wrapping by binary search.
                // `binary_search_by` returns Err when there's no exactly
                // matching line length, which is usually going to be the case.
                let wrap_search_result =
                    wrap_points[next_wrap_point_idx..].binary_search_by(|&wrap_point| {
                        let line = &line[line_start..wrap_point];
                        let line_width = self
                            .calculate_line_width(font_size, trim_wrapped_line(wrap_mode, line));
                        line_width.partial_cmp(&wrap_width).unwrap()
                    });
                let wrap_point_idx = match wrap_search_result {
                    Ok(i) => next_wrap_point_idx + i,
                    Err(i @ 1..) => next_wrap_point_idx + (i - 1),
                    _ => {
                        // The span between the current wrap point and the next
                        // wrap point is wider than the wrap width. In practice,
                        // this means a word too big to fit on-screen.
                        if matches!(wrap_mode, WrapMode::Word) {
                            // Try to break the word.
                            let word_end = wrap_points[next_wrap_point_idx];
                            let word = &line[line_start..word_end];
                            let broken_words = self.break_lines(
                                font_size,
                                word,
                                Some((wrap_width, WrapMode::Char)),
                            );
                            lines.extend(broken_words);
                            next_wrap_point_idx += 1;
                            line_start = word_end;
                            continue;
                        }
                        // It can't be helped: truncate.
                        next_wrap_point_idx
                    }
                };
                let line_end = wrap_points[wrap_point_idx];
                let line = &line[line_start..line_end];

                let trimmed_line = if line_end != unwrapped_line.len() {
                    // Whitespace at the end of a line must only be ignored if
                    // that line break came from word wrapping.
                    trim_wrapped_line(wrap_mode, line)
                } else {
                    line
                };

                lines.push((
                    self.calculate_line_width(font_size, trimmed_line),
                    trimmed_line,
                ));

                next_wrap_point_idx = wrap_point_idx + 1;
                line_start = line_end;
            }
        }

        lines
    }

    /// Calculate the on-screen width and height of text with a given font size.
    pub fn calculate_text_size(
        &self,
        font_size: f32,
        text: &str,
        wrap: Option<(f32, WrapMode)>,
    ) -> (f32, f32) {
        let lines = self.break_lines(font_size, text, wrap);

        let width = lines
            .iter()
            .fold(0f32, |widest, &(line_width, _line)| widest.max(line_width));
        let (line_height, line_gap) = self.line_height_and_gap(font_size);
        let height =
            line_height * (lines.len() as f32) + line_gap * (lines.len().saturating_sub(1) as f32);

        (width, height)
    }

    /// Draw text. Calls the provided callback for each glyph that is to be
    /// drawn. Assumes y starts at the top-left corner and points downwards.
    /// Used by UIKit for font rendering.
    pub fn draw<F: FnMut(RasterGlyph)>(
        &self,
        font_size: f32,
        text: &str,
        origin: (f32, f32),
        wrap: Option<(f32, WrapMode)>,
        alignment: TextAlignment,
        mut draw_glyph: F,
    ) {
        note_text_path("font::draw(str)", text.len());
        let lines = self.break_lines(font_size, text, wrap);

        let ascent = self.rt().v_metrics(scale(font_size)).ascent;
        let (line_height, line_gap) = self.line_height_and_gap(font_size);

        // RustType requires a "draw pixel" callback that will be called for
        // each pixel in the glyph's bounding box, in left-to-right
        // top-to-bottom order. This is unfortunately incompatible with
        // touchHLE's code which needs to be able to sample the pixels in any
        // order in order to support rotation. This is worked around by creating
        // a temporary bitmap for the glyph, and then the caller of this
        // function can provide a "draw glyph" callback that can do whatever it
        // wants with this bitmap.
        // TODO: Do we need to increase the font size when scale transforms are
        //       used, to avoid blurry text?
        let mut glyph_bitmap: Vec<f32> = Vec::new();

        for (line_idx, (line_width, line_text)) in lines.iter().enumerate() {
            let line_x_offset = match alignment {
                TextAlignment::Left => 0.0,
                TextAlignment::Center => -line_width / 2.0,
                TextAlignment::Right => -line_width,
            };

            let baseline = origin.1 + ascent + line_idx as f32 * (line_gap + line_height);

            // Reshape/reorder Arabic text into visual order before layout.
            let shaped_line = arabic::shape_line_for_display(line_text);

            for glyph in self.rt().layout(
                &shaped_line,
                scale(font_size),
                Point {
                    x: origin.0 + line_x_offset,
                    y: baseline,
                },
            ) {
                let Some(glyph_bounds) = glyph.pixel_bounding_box() else {

                    continue;
                };

                // TODO: Refactor this method to support y clipping too.
                // It's not mandatory since the caller can do it, but it would
                // be more efficient.
                if let Some((wrap_width, _)) = wrap {
                    if glyph_bounds.min.x as f32 > origin.0 + wrap_width {
                        // Avoid wasting effort on glyphs that are entirely
                        // clipped. Partial clipping is the responsibility of
                        // the draw_glyph implementation.
                        continue;
                    }
                }

                let glyph_bitmap_bounds = (
                    glyph_bounds.width() as usize,
                    glyph_bounds.height() as usize,
                );
                glyph_bitmap.clear();
                glyph_bitmap.resize(glyph_bitmap_bounds.0 * glyph_bitmap_bounds.1, 0.0);

                glyph.draw(|x, y, coverage| {
                    glyph_bitmap[y as usize * glyph_bitmap_bounds.0 + x as usize] = coverage;
                });

                let raster_glyph = RasterGlyph {
                    origin: (glyph_bounds.min.x as f32, glyph_bounds.min.y as f32),
                    dimensions: (glyph_bitmap_bounds.0 as i32, glyph_bitmap_bounds.1 as i32),
                    pixels: &glyph_bitmap,
                };

                draw_glyph(raster_glyph);
            }
        }
    }

    /// Draw glyphs. Similar to [Self::draw], but uses raw glyph ids instead of
    /// text and doesn't account for line breaks or text alignment (those
    /// should be handled by the caller). Used by CoreGraphics for font
    /// rendering.
    /// TODO: unify with [Self::draw]. Note: y sense is different! If you
    /// plan to do that refactoring, make sure that there are no visual
    /// regressions in the GUI tests for CGFont/CGGlyph!
    pub fn draw_glyphs<I, F>(
        &self,
        font_size: f32,
        glyphs: I,
        origin: (f32, f32),
        text_transform: CGAffineTransform,
        baseline_at_origin: bool,
        mut draw_glyph: F,
    ) where
        I: IntoIterator<Item = GlyphId>,
        F: FnMut(RasterGlyph),
    {
        note_text_path("font::glyphs", 0);
        // Substitute-face glyph-id translation (see `gid_remap`).
        // Both branches have to produce the same iterator type, so collect
        // first and hand out a Vec iterator below.
        let mapped_glyphs: Vec<GlyphId> = if let Some(map) = &self.gid_remap {
            let font = self.rt();
            glyphs
                .into_iter()
                .map(|g| {
                    // `TOUCHHLE_DS_LOG_GLYPHS=1`: show how guest glyph ids are
                    // translated (digits and unmapped ids only, capped) - this is
                    // how the `档案 1+` / `05:04:33` breakage is being chased.
                    if std::env::var_os("TOUCHHLE_DS_LOG_GLYPHS").is_some() {
                        static N: std::sync::atomic::AtomicUsize =
                            std::sync::atomic::AtomicUsize::new(0);
                        match map.get(&g.0) {
                            Some(c) => {
                                if c.is_ascii_digit() && N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 120 {
                                    log!(
                                        "[DSglyph] gid {} -> {:?} -> sub gid {}",
                                        g.0,
                                        c,
                                        font.glyph(*c).id().0
                                    );
                                }
                            }
                            None => {
                                if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 120 {
                                    log!("[DSglyph] gid {} UNMAPPED (kept as-is)", g.0);
                                }
                            }
                        }
                    }
                    map.get(&g.0).map(|c| font.glyph(*c).id()).unwrap_or(g)
                })
                .collect()
        } else {
            glyphs.into_iter().collect()
        };
        let glyphs = mapped_glyphs.into_iter();
        // TODO: avoid creating a tmp bitmap
        let mut tmp_glyph_bitmap: Vec<f32> = Vec::new();
        // Cf. comment in the [Self::draw] function.
        let mut glyph_bitmap: Vec<f32> = Vec::new();

        let inverted_text_transform = text_transform.invert();

        // Optional supersampling for the glyph bitmap
        // (`TOUCHHLE_DS_CG_GLYPH_DENSITY`, default 1). The bitmap is rasterised
        // at this many times the text transform's scale while the drawn size is
        // divided by the same factor further down, so the text keeps its size
        // and only gets sharper. Cost grows with the square of the factor, so
        // values above ~3 are usually not worth it.
        let ds_glyph_density = std::env::var("TOUCHHLE_DS_CG_GLYPH_DENSITY")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| *v > 0.0)
            .unwrap_or(1.0);

        // rusttype only supports scaling, so we render each glyph at a
        // resolution matched to the effective scale of the text
        // transform and then resample to apply rotation/mirror/etc.
        // TODO: apply x and y scales independently?
        let unit_x = text_transform.apply_to_size(CGSize {
            width: 1.0,
            height: 0.0,
        });
        let unit_y = text_transform.apply_to_size(CGSize {
            width: 0.0,
            height: 1.0,
        });
        let tmp_font_scale = unit_x
            .width
            .hypot(unit_x.height)
            .max(unit_y.width.hypot(unit_y.height))
            .max(1.0)
            * ds_glyph_density;

        // Dead Space 1.3.34 compatibility knobs (see REPORT.md 阶段 13):
        // the app rasterises its Latin/digit glyph atlas through
        // CGContextShowGlyphsAtPoint; we can tune the rasterisation size and
        // vertical placement at run time to line the glyph ink up with the
        // atlas cell the app samples. Defaults keep upstream behaviour.
        let ds_glyph_scale = std::env::var("TOUCHHLE_DS_CG_GLYPH_SCALE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(1.0);
        let ds_glyph_dy = std::env::var("TOUCHHLE_DS_CG_GLYPH_DY")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0);
        let eff_scale = tmp_font_scale * ds_glyph_scale;

        let start = Point { x: 0.0, y: 0.0 };
        // This code is adapted from documentation of [rusttype::Font::layout].
        let iter = self.rt().glyphs_for(glyphs)
            .scan((None, 0.0), |(last, x), g| {
                let g = g.scaled(scale(font_size * eff_scale));
                if let Some(last) = last {
                    *x += self.rt().pair_kerning(
                        scale(font_size * eff_scale),
                        *last,
                        g.id(),
                    );
                }
                let w = if let Some((advance, _)) = self.hmtx_metrics(g.id().0 as u32) {
advance as f32 / self.units_per_em() as f32 * (font_size * eff_scale) } else { g.h_metrics().advance_width };
                let next = g.positioned(start + vector(*x, 0.0));
                *last = Some(next.id());
                *x += w;
                Some(next)
            });
        for glyph in iter {
            let Some(glyph_bounds) = glyph.pixel_bounding_box() else {
                {
                    use std::sync::atomic::{AtomicU32, Ordering};
                    static N: AtomicU32 = AtomicU32::new(0);
                    if N.fetch_add(1, Ordering::Relaxed) < 16 {
                        log!(
                            "[glyphdbg] pixel_bounding_box() = None -> glyph {:?} skipped (font_size={} tmp_scale={})",
                            glyph.id(),
                            font_size,
                            tmp_font_scale
                        );
                    }
                }
                continue;
            };
            let rect = CGRect {
                origin: CGPoint {
                    x: glyph_bounds.min.x as f32 / tmp_font_scale,
                    // Note: this is because y is inverted!
                    // Dead Space 1.3.34 draws its Latin/digit glyph atlas
                    // through CGContextShowGlyphsAtPoint using a top-down
                    // baseline: the glyph ink sits directly above the caller's
                    // y. With this flag the rect starts at the (already
                    // flipped) baseline instead of hanging below it a second
                    // time, which left the sampled atlas cells empty.
                    y: if baseline_at_origin {
                        0.0
                    } else {
                        -glyph_bounds.max.y as f32 / tmp_font_scale
                    },
                },
                size: CGSize {
                    width: glyph_bounds.width() as f32 / tmp_font_scale,
                    height: glyph_bounds.height() as f32 / tmp_font_scale,
                },
            };

            let transformed = text_transform.apply_to_rect(rect);
            assert!(rect.size.width >= 0.0 && rect.size.height >= 0.0);
            log_dbg!(
                "draw_glyphs: glyph {:?}, bounds {:?}, rect {:?}, transformed {:?}",
                glyph,
                glyph_bounds,
                rect,
                transformed
            );

            let x_offset = (origin.0 + transformed.origin.x).round() as i32;
            let y_offset = (origin.1 + ds_glyph_dy + transformed.origin.y).round() as i32;

            let tmp_glyph_bitmap_bounds = (
                glyph_bounds.width() as usize,
                glyph_bounds.height() as usize,
            );
            log_dbg!("tmp_glyph_bitmap_bounds {:?}", tmp_glyph_bitmap_bounds);
            tmp_glyph_bitmap.clear();
            tmp_glyph_bitmap.resize(tmp_glyph_bitmap_bounds.0 * tmp_glyph_bitmap_bounds.1, 0.0);

            glyph.draw(|x, y, coverage| {
                // Note: need to fill the bitmap in the reverse y order to
                // account for y sense
                tmp_glyph_bitmap[(tmp_glyph_bitmap_bounds.1 - 1 - y as usize)
                    * tmp_glyph_bitmap_bounds.0
                    + x as usize] = coverage;
            });

            // Ceil so fractional pixels at the right/bottom edge aren't
            // clipped.
            let glyph_bitmap_bounds = (
                transformed.size.width.ceil() as usize,
                transformed.size.height.ceil() as usize,
            );
            log_dbg!(
                "x_offset {}, y_offset {}, size {:?}",
                x_offset,
                y_offset,
                glyph_bitmap_bounds
            );
            glyph_bitmap.clear();
            glyph_bitmap.resize(glyph_bitmap_bounds.0 * glyph_bitmap_bounds.1, 0.0);

            for y in 0..glyph_bitmap_bounds.1 {
                for x in 0..glyph_bitmap_bounds.0 {
                    let point = CGPoint {
                        x: transformed.origin.x + x as f32 + 0.5,
                        y: transformed.origin.y + y as f32 + 0.5,
                    };
                    let orig = inverted_text_transform.apply_to_point(point);
                    let orig_x = (orig.x * tmp_font_scale - glyph_bounds.min.x as f32) as i32;
                    let orig_y = (orig.y * tmp_font_scale + glyph_bounds.max.y as f32) as i32;
                    log_dbg!("({}, {}) -> ({}, {})", x, y, orig_x, orig_y);
                    if orig_x >= 0
                        && orig_y >= 0
                        && orig_x < tmp_glyph_bitmap_bounds.0 as i32
                        && orig_y < tmp_glyph_bitmap_bounds.1 as i32
                    {
                        let idx_orig =
                            (orig_y * tmp_glyph_bitmap_bounds.0 as i32 + orig_x) as usize;
                        glyph_bitmap[y * glyph_bitmap_bounds.0 + x] = tmp_glyph_bitmap[idx_orig];
                    }
                }
            }

            let raster_glyph = RasterGlyph {
                origin: (x_offset as f32, y_offset as f32),
                dimensions: (glyph_bitmap_bounds.0 as _, glyph_bitmap_bounds.1 as _),
                pixels: &glyph_bitmap,
            };

            draw_glyph(raster_glyph);
        }
    }

    /// Draw glyphs at specified positions. Each glyph is placed at its
    /// corresponding position (x, y). Used by CGContextShowGlyphsAtPositions.
    /// The y sense matches CoreGraphics (y growing upward in user space).
    pub fn draw_glyphs_at_positions<F>(
        &self,
        font_size: f32,
        glyphs: &[GlyphId],
        positions: &[(f32, f32)],
        origin: (f32, f32),
        mut draw_glyph: F,
    ) where
        F: FnMut(RasterGlyph),
    {
        let mut glyph_bitmap: Vec<f32> = Vec::new();
        // Supersampling, same knob as `draw_glyphs`
        // (`TOUCHHLE_DS_CG_GLYPH_DENSITY`). Dead Space renders its *localised*
        // menu text through CGContextShowGlyphsAtPositions, which previously
        // ignored this knob entirely, so the density change had no visible
        // effect. Rasterise at density x and box-downsample back to 1x.
        let density = glyph_density();

        for (glyph_id, pos) in glyphs.iter().zip(positions.iter()) {
        note_text_path("font::positions", 0);
            let x = origin.0 + pos.0;
            let y = origin.1 + pos.1;

            let mapped_glyph_id = if let Some(map) = &self.gid_remap {
                map.get(&glyph_id.0)
                    .map(|character| {
                        let id = self.rt().glyph(*character).id();
                        if id.0 == 0 { *glyph_id } else { id }
                    })
                    .unwrap_or(*glyph_id)
            } else {
                *glyph_id
            };
            let g = self.rt().glyph(mapped_glyph_id)
                .scaled(scale(font_size * density))
                .positioned(Point { x: x * density, y: 0.0 });

            let Some(glyph_bounds) = g.pixel_bounding_box() else {
                continue;
            };

            let hi_bounds = (
                glyph_bounds.width() as usize,
                glyph_bounds.height() as usize,
            );
            glyph_bitmap.clear();
            glyph_bitmap.resize(hi_bounds.0 * hi_bounds.1, 0.0);

            g.draw(|x, y, coverage| {
                glyph_bitmap[(hi_bounds.1 - 1 - y as usize) * hi_bounds.0
                    + x as usize] = coverage;
            });

            let (x_offset, y_offset, out_bounds) = if density > 1.0001 {
                let d = density.round().max(1.0) as usize;
                let ow = (hi_bounds.0 + d - 1) / d;
                let oh = (hi_bounds.1 + d - 1) / d;
                let mut lo = vec![0.0f32; ow * oh];
                for oy in 0..oh {
                    for ox in 0..ow {
                        let mut sum = 0.0f32;
                        let mut n = 0.0f32;
                        for sy in 0..d {
                            for sx in 0..d {
                                let (yy, xx) = (oy * d + sy, ox * d + sx);
                                if yy < hi_bounds.1 && xx < hi_bounds.0 {
                                    sum += glyph_bitmap[yy * hi_bounds.0 + xx];
                                    n += 1.0;
                                }
                            }
                        }
                        lo[oy * ow + ox] = if n > 0.0 { sum / n } else { 0.0 };
                    }
                }
                glyph_bitmap = lo;
                (
                    (glyph_bounds.min.x as f32 / density).floor() as i32,
                    (y.round() as i32) - (glyph_bounds.max.y as f32 / density).ceil() as i32,
                    (ow, oh),
                )
            } else {
                (
                    glyph_bounds.min.x,
                    (y.round() as i32) - glyph_bounds.max.y,
                    hi_bounds,
                )
            };

            let raster_glyph = RasterGlyph {
                origin: (x_offset as f32, y_offset as f32),
                dimensions: (out_bounds.0 as _, out_bounds.1 as _),
                pixels: &glyph_bitmap,
            };

            draw_glyph(raster_glyph);
        }
    }

    /// Draw glyphs with explicit advances. Each glyph is placed sequentially
    /// with the given advance (dx, dy) offsets. Used by
    /// CGContextShowGlyphsWithAdvances.
    pub fn draw_glyphs_with_advances<F>(
        &self,
        font_size: f32,
        glyphs: &[GlyphId],
        advances: &[(f32, f32)],
        origin: (f32, f32),
        mut draw_glyph: F,
    ) where
        F: FnMut(RasterGlyph),
    {
        let mut glyph_bitmap: Vec<f32> = Vec::new();
        let mut current_x = origin.0;
        let mut current_y = origin.1;

        for (i, glyph_id) in glyphs.iter().enumerate() {
        note_text_path("font::advances", 0);
            let mapped_glyph_id = if let Some(map) = &self.gid_remap {
                map.get(&glyph_id.0)
                    .map(|character| {
                        let id = self.rt().glyph(*character).id();
                        if id.0 == 0 { *glyph_id } else { id }
                    })
                    .unwrap_or(*glyph_id)
            } else {
                *glyph_id
            };
            let g = self.rt().glyph(mapped_glyph_id)
                .scaled(scale(font_size))
                .positioned(Point {
                    x: current_x,
                    y: 0.0,
                });

            let Some(glyph_bounds) = g.pixel_bounding_box() else {
                // Even if the glyph has no outline, advance the pen.
                if i < advances.len() {
                    current_x += advances[i].0;
                    current_y += advances[i].1;
                }
                continue;
            };

            let x_offset = glyph_bounds.min.x;
            let y_offset = (current_y.round() as i32) - glyph_bounds.max.y;

            let glyph_bitmap_bounds = (
                glyph_bounds.width() as usize,
                glyph_bounds.height() as usize,
            );
            glyph_bitmap.clear();
            glyph_bitmap.resize(glyph_bitmap_bounds.0 * glyph_bitmap_bounds.1, 0.0);

            g.draw(|x, y, coverage| {
                glyph_bitmap[(glyph_bitmap_bounds.1 - 1 - y as usize) * glyph_bitmap_bounds.0
                    + x as usize] = coverage;
            });

            let raster_glyph = RasterGlyph {
                origin: (x_offset as f32, y_offset as f32),
                dimensions: (glyph_bitmap_bounds.0 as _, glyph_bitmap_bounds.1 as _),
                pixels: &glyph_bitmap,
            };

            draw_glyph(raster_glyph);

            if i < advances.len() {
                current_x += advances[i].0;
                current_y += advances[i].1;
            }
        }
    }
}

// =============================================================================
// MARK: - TrueType/OpenType table parsing helpers
// =============================================================================

/// Parse the table directory from raw font file bytes and return all table tags.
fn parse_table_tags_from_raw(data: &[u8]) -> Vec<u32> {
    if data.len() < 12 {
        return Vec::new();
    }

    // Check for TTC (TrueType Collection) — we only handle the first font.
    let offset_to_dir = if &data[0..4] == b"ttcf" {
        if data.len() < 16 {
            return Vec::new();
        }
        // offsetTable[0] is at byte 12 in TTC header
        u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize
    } else {
        0
    };

    if data.len() < offset_to_dir + 12 {
        return Vec::new();
    }

    let num_tables =
        u16::from_be_bytes([data[offset_to_dir + 4], data[offset_to_dir + 5]]) as usize;

    let table_dir_start = offset_to_dir + 12;
    let mut tags = Vec::with_capacity(num_tables);

    for i in 0..num_tables {
        let entry_offset = table_dir_start + i * 16;
        if data.len() < entry_offset + 4 {
            break;
        }
        let tag = u32::from_be_bytes([
            data[entry_offset],
            data[entry_offset + 1],
            data[entry_offset + 2],
            data[entry_offset + 3],
        ]);
        tags.push(tag);
    }

    tags
}

/// Parse a specific table from raw font file bytes by its 4-byte tag.
fn parse_table_from_raw(data: &[u8], tag: u32) -> Option<Vec<u8>> {
    if data.len() < 12 {
        return None;
    }

    let offset_to_dir = if &data[0..4] == b"ttcf" {
        if data.len() < 16 {
            return None;
        }
        u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize
    } else {
        0
    };

    if data.len() < offset_to_dir + 12 {
        return None;
    }

    let num_tables =
        u16::from_be_bytes([data[offset_to_dir + 4], data[offset_to_dir + 5]]) as usize;

    let table_dir_start = offset_to_dir + 12;

    for i in 0..num_tables {
        let entry_offset = table_dir_start + i * 16;
        if data.len() < entry_offset + 16 {
            break;
        }
        let entry_tag = u32::from_be_bytes([
            data[entry_offset],
            data[entry_offset + 1],
            data[entry_offset + 2],
            data[entry_offset + 3],
        ]);
        if entry_tag == tag {
            let offset = u32::from_be_bytes([
                data[entry_offset + 8],
                data[entry_offset + 9],
                data[entry_offset + 10],
                data[entry_offset + 11],
            ]) as usize;
            let length = u32::from_be_bytes([
                data[entry_offset + 12],
                data[entry_offset + 13],
                data[entry_offset + 14],
                data[entry_offset + 15],
            ]) as usize;
            if offset + length <= data.len() {
                return Some(data[offset..offset + length].to_vec());
            }
            return None;
        }
    }

    None
}

// Standard PostScript glyph names for glyph indices 0..257 (Macintosh
// standard ordering) used by 'post' table format 1.0 and 2.0.
const STANDARD_MAC_GLYPH_NAMES: &[&str] = &[
    ".notdef", ".null", "nonmarkingreturn", "space", "exclam", "quotedbl",
    "numbersign", "dollar", "percent", "ampersand", "quotesingle", "parenleft",
    "parenright", "asterisk", "plus", "comma", "hyphen", "period", "slash",
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight",
    "nine", "colon", "semicolon", "less", "equal", "greater", "question", "at",
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N",
    "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
    "bracketleft", "backslash", "bracketright", "asciicircum", "underscore",
    "grave", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l",
    "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    "braceleft", "bar", "braceright", "asciitilde", "Adieresis", "Aring",
    "Ccedilla", "Eacute", "Ntilde", "Odieresis", "Udieresis", "aacute",
    "agrave", "acircumflex", "adieresis", "atilde", "aring", "ccedilla",
    "eacute", "egrave", "ecircumflex", "edieresis", "iacute", "igrave",
    "icircumflex", "idieresis", "ntilde", "oacute", "ograve", "ocircumflex",
    "odieresis", "otilde", "uacute", "ugrave", "ucircumflex", "udieresis",
    "dagger", "degree", "cent", "sterling", "section", "bullet", "paragraph",
    "germandbls", "registered", "copyright", "trademark", "acute", "dieresis",
    "notequal", "AE", "Oslash", "infinity", "plusminus", "lessequal",
    "greaterequal", "yen", "mu", "partialdiff", "summation", "product", "pi",
    "integral", "ordfeminine", "ordmasculine", "Omega", "ae", "oslash",
    "questiondown", "exclamdown", "logicalnot", "radical", "florin",
    "approxequal", "Delta", "guillemotleft", "guillemotright", "ellipsis",
    "nonbreakingspace", "Agrave", "Atilde", "Otilde", "OE", "oe", "endash",
    "emdash", "quotedblleft", "quotedblright", "quoteleft", "quoteright",
    "divide", "lozenge", "ydieresis", "Ydieresis", "fraction", "currency",
    "guilsinglleft", "guilsinglright", "fi", "fl", "daggerdbl", "periodcentered",
    "quotesinglbase", "quotedblbase", "perthousand", "Acircumflex",
    "Ecircumflex", "Aacute", "Edieresis", "Egrave", "Iacute", "Icircumflex",
    "Idieresis", "Igrave", "Oacute", "Ocircumflex", "apple", "Ograve",
    "Uacute", "Ucircumflex", "Ugrave", "dotlessi", "circumflex", "tilde",
    "macron", "breve", "dotaccent", "ring", "cedilla", "hungarumlaut",
    "ogonek", "caron", "Lslash", "lslash", "Scaron", "scaron", "Zcaron",
    "zcaron", "brokenbar", "Eth", "eth", "Yacute", "yacute", "Thorn",
    "thorn", "minus", "multiply", "onesuperior", "twosuperior", "threesuperior",
    "onehalf", "onequarter", "threequarters", "franc", "Gbreve", "gbreve",
    "Idotaccent", "Scedilla", "scedilla", "Cacute", "cacute", "Ccaron",
    "ccaron", "dcroat",
];

/// Look up a glyph index from a 'post' table by PostScript name.
fn glyph_id_from_post_table(post_data: &[u8], name: &str, _glyph_count: u32) -> Option<GlyphId> {
    if post_data.len() < 32 {
        return None;
    }

    let format = u32::from_be_bytes([post_data[0], post_data[1], post_data[2], post_data[3]]);

    match format {
        // Format 1.0: standard 258 Macintosh glyphs
        0x00010000 => {
            for (i, &std_name) in STANDARD_MAC_GLYPH_NAMES.iter().enumerate() {
                if std_name == name {
                    return Some(GlyphId(i as u16));
                }
            }
            None
        }
        // Format 2.0: custom glyph names
        0x00020000 => {
            if post_data.len() < 34 {
                return None;
            }
            let num_glyphs =
                u16::from_be_bytes([post_data[32], post_data[33]]) as usize;
            let indices_start = 34;
            let indices_end = indices_start + num_glyphs * 2;
            if post_data.len() < indices_end {
                return None;
            }

            // Parse the string table (Pascal strings after the index array)
            let mut extra_names: Vec<&str> = Vec::new();
            let mut pos = indices_end;
            while pos < post_data.len() {
                let len = post_data[pos] as usize;
                pos += 1;
                if pos + len > post_data.len() {
                    break;
                }
                let s = std::str::from_utf8(&post_data[pos..pos + len]).unwrap_or("");
                extra_names.push(s);
                pos += len;
            }

            for glyph_idx in 0..num_glyphs {
                let idx_offset = indices_start + glyph_idx * 2;
                let name_index = u16::from_be_bytes([
                    post_data[idx_offset],
                    post_data[idx_offset + 1],
                ]) as usize;

                let glyph_name = if name_index < 258 {
                    STANDARD_MAC_GLYPH_NAMES
                        .get(name_index)
                        .copied()
                        .unwrap_or("")
                } else {
                    let extra_idx = name_index - 258;
                    extra_names.get(extra_idx).copied().unwrap_or("")
                };

                if glyph_name == name {
                    return Some(GlyphId(glyph_idx as u16));
                }
            }
            None
        }
        _ => None,
    }
}

/// Get the PostScript name of a glyph from the 'post' table.
fn glyph_name_from_post_table(
    post_data: &[u8],
    glyph_index: u32,
    _glyph_count: u32,
) -> Option<String> {
    if post_data.len() < 32 {
        return None;
    }

    let format = u32::from_be_bytes([post_data[0], post_data[1], post_data[2], post_data[3]]);

    match format {
        // Format 1.0
        0x00010000 => STANDARD_MAC_GLYPH_NAMES
            .get(glyph_index as usize)
            .map(|s| s.to_string()),
        // Format 2.0
        0x00020000 => {
            if post_data.len() < 34 {
                return None;
            }
            let num_glyphs =
                u16::from_be_bytes([post_data[32], post_data[33]]) as usize;
            if glyph_index as usize >= num_glyphs {
                return None;
            }
            let indices_start = 34;
            let indices_end = indices_start + num_glyphs * 2;
            if post_data.len() < indices_end {
                return None;
            }

            let idx_offset = indices_start + glyph_index as usize * 2;
            let name_index = u16::from_be_bytes([
                post_data[idx_offset],
                post_data[idx_offset + 1],
            ]) as usize;

            if name_index < 258 {
                return STANDARD_MAC_GLYPH_NAMES
                    .get(name_index)
                    .map(|s| s.to_string());
            }

            // Parse extra names
            let extra_idx = name_index - 258;
            let mut pos = indices_end;
            let mut current_extra = 0usize;
            while pos < post_data.len() {
                let len = post_data[pos] as usize;
                pos += 1;
                if pos + len > post_data.len() {
                    break;
                }
                if current_extra == extra_idx {
                    return std::str::from_utf8(&post_data[pos..pos + len])
                        .ok()
                        .map(|s| s.to_string());
                }
                pos += len;
                current_extra += 1;
            }
            None
        }
        _ => None,
    }
}

/// Map a standard Adobe/PostScript glyph name to the character it represents.
///
/// Used as a fallback when a font's `post` table is format 3 (no glyph names):
/// callers still ask for glyphs by name, so we translate the common names to
/// characters and resolve them through the font's cmap instead.
fn standard_glyph_name_to_char(name: &str) -> Option<char> {
    // "uniXXXX"
    if let Some(hex) = name.strip_prefix("uni") {
        if hex.len() == 4 {
            if let Ok(v) = u32::from_str_radix(hex, 16) {
                return char::from_u32(v);
            }
        }
    }
    // Single ASCII letters and digits are their own names ("A", "a", "7").
    if name.len() == 1 {
        let c = name.chars().next().unwrap();
        if c.is_ascii_alphanumeric() {
            return Some(c);
        }
    }
    const NAMES: &[(&str, char)] = &[
        ("space", ' '), ("exclam", '!'), ("quotedbl", '"'), ("numbersign", '#'),
        ("dollar", '$'), ("percent", '%'), ("ampersand", '&'), ("quotesingle", '\''),
        ("parenleft", '('), ("parenright", ')'), ("asterisk", '*'), ("plus", '+'),
        ("comma", ','), ("hyphen", '-'), ("period", '.'), ("slash", '/'),
        ("zero", '0'), ("one", '1'), ("two", '2'), ("three", '3'), ("four", '4'),
        ("five", '5'), ("six", '6'), ("seven", '7'), ("eight", '8'), ("nine", '9'),
        ("colon", ':'), ("semicolon", ';'), ("less", '<'), ("equal", '='),
        ("greater", '>'), ("question", '?'), ("at", '@'),
        ("bracketleft", '['), ("backslash", '\\'), ("bracketright", ']'),
        ("asciicircum", '^'), ("underscore", '_'), ("grave", '`'),
        ("braceleft", '{'), ("bar", '|'), ("braceright", '}'), ("asciitilde", '~'),
        ("periodcentered", '\u{b7}'), ("bullet", '\u{2022}'), ("endash", '\u{2013}'),
        ("emdash", '\u{2014}'), ("quotedblleft", '\u{201c}'),
        ("quotedblright", '\u{201d}'), ("quoteleft", '\u{2018}'),
        ("quoteright", '\u{2019}'), ("ellipsis", '\u{2026}'),
    ];
    NAMES.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// Supersampling factor for glyph rasterisation
/// (`TOUCHHLE_DS_CG_GLYPH_DENSITY`, default 1). Shared by every glyph-drawing
/// entry point so the knob behaves the same everywhere; Dead Space's localised
/// menu text goes through `draw_glyphs_at_positions`, which used to ignore it.
fn glyph_density() -> f32 {
    std::env::var("TOUCHHLE_DS_CG_GLYPH_DENSITY")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(1.0)
}
/// Parse a font's `cmap` table ourselves and return `glyph id -> character`
/// (the lowest code point that maps to a glyph wins).
///
/// Why not `rusttype::Font::glyph(c).id()`: measured 2026-10-11, the guest font
/// the game loads (EurostileLTStd.ttf) resolves the character '1' incorrectly
/// through rusttype's cmap reader, so the id->char table used by the substitute
/// face lost that entry and drew an unrelated glyph - the user saw
/// `档案 1+` / `05:04:33` rendered as `档案 ₄+` / `5: 4: 33` (screenshots in
/// `vm/font_ovr_slots_s.jpg` vs `vm/font_noovr_slots_s.jpg`). A direct parse of
/// the table also covers characters outside the hard-coded ranges the old code
/// scanned.
fn cmap_gid_to_char(data: &[u8]) -> Option<std::collections::HashMap<u16, char>> {
    fn u16be(d: &[u8], o: usize) -> Option<u16> {
        Some(u16::from_be_bytes([*d.get(o)?, *d.get(o + 1)?]))
    }
    fn u32be(d: &[u8], o: usize) -> Option<u32> {
        Some(u32::from_be_bytes([
            *d.get(o)?,
            *d.get(o + 1)?,
            *d.get(o + 2)?,
            *d.get(o + 3)?,
        ]))
    }
    let num_tables = u16be(data, 4)? as usize;
    let mut cmap_off = None;
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        if data.get(rec..rec + 4)? == b"cmap" {
            cmap_off = Some(u32be(data, rec + 8)? as usize);
            break;
        }
    }
    let cmap = cmap_off?;
    let n = u16be(data, cmap + 2)? as usize;
    // Prefer a Unicode subtable: (3,10)/(0,4) format 12, then (3,1) format 4.
    let mut best: Option<(usize, u16)> = None;
    let mut score = |pid: u16, eid: u16, fmt: u16| -> i32 {
        match (pid, eid, fmt) {
            (3, 10, 12) | (0, 4, 12) => 3,
            (3, 1, 4) => 2,
            (0, _, 4) | (0, _, 12) => 1,
            _ => 0,
        }
    };
    let mut best_score = 0;
    for i in 0..n {
        let rec = cmap + 4 + i * 8;
        let pid = u16be(data, rec)?;
        let eid = u16be(data, rec + 2)?;
        let off = u32be(data, rec + 4)? as usize;
        let sub = cmap + off;
        let fmt = u16be(data, sub)?;
        let s = score(pid, eid, fmt);
        if s > best_score {
            best_score = s;
            best = Some((sub, fmt));
        }
    }
    let (sub, fmt) = best?;
    let mut map: std::collections::HashMap<u16, char> = std::collections::HashMap::new();
    let mut push = |gid: u32, cp: u32| {
        if gid == 0 || map.len() > 200_000 {
            return;
        }
        if let Some(c) = char::from_u32(cp) {
            map.entry(gid as u16).or_insert(c);
        }
    };
    if fmt == 4 {
        let seg_x2 = u16be(data, sub + 6)? as usize;
        let seg = seg_x2 / 2;
        let end = sub + 14;
        let start = end + seg_x2 + 2;
        let delta = start + seg_x2;
        let range = delta + seg_x2;
        for i in 0..seg {
            let e = u16be(data, end + i * 2)? as u32;
            let s = u16be(data, start + i * 2)? as u32;
            let d = u16be(data, delta + i * 2)? as i32;
            let ro = u16be(data, range + i * 2)? as usize;
            if s > e {
                continue;
            }
            for cp in s..=e {
                let gid = if ro == 0 {
                    ((cp as i32 + d) & 0xFFFF) as u32
                } else {
                    let addr = range + i * 2 + ro + (cp - s) as usize * 2;
                    let g = match u16be(data, addr) {
                        Some(g) => g as u32,
                        None => continue,
                    };
                    if g == 0 {
                        continue;
                    }
                    ((g as i32 + d) & 0xFFFF) as u32
                };
                push(gid, cp);
            }
        }
    } else if fmt == 12 {
        let groups = u32be(data, sub + 12)? as usize;
        for i in 0..groups {
            let o = sub + 16 + i * 12;
            let s = u32be(data, o)?;
            let e = u32be(data, o + 4)?;
            let g = u32be(data, o + 8)?;
            if s > e || e - s > 0x10000 {
                continue;
            }
            for cp in s..=e {
                push(g + (cp - s), cp);
            }
        }
    } else {
        return None;
    }
    Some(map)
}

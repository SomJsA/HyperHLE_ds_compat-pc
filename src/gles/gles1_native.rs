/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0.
 * If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Passthrough for a native OpenGL ES 1.1 driver.

#[path = "ds_blood_size.rs"]
mod ds_blood_size;
#[path = "ds_opening_fog.rs"]
mod ds_opening_fog;

use super::gles11_raw as gles11;
use super::gles11_raw::types::*;
use super::gles_generic::GLES;
use super::util::{fixed_to_float, try_decode_pvrtc, PalettedTextureFormat};
use super::GLESContext;
use crate::window::{GLContext, GLVersion, Window};
use std::ffi::CStr;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};

static NATIVE_DEPTH_TEST_ENABLED: AtomicBool = AtomicBool::new(false);
static NATIVE_DEPTH_FUNC_LESS: AtomicBool = AtomicBool::new(true);
static NATIVE_FOG_ENABLED: AtomicBool = AtomicBool::new(false);

const VERTEX_ARRAY_SIZE: GLenum = 0x807a;
const DEFAULT_DEPTH_LEQUAL_KEYS: &[u32] = &[
    0x25a0_8a82,
    0xcee4_3546,
    0x3649_7762,
    0x1d5c_ccb8,
    0x8c12_a704,
    0x8712_7d0f,
    0xe70e_fc97,
    0x2131_afd5,
    0x18c9_49fb,
    0xf4d1_78fa,
];
const VERTEX_ARRAY_TYPE: GLenum = 0x807b;
const VERTEX_ARRAY_STRIDE: GLenum = 0x807c;
const VERTEX_ARRAY_POINTER: GLenum = 0x808e;

pub struct GLES1NativeContext {
    gl_ctx: GLContext,
    is_loaded: bool,
    /// Whether the underlying OpenGL ES 1.1 driver advertises
    /// `GL_IMG_texture_compression_pvrtc`. Apps shipped for iPhone OS use
    /// PVRTC textures pervasively (Apple's recommended compression format on
    /// PowerVR-based devices), so when the host driver lacks PVRTC we must
    /// software-decode the payload and upload it as plain RGBA — otherwise
    /// every PVRTC texture silently fails with `GL_INVALID_ENUM` and the
    /// app renders as black silhouettes (see Temple Run 1.0 on
    /// ARM Mali / Qualcomm Adreno, neither of which advertises this
    /// extension on their ES 1.1 surface).
    pvrtc_native: bool,
    /// Whether `pvrtc_native` has been populated yet. The check is deferred
    /// to the first `make_current` because we need a current GL context to
    /// query `GL_EXTENSIONS`.
    pvrtc_native_checked: bool,
    opening_fog: ds_opening_fog::OpeningFog,
}

impl GLESContext for GLES1NativeContext {
    fn description() -> &'static str {
        "Native OpenGL ES 1.1"
    }

    fn new(window: &mut Window) -> Result<Self, String> {
        Ok(Self {
            gl_ctx: window.create_gl_context(GLVersion::GLES11)?,
            is_loaded: false,
            pvrtc_native: false,
            pvrtc_native_checked: false,
            opening_fog: ds_opening_fog::OpeningFog::default(),
        })
    }

    fn make_current<'gl_ctx, 'win: 'gl_ctx>(
        &'gl_ctx mut self,
        window: &'win mut Window,
    ) -> Box<dyn GLES + 'gl_ctx> {
        if self.gl_ctx.is_current() && self.is_loaded {
            return Box::new(GLES1Native {
                _gl_lifetime: PhantomData,
                pending_synthetic_error: std::cell::Cell::new(gles11::NO_ERROR),
                pvrtc_native: self.pvrtc_native,
                opening_fog: &mut self.opening_fog,
            });
        }

        unsafe {
            window.make_gl_context_current(&self.gl_ctx);
        }
        gles11::load_with(|s| window.gl_get_proc_address(s));
        self.is_loaded = true;
        if !self.pvrtc_native_checked {
            self.pvrtc_native = unsafe { detect_pvrtc_support() };
            self.pvrtc_native_checked = true;
            log!(
                "GLES1Native: GL_IMG_texture_compression_pvrtc {} (PVRTC textures will \
                 be {} on this driver)",
                if self.pvrtc_native {
                    "advertised by host driver"
                } else {
                    "NOT advertised by host driver"
                },
                if self.pvrtc_native {
                    "uploaded directly"
                } else {
                    "software-decoded to RGBA before upload"
                },
            );
        }
        Box::new(GLES1Native {
            _gl_lifetime: PhantomData,
            pending_synthetic_error: std::cell::Cell::new(gles11::NO_ERROR),
            pvrtc_native: self.pvrtc_native,
            opening_fog: &mut self.opening_fog,
        })
    }

    unsafe fn make_current_unchecked_for_window<'gl_ctx>(
        &'gl_ctx mut self,
        make_current_fn: &mut dyn FnMut(&GLContext),
        loader_fn: &mut dyn FnMut(&'static str) -> *const std::ffi::c_void,
    ) -> Box<dyn GLES + 'gl_ctx> {
        if self.gl_ctx.is_current() && self.is_loaded {
            return Box::new(GLES1Native {
                _gl_lifetime: PhantomData,
                pending_synthetic_error: std::cell::Cell::new(gles11::NO_ERROR),
                pvrtc_native: self.pvrtc_native,
                opening_fog: &mut self.opening_fog,
            });
        }

        make_current_fn(&self.gl_ctx);
        gles11::load_with(loader_fn);
        self.is_loaded = true;
        if !self.pvrtc_native_checked {
            self.pvrtc_native = detect_pvrtc_support();
            self.pvrtc_native_checked = true;
        }
        Box::new(GLES1Native {
            _gl_lifetime: PhantomData,
            pending_synthetic_error: std::cell::Cell::new(gles11::NO_ERROR),
            pvrtc_native: self.pvrtc_native,
            opening_fog: &mut self.opening_fog,
        })
    }
}

/// Query `GL_EXTENSIONS` on the currently-bound OpenGL ES 1.1 context and
/// return whether it advertises `GL_IMG_texture_compression_pvrtc`.
///
/// Must be called with a current GL context. Returns `false` on any
/// driver-reported error (NULL string, non-UTF-8 string, missing token);
/// software-decoding PVRTC is the safe-default behaviour.
unsafe fn detect_pvrtc_support() -> bool {
    let raw = gles11::GetString(gles11::EXTENSIONS);
    if raw.is_null() {
        return false;
    }
    let Ok(s) = CStr::from_ptr(raw as *const _).to_str() else {
        return false;
    };
    if s.is_empty() {
        return false;
    }
    s.split(' ')
        .any(|ext| ext == "GL_IMG_texture_compression_pvrtc")
}

fn native_env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
}

unsafe fn native_bound_texture_id() -> GLint {
    let mut current: GLint = 0;
    gles11::GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut current);
    current
}

unsafe fn native_bound_texture_key() -> Option<u32> {
    crate::ds_texid::lookup(native_bound_texture_id() as u32).map(|ident| ident.key)
}

unsafe fn native_texture_selected(id: Option<GLint>, keys: &[u32]) -> bool {
    let current = native_bound_texture_id();
    if id == Some(current) {
        return true;
    }
    if keys.is_empty() {
        return false;
    }
    matches!(
        crate::ds_texid::lookup(current as u32),
        Some(ident) if keys.contains(&ident.key)
    )
}

unsafe fn native_tex_hash_in_spec(spec: &str) -> bool {
    let keys = crate::ds_texid::parse_keys(spec);
    !keys.is_empty() && native_texture_selected(None, &keys)
}

unsafe fn native_projection_is_ortho() -> bool {
    let mut matrix = [0.0f32; 16];
    gles11::GetFloatv(gles11::PROJECTION_MATRIX, matrix.as_mut_ptr());
    matrix[3].abs() < 1e-6
        && matrix[7].abs() < 1e-6
        && matrix[11].abs() < 1e-6
        && (matrix[15] - 1.0).abs() < 1e-6
}

unsafe fn native_opening_projection_active() -> bool {
    let mut matrix = [0.0f32; 16];
    gles11::GetFloatv(gles11::PROJECTION_MATRIX, matrix.as_mut_ptr());
    matrix[10] >= -1.006 && matrix[10] <= -0.999 && matrix[14] >= -0.105 && matrix[14] <= -0.095
}

unsafe fn native_draw_ndc_span(count: GLsizei) -> Option<(f32, f32)> {
    let mut vertex_size: GLint = 0;
    let mut vertex_type: GLint = 0;
    let mut vertex_stride: GLsizei = 0;
    let mut vertex_buffer: GLint = 0;
    gles11::GetIntegerv(VERTEX_ARRAY_SIZE, &mut vertex_size);
    gles11::GetIntegerv(VERTEX_ARRAY_TYPE, &mut vertex_type);
    gles11::GetIntegerv(VERTEX_ARRAY_STRIDE, &mut vertex_stride);
    gles11::GetIntegerv(gles11::VERTEX_ARRAY_BUFFER_BINDING, &mut vertex_buffer);
    let mut vertex_ptr: *mut GLvoid = std::ptr::null_mut();
    gles11::GetPointerv(VERTEX_ARRAY_POINTER, &mut vertex_ptr as *mut _ as *const _);
    if vertex_buffer != 0 || vertex_ptr.is_null() || vertex_size <= 0 || count <= 0 {
        return None;
    }

    let vertex_size = vertex_size.clamp(1, 4) as usize;
    let elem_size = match vertex_type as GLenum {
        gles11::BYTE | gles11::UNSIGNED_BYTE => 1usize,
        gles11::SHORT | gles11::UNSIGNED_SHORT => 2,
        _ => 4,
    };
    let stride = if vertex_stride == 0 {
        vertex_size * elem_size
    } else {
        vertex_stride as usize
    };
    let mut mv = [0.0f32; 16];
    gles11::GetFloatv(gles11::MODELVIEW_MATRIX, mv.as_mut_ptr());
    let mut projection = [0.0f32; 16];
    gles11::GetFloatv(gles11::PROJECTION_MATRIX, projection.as_mut_ptr());

    let n = count.clamp(1, 4096) as usize;
    let mut xmin = f32::MAX;
    let mut xmax = f32::MIN;
    let mut ymin = f32::MAX;
    let mut ymax = f32::MIN;
    for i in 0..n {
        let mut vertex = [0.0f32; 4];
        vertex[3] = 1.0;
        for component in 0..vertex_size {
            let ptr = (vertex_ptr as *const u8).add(i * stride + component * elem_size);
            vertex[component] = match vertex_type as GLenum {
                gles11::FLOAT => (ptr as *const f32).read_unaligned(),
                gles11::FIXED => (ptr as *const i32).read_unaligned() as f32 / 65536.0,
                gles11::UNSIGNED_BYTE => (ptr as *const u8).read_unaligned() as f32,
                gles11::BYTE => (ptr as *const i8).read_unaligned() as f32,
                gles11::UNSIGNED_SHORT => (ptr as *const u16).read_unaligned() as f32,
                gles11::SHORT => (ptr as *const i16).read_unaligned() as f32,
                _ => 0.0,
            };
        }
        let ex = mv[0] * vertex[0] + mv[4] * vertex[1] + mv[8] * vertex[2] + mv[12] * vertex[3];
        let ey = mv[1] * vertex[0] + mv[5] * vertex[1] + mv[9] * vertex[2] + mv[13] * vertex[3];
        let ez = mv[2] * vertex[0] + mv[6] * vertex[1] + mv[10] * vertex[2] + mv[14] * vertex[3];
        let ew = mv[3] * vertex[0] + mv[7] * vertex[1] + mv[11] * vertex[2] + mv[15] * vertex[3];
        let cx = projection[0] * ex + projection[4] * ey + projection[8] * ez + projection[12] * ew;
        let cy = projection[1] * ex + projection[5] * ey + projection[9] * ez + projection[13] * ew;
        let cw =
            projection[3] * ex + projection[7] * ey + projection[11] * ez + projection[15] * ew;
        if cw == 0.0 || !cx.is_finite() || !cy.is_finite() || !cw.is_finite() {
            continue;
        }
        let nx = cx / cw;
        let ny = cy / cw;
        if !nx.is_finite() || !ny.is_finite() {
            continue;
        }
        xmin = xmin.min(nx);
        xmax = xmax.max(nx);
        ymin = ymin.min(ny);
        ymax = ymax.max(ny);
    }
    if xmin > xmax || ymin > ymax {
        None
    } else {
        Some((xmax - xmin, ymax - ymin))
    }
}

#[derive(Copy, Clone)]
struct NativePixelTrace {
    x: GLint,
    y: GLint,
    before: [u8; 4],
}

unsafe fn native_pixel_trace_begin() -> Option<NativePixelTrace> {
    if native_env_flag("TOUCHHLE_DS_PIXEL_TRACE_WAIT_FOR_MARK")
        && !crate::window::DS_TRACE_MARKED.load(Ordering::Relaxed)
    {
        return None;
    }
    let frame = crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed);
    if std::env::var("TOUCHHLE_DS_PIXEL_TRACE_FROM_FRAME")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|from| frame < from)
    {
        return None;
    }
    if std::env::var("TOUCHHLE_DS_PIXEL_TRACE_TO_FRAME")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|to| frame > to)
    {
        return None;
    }
    let spec = std::env::var("TOUCHHLE_DS_PIXEL_TRACE").ok()?;
    let values: Vec<f32> = spec
        .split(',')
        .filter_map(|value| value.trim().parse::<f32>().ok())
        .collect();
    if values.len() != 2 {
        return None;
    }
    let mut viewport = [0 as GLint; 4];
    gles11::GetIntegerv(gles11::VIEWPORT, viewport.as_mut_ptr());
    if viewport[2] <= 0 || viewport[3] <= 0 {
        return None;
    }
    let x = viewport[0] + (viewport[2] as f32 * values[0] / 100.0) as GLint;
    let y = viewport[1] + (viewport[3] as f32 * values[1] / 100.0) as GLint;
    let mut before = [0u8; 4];
    gles11::ReadPixels(
        x,
        y,
        1,
        1,
        gles11::RGBA,
        gles11::UNSIGNED_BYTE,
        before.as_mut_ptr() as *mut GLvoid,
    );
    Some(NativePixelTrace { x, y, before })
}

unsafe fn native_pixel_trace_end(trace: Option<NativePixelTrace>, kind: &str, count: GLsizei) {
    let Some(trace) = trace else {
        return;
    };
    let min_count = std::env::var("TOUCHHLE_DS_PIXEL_TRACE_MIN_COUNT")
        .ok()
        .and_then(|value| value.parse::<GLsizei>().ok())
        .unwrap_or(0);
    if count < min_count {
        return;
    }
    let texture_key = native_bound_texture_key().unwrap_or(0);
    if native_env_flag("TOUCHHLE_DS_PIXEL_TRACE_ONLY_KEYED") && texture_key == 0 {
        return;
    }
    let mut after = [0u8; 4];
    gles11::ReadPixels(
        trace.x,
        trace.y,
        1,
        1,
        gles11::RGBA,
        gles11::UNSIGNED_BYTE,
        after.as_mut_ptr() as *mut GLvoid,
    );
    if !native_env_flag("TOUCHHLE_DS_PIXEL_TRACE_ALL") && trace.before == after {
        return;
    }
    static TRACE_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
    let max_lines = std::env::var("TOUCHHLE_DS_PIXEL_TRACE_MAX")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(400);
    if n >= max_lines {
        return;
    }
    let mut framebuffer: GLint = 0;
    gles11::GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut framebuffer);
    log!(
        "[DSpixelNative] #{} {} count={} key={:08x} tex={} fbo={} point=({},{}) before={:?} after={:?}",
        n,
        kind,
        count,
        texture_key,
        native_bound_texture_id(),
        framebuffer,
        trace.x,
        trace.y,
        trace.before,
        after,
    );
}

unsafe fn native_nofog_begin(count: GLsizei) -> bool {
    let global = native_env_flag("TOUCHHLE_DS_NO_FOG");
    let texture_match = std::env::var("TOUCHHLE_DS_NO_FOG_HASH")
        .ok()
        .is_some_and(|spec| native_tex_hash_in_spec(&spec));
    let hash_count_match = std::env::var("TOUCHHLE_DS_NO_FOG_HASH_COUNT")
        .ok()
        .is_some_and(|spec| {
            spec.split(';').any(|item| {
                let mut parts = item.splitn(2, ':');
                let key = parts
                    .next()
                    .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
                let wanted_count = parts
                    .next()
                    .and_then(|value| value.trim().parse::<GLsizei>().ok());
                key.is_some_and(|key| native_texture_selected(None, &[key]))
                    && wanted_count == Some(count)
            })
        });
    let screen_layer = native_env_flag("TOUCHHLE_DS_NO_FOG_SCREEN")
        && native_draw_ndc_span(count)
            .map(|(width, height)| width >= 1.5 && height >= 1.5)
            .unwrap_or(false);
    let blend_layer =
        native_env_flag("TOUCHHLE_DS_NO_FOG_BLEND") && gles11::IsEnabled(gles11::BLEND) != 0;
    let degenerate_linear_fog = gles11::IsEnabled(gles11::FOG) != 0 && {
        let mut mode: GLint = 0;
        let mut start: GLfloat = 0.0;
        let mut end: GLfloat = 0.0;
        gles11::GetIntegerv(gles11::FOG_MODE, &mut mode);
        gles11::GetFloatv(gles11::FOG_START, &mut start);
        gles11::GetFloatv(gles11::FOG_END, &mut end);
        mode == gles11::LINEAR as GLint && (end <= 0.0 || end <= start)
    };
    if !global
        && !texture_match
        && !hash_count_match
        && !screen_layer
        && !blend_layer
        && !degenerate_linear_fog
    {
        return false;
    }
    let was_on = gles11::IsEnabled(gles11::FOG) != 0;
    if was_on {
        gles11::Disable(gles11::FOG);
        log_once!("Android native: applied scoped PC fog repair");
    }
    was_on
}

unsafe fn native_nofog_ortho_begin() -> bool {
    if !native_env_flag("TOUCHHLE_DS_NO_FOG_ORTHO") || !native_projection_is_ortho() {
        return false;
    }
    let was_on = gles11::IsEnabled(gles11::FOG) != 0;
    if was_on {
        gles11::Disable(gles11::FOG);
        log_once!("Android native: disabled fog for orthographic PC overlay");
    }
    was_on
}

unsafe fn native_nofog_end(was_on: bool) {
    if was_on {
        gles11::Enable(gles11::FOG);
    }
}

unsafe fn native_opening_fog_begin(state: &mut ds_opening_fog::OpeningFog) -> Option<(f32, f32)> {
    if std::env::var("TOUCHHLE_DS_OPENING_FOG_FIX").as_deref() == Ok("0") {
        return None;
    }
    let enabled = gles11::IsEnabled(gles11::FOG) != 0;
    let mut mode: GLint = 0;
    let mut color = [0.0f32; 4];
    let mut projection = [0.0f32; 16];
    let mut start: GLfloat = 0.0;
    let mut end: GLfloat = 0.0;
    gles11::GetIntegerv(gles11::FOG_MODE, &mut mode);
    gles11::GetFloatv(gles11::FOG_COLOR, color.as_mut_ptr());
    gles11::GetFloatv(gles11::PROJECTION_MATRIX, projection.as_mut_ptr());
    gles11::GetFloatv(gles11::FOG_START, &mut start);
    gles11::GetFloatv(gles11::FOG_END, &mut end);
    if end > 0.0
        && end < 0.25
        && (start - end * 0.4).abs() < 0.002
        && (projection[11] + 1.0).abs() < 0.001
    {
        static CHECKED: AtomicBool = AtomicBool::new(false);
        if !CHECKED.swap(true, Ordering::Relaxed) {
            log!(
                "[DSopeningFogCheck] enabled={} mode={} color={:?} \
                 near_coefficient={} range={}..{} state={:?}",
                enabled,
                mode,
                color,
                projection[14],
                start,
                end,
                state
            );
        }
    }
    if mode != gles11::LINEAR as GLint
        || color[..3].iter().any(|component| component.abs() > 0.001)
        || (projection[11] + 1.0).abs() > 0.001
        || projection[15].abs() > 0.001
        || !(-0.105..=-0.095).contains(&projection[14])
    {
        return None;
    }
    let frame = crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed);
    let (fixed_start, fixed_end) = state.range(frame, start, end)?;
    if !enabled {
        return None;
    }
    gles11::Fogf(gles11::FOG_START, fixed_start);
    gles11::Fogf(gles11::FOG_END, fixed_end);
    log_once!("Android native: preserving PC opening fog range 10..25");
    Some((start, end))
}

unsafe fn native_opening_fog_end(original: Option<(f32, f32)>) {
    if let Some((start, end)) = original {
        gles11::Fogf(gles11::FOG_START, start);
        gles11::Fogf(gles11::FOG_END, end);
    }
}

struct NativeTextureFilterBackup {
    min_filter: GLint,
    mag_filter: GLint,
}

unsafe fn native_texfilter_begin(count: GLsizei) -> Option<NativeTextureFilterBackup> {
    let linear_keys = std::env::var("TOUCHHLE_DS_TEXFILTER_LINEAR_HASH")
        .map(|spec| crate::ds_texid::parse_keys(&spec))
        .unwrap_or_default();
    if linear_keys.is_empty() {
        return None;
    }
    let max_count = std::env::var("TOUCHHLE_DS_TEXFILTER_LINEAR_MAX_COUNT")
        .ok()
        .and_then(|value| value.parse::<GLsizei>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(GLsizei::MAX);
    if count <= 0 || count > max_count || !native_texture_selected(None, &linear_keys) {
        return None;
    }

    let mut min_filter: GLint = 0;
    let mut mag_filter: GLint = 0;
    gles11::GetTexParameteriv(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_MIN_FILTER,
        &mut min_filter,
    );
    gles11::GetTexParameteriv(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_MAG_FILTER,
        &mut mag_filter,
    );
    gles11::TexParameteri(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_MIN_FILTER,
        gles11::LINEAR as GLint,
    );
    gles11::TexParameteri(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_MAG_FILTER,
        gles11::LINEAR as GLint,
    );
    log_once!("Android native: applied PC poster LINEAR/LINEAR filter");
    Some(NativeTextureFilterBackup {
        min_filter,
        mag_filter,
    })
}

unsafe fn native_texfilter_end(backup: Option<NativeTextureFilterBackup>) {
    if let Some(backup) = backup {
        gles11::TexParameteri(
            gles11::TEXTURE_2D,
            gles11::TEXTURE_MIN_FILTER,
            backup.min_filter,
        );
        gles11::TexParameteri(
            gles11::TEXTURE_2D,
            gles11::TEXTURE_MAG_FILTER,
            backup.mag_filter,
        );
    }
}

struct NativeCullBackup {
    was_enabled: bool,
}

unsafe fn native_texmat_identity_begin(count: GLsizei) -> Option<GLint> {
    let spec = std::env::var("TOUCHHLE_DS_TEXMAT_IDENTITY_HASH_COUNT").ok()?;
    if !native_opening_projection_active() {
        return None;
    }
    for item in spec.split(';') {
        let mut parts = item.splitn(2, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok());
        if key.is_some_and(|key| native_texture_selected(None, &[key]))
            && wanted_count == Some(count)
        {
            let mut old_mode: GLint = 0;
            gles11::GetIntegerv(gles11::MATRIX_MODE, &mut old_mode);
            gles11::MatrixMode(gles11::TEXTURE);
            gles11::PushMatrix();
            gles11::LoadIdentity();
            gles11::MatrixMode(old_mode as GLenum);
            log_once!("Android native: restored atlas UV matrix for opening asteroid");
            return Some(old_mode);
        }
    }
    None
}

unsafe fn native_texmat_identity_end(backup: Option<GLint>) {
    if let Some(old_mode) = backup {
        gles11::MatrixMode(gles11::TEXTURE);
        gles11::PopMatrix();
        gles11::MatrixMode(old_mode as GLenum);
    }
}

#[derive(Copy, Clone)]
struct NativeTexFlatBackup {
    texture_2d: bool,
    lighting: bool,
    blend: bool,
    alpha_test: bool,
    color: [GLfloat; 4],
}

unsafe fn native_texflat_begin() -> Option<NativeTexFlatBackup> {
    let spec = std::env::var("TOUCHHLE_DS_TEXFLAT_HASH").ok()?;
    if !native_tex_hash_in_spec(&spec) {
        return None;
    }
    let mut color = [1.0f32; 4];
    gles11::GetFloatv(gles11::CURRENT_COLOR, color.as_mut_ptr());
    let backup = NativeTexFlatBackup {
        texture_2d: gles11::IsEnabled(gles11::TEXTURE_2D) != 0,
        lighting: gles11::IsEnabled(gles11::LIGHTING) != 0,
        blend: gles11::IsEnabled(gles11::BLEND) != 0,
        alpha_test: gles11::IsEnabled(gles11::ALPHA_TEST) != 0,
        color,
    };
    gles11::Disable(gles11::TEXTURE_2D);
    if backup.lighting {
        gles11::Disable(gles11::LIGHTING);
    }
    if backup.blend {
        gles11::Disable(gles11::BLEND);
    }
    if backup.alpha_test {
        gles11::Disable(gles11::ALPHA_TEST);
    }
    gles11::Color4f(1.0, 1.0, 1.0, 1.0);
    log_once!("Android native: selected texture rendered flat for diagnosis");
    Some(backup)
}

unsafe fn native_texflat_end(backup: Option<NativeTexFlatBackup>) {
    if let Some(backup) = backup {
        gles11::Color4f(
            backup.color[0],
            backup.color[1],
            backup.color[2],
            backup.color[3],
        );
        if backup.texture_2d {
            gles11::Enable(gles11::TEXTURE_2D);
        }
        if backup.lighting {
            gles11::Enable(gles11::LIGHTING);
        }
        if backup.blend {
            gles11::Enable(gles11::BLEND);
        }
        if backup.alpha_test {
            gles11::Enable(gles11::ALPHA_TEST);
        }
    }
}

unsafe fn native_no_cull_begin(count: GLsizei) -> Option<NativeCullBackup> {
    let spec = std::env::var("TOUCHHLE_DS_NO_CULL_HASH_COUNT")
        .unwrap_or_else(|_| "02d3486c:405".to_owned());
    if spec.trim() == "0" {
        return None;
    }
    for item in spec.split(';') {
        let mut parts = item.splitn(2, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok());
        if key.is_some_and(|key| native_texture_selected(None, &[key]))
            && wanted_count == Some(count)
        {
            let was_enabled = gles11::IsEnabled(gles11::CULL_FACE) != 0;
            if was_enabled {
                gles11::Disable(gles11::CULL_FACE);
            }
            log_once!("Android native: disabled culling for PC station-hull batch");
            return Some(NativeCullBackup { was_enabled });
        }
    }
    None
}

unsafe fn native_no_cull_end(backup: Option<NativeCullBackup>) {
    if backup.is_some_and(|backup| backup.was_enabled) {
        gles11::Enable(gles11::CULL_FACE);
    }
}

struct NativeDepthBackup {
    was_test: bool,
    old_func: GLint,
    old_mask: GLboolean,
}

unsafe fn native_depth_backup_and_override(
    disable_test: bool,
    depth_func: Option<GLenum>,
    depth_write: Option<GLboolean>,
) -> NativeDepthBackup {
    let was_test = gles11::IsEnabled(gles11::DEPTH_TEST) != 0;
    let mut old_func: GLint = 0;
    let mut old_mask: GLboolean = 0;
    gles11::GetIntegerv(gles11::DEPTH_FUNC, &mut old_func);
    gles11::GetBooleanv(gles11::DEPTH_WRITEMASK, &mut old_mask);
    if disable_test {
        gles11::Disable(gles11::DEPTH_TEST);
    }
    if let Some(func) = depth_func {
        gles11::DepthFunc(func);
    }
    if let Some(mask) = depth_write {
        gles11::DepthMask(mask);
    }
    NativeDepthBackup {
        was_test,
        old_func,
        old_mask,
    }
}

unsafe fn native_depth_restore(backup: NativeDepthBackup) {
    gles11::DepthMask(backup.old_mask);
    gles11::DepthFunc(backup.old_func as GLenum);
    if backup.was_test {
        gles11::Enable(gles11::DEPTH_TEST);
    } else {
        gles11::Disable(gles11::DEPTH_TEST);
    }
}

unsafe fn native_texnodepth_begin(count: GLsizei) -> Option<NativeDepthBackup> {
    let lequal_mode = std::env::var("TOUCHHLE_DS_DEPTH_LEQUAL").ok();
    let global_lequal = lequal_mode.as_deref() == Some("1");
    let hash_spec = std::env::var("TOUCHHLE_DS_DEPTH_LEQUAL_HASH").ok();
    let z_spec = std::env::var_os("TOUCHHLE_DS_DEPTH_LEQUAL_Z").is_some();
    let legacy_requested = lequal_mode.is_some() || hash_spec.is_some() || z_spec;
    let depth_repair_enabled = lequal_mode.as_deref() != Some("0") && legacy_requested;
    let min_stack_span = std::env::var("TOUCHHLE_DS_DEPTH_LEQUAL_MIN_SPAN")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| *value > 0.0)
        .unwrap_or(1.5);
    let screen_sized_draw = native_draw_ndc_span(count)
        .map(|(width, height)| width >= min_stack_span && height >= min_stack_span)
        .unwrap_or(false);
    let scene_rock_batch = count == 600 && native_texture_selected(None, &[0xd1fe_b687]);
    let scoped_repair = !scene_rock_batch
        && match hash_spec.as_deref() {
            Some(spec) => screen_sized_draw && native_tex_hash_in_spec(spec),
            None => screen_sized_draw && native_texture_selected(None, DEFAULT_DEPTH_LEQUAL_KEYS),
        };
    if depth_repair_enabled
        && (global_lequal || scoped_repair)
        && gles11::IsEnabled(gles11::DEPTH_TEST) != 0
    {
        let mut current_func: GLint = 0;
        gles11::GetIntegerv(gles11::DEPTH_FUNC, &mut current_func);
        if current_func == gles11::LESS as GLint {
            let depth_write =
                if global_lequal || !native_env_flag("TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE") {
                    None
                } else {
                    Some(gles11::FALSE)
                };
            let backup = native_depth_backup_and_override(false, Some(gles11::LEQUAL), depth_write);
            log_once!("Android native: scoped PC background GL_LEQUAL repair");
            return Some(backup);
        }
    }

    let count_spec = std::env::var("TOUCHHLE_DS_TEXNODEPTH_HASH_COUNT").ok();
    if let Some(spec) = count_spec {
        let opening_projection = native_opening_projection_active();
        let mut matched = false;
        for item in spec.split(',') {
            let mut parts = item.splitn(2, ':');
            let key = parts
                .next()
                .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
            let wanted_count = parts
                .next()
                .and_then(|value| value.trim().parse::<GLsizei>().ok());
            if let (Some(key), Some(wanted_count)) = (key, wanted_count) {
                if opening_projection && wanted_count == 6 {
                    continue;
                }
                if wanted_count == count && native_texture_selected(None, &[key]) {
                    let blend_enabled = gles11::IsEnabled(gles11::BLEND) != 0;
                    let mut blend_src: GLint = 0;
                    let mut blend_dst: GLint = 0;
                    gles11::GetIntegerv(gles11::BLEND_SRC, &mut blend_src);
                    gles11::GetIntegerv(gles11::BLEND_DST, &mut blend_dst);
                    if crate::ds_texid::overlay_effect_keeps_depth(
                        key,
                        count,
                        blend_enabled,
                        blend_src as u32,
                        blend_dst as u32,
                    ) {
                        if blend_enabled {
                            log_once!(
                                "[DSshaftDepth] additive overlay effect retains guest depth state"
                            );
                        } else {
                            log_once!(
                                "[DSpropDepth] opaque overlay-atlas prop retains guest depth test/write"
                            );
                        }
                        continue;
                    }
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            log_once!("Android native: scoped PC cover/wall depth override");
            return Some(native_depth_backup_and_override(
                true,
                None,
                Some(gles11::FALSE),
            ));
        }
    }

    let id = std::env::var("TOUCHHLE_DS_TEXNODEPTH")
        .ok()
        .and_then(|value| value.parse::<GLint>().ok());
    let keys = std::env::var("TOUCHHLE_DS_TEXNODEPTH_HASH")
        .map(|spec| crate::ds_texid::parse_keys(&spec))
        .unwrap_or_default();
    if (id.is_some() || !keys.is_empty()) && native_texture_selected(id, &keys) {
        log_once!("Android native: selected texture depth override");
        return Some(native_depth_backup_and_override(
            true,
            None,
            Some(gles11::FALSE),
        ));
    }
    None
}

unsafe fn native_texnodepth_end(backup: Option<NativeDepthBackup>) {
    if let Some(backup) = backup {
        native_depth_restore(backup);
    }
}

const BLOOD_TEXTURE_KEYS: &[u32] = &[0x00b9_b876, 0xfd93_7e94, 0xb3ca_1a51];

unsafe fn native_blood_depth_begin(mode: GLenum, count: GLsizei) -> bool {
    if mode != gles11::TRIANGLES
        || count <= 0
        || count % 6 != 0
        || std::env::var("TOUCHHLE_DS_BLOOD_DEPTH_FIX").as_deref() == Ok("0")
        || !native_texture_selected(None, BLOOD_TEXTURE_KEYS)
        || gles11::IsEnabled(gles11::TEXTURE_2D) == 0
        || gles11::IsEnabled(gles11::BLEND) == 0
        || gles11::IsEnabled(gles11::DEPTH_TEST) != 0
    {
        return false;
    }
    let mut projection = [0.0f32; 16];
    gles11::GetFloatv(gles11::PROJECTION_MATRIX, projection.as_mut_ptr());
    if (projection[11] + 1.0).abs() > 0.001 || projection[15].abs() > 0.001 {
        return false;
    }
    let mut source: GLint = 0;
    let mut destination: GLint = 0;
    let mut depth_function: GLint = 0;
    let mut depth_write: GLboolean = 0;
    gles11::GetIntegerv(gles11::BLEND_SRC, &mut source);
    gles11::GetIntegerv(gles11::BLEND_DST, &mut destination);
    gles11::GetIntegerv(gles11::DEPTH_FUNC, &mut depth_function);
    gles11::GetBooleanv(gles11::DEPTH_WRITEMASK, &mut depth_write);
    if source != gles11::SRC_ALPHA as GLint
        || destination != gles11::ONE_MINUS_SRC_ALPHA as GLint
        || depth_function != gles11::LESS as GLint
        || depth_write != 0
    {
        return false;
    }
    gles11::Enable(gles11::DEPTH_TEST);
    log_once!("Android native: enabling scene depth for PC hit-blood particles");
    true
}

unsafe fn native_blood_depth_end(active: bool) {
    if active {
        gles11::Disable(gles11::DEPTH_TEST);
    }
}

unsafe fn native_draw_scaled_blood_elements(
    count: GLsizei,
    type_: GLenum,
    indices: *const GLvoid,
) -> bool {
    let scale = std::env::var("TOUCHHLE_DS_BLOOD_SIZE")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.0 && *value <= 1.0)
        .unwrap_or(0.5);
    if scale == 1.0 {
        return false;
    }
    let element_size = match type_ {
        gles11::UNSIGNED_BYTE => 1,
        gles11::UNSIGNED_SHORT => 2,
        _ => return false,
    };
    let mut element_buffer: GLint = 0;
    gles11::GetIntegerv(gles11::ELEMENT_ARRAY_BUFFER_BINDING, &mut element_buffer);
    if element_buffer != 0 || indices.is_null() {
        log_once!("Android native: blood size fix kept original VBO/EBO draw");
        return false;
    }
    let byte_count = count as usize * element_size;
    let bytes = std::slice::from_raw_parts(indices.cast::<u8>(), byte_count);
    let order: Vec<usize> = bytes
        .chunks_exact(element_size)
        .map(|index| {
            if element_size == 1 {
                index[0] as usize
            } else {
                u16::from_ne_bytes([index[0], index[1]]) as usize
            }
        })
        .collect();
    let vertex_count = order.iter().copied().max().unwrap_or(0) + 1;

    let mut size: GLint = 0;
    let mut vertex_type: GLint = 0;
    let mut stride: GLsizei = 0;
    let mut vertex_buffer: GLint = 0;
    let mut pointer: *mut GLvoid = std::ptr::null_mut();
    gles11::GetIntegerv(VERTEX_ARRAY_SIZE, &mut size);
    gles11::GetIntegerv(VERTEX_ARRAY_TYPE, &mut vertex_type);
    gles11::GetIntegerv(VERTEX_ARRAY_STRIDE, &mut stride);
    gles11::GetIntegerv(gles11::VERTEX_ARRAY_BUFFER_BINDING, &mut vertex_buffer);
    gles11::GetPointerv(VERTEX_ARRAY_POINTER, &mut pointer as *mut _ as *const _);
    if vertex_buffer != 0
        || pointer.is_null()
        || !(3..=4).contains(&size)
        || vertex_type != gles11::FLOAT as GLint
        || stride < 0
    {
        log_once!("Android native: blood size fix kept unsupported vertex format");
        return false;
    }

    let vertex_bytes = size as usize * std::mem::size_of::<f32>();
    let effective_stride = if stride == 0 {
        vertex_bytes
    } else {
        stride as usize
    };
    let base = pointer.cast::<u8>();
    let mut positions = vec![[0.0f32; 3]; vertex_count];
    let mut vertices = vec![[0.0f32, 0.0, 0.0, 1.0]; vertex_count];
    for &index in &order {
        if size == 4 {
            vertices[index][3] =
                (base.add(index * effective_stride + 12) as *const f32).read_unaligned();
        }
        let weight = vertices[index][3];
        if !weight.is_finite() || weight == 0.0 {
            return false;
        }
        for axis in 0..3 {
            positions[index][axis] = (base.add(index * effective_stride + axis * 4) as *const f32)
                .read_unaligned()
                / weight;
        }
    }
    if !ds_blood_size::scale_quads(&mut positions, &order, scale) {
        log_once!("Android native: blood size fix kept non-quad topology");
        return false;
    }
    for index in 0..vertex_count {
        for axis in 0..3 {
            vertices[index][axis] = positions[index][axis] * vertices[index][3];
        }
    }
    gles11::VertexPointer(4, gles11::FLOAT, 0, vertices.as_ptr().cast());
    gles11::DrawElements(gles11::TRIANGLES, count, type_, indices);
    gles11::VertexPointer(size, vertex_type as GLenum, stride, pointer);
    log_once!("Android native: applied PC hit-blood half-size quads");
    true
}

/// Fix the opening camera where it is defined: at `glFrustum*`, not at an
/// arbitrary presented-frame number. Different menu/loading speeds can move
/// the prologue far outside any hard-coded frame window.
fn native_adjust_frustum_far(near: f32, far: f32) -> f32 {
    if std::env::var_os("TOUCHHLE_DS_NO_PROJ_Z_FIX").is_some() {
        return far;
    }
    let setting = std::env::var("TOUCHHLE_DS_PROJ_Z_INFINITE")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase());
    if setting.as_deref() == Some("0") || near <= 0.0 || far <= near {
        return far;
    }
    let opening_projection = near >= 0.01 && near <= 0.1 && far >= 20.0 && far <= 40.0;
    if setting.as_deref() == Some("1") || opening_projection {
        // f32(far=1e9, near=0.05) rounds m[10] to -1, i.e. the infinite-far
        // projection, while keeping m[14] unchanged.
        1.0e9
    } else {
        far
    }
}

/// Optional per-draw depth-row scaling remains available for diagnostics.
/// The actual prologue repair is done once in `Frustumf`/`Frustumx`.
unsafe fn native_proj_z_fix_begin() -> bool {
    if std::env::var_os("TOUCHHLE_DS_NO_PROJ_Z_FIX").is_some() {
        return false;
    }

    let infinite_setting = std::env::var("TOUCHHLE_DS_PROJ_Z_INFINITE").ok();
    let scale_setting = std::env::var("TOUCHHLE_DS_PROJ_Z_SCALE")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| *value > 0.0 && *value <= 1.0);

    let mut matrix = [0.0f32; 16];
    gles11::GetFloatv(gles11::PROJECTION_MATRIX, matrix.as_mut_ptr());

    // The game sometimes uploads its finite-far projection with LoadMatrixf
    // instead of defining it through glFrustum. Match PC's draw-time guard so
    // that path receives the same infinite-far correction.
    let opening_projection = matrix[10] >= -1.006
        && matrix[10] <= -1.001
        && matrix[14] >= -0.105
        && matrix[14] <= -0.095;
    let infinite_far = infinite_setting.as_deref() == Some("1")
        || (infinite_setting.is_none() && opening_projection);
    if !infinite_far && scale_setting.is_none() {
        return false;
    }

    if infinite_far {
        if matrix[10] < -1.0 {
            log_once!("Android native: corrected draw-time finite-far projection");
            matrix[10] = -1.0;
        }
    } else {
        let scale = scale_setting.unwrap_or(1.0);
        matrix[2] *= scale;
        matrix[6] *= scale;
        matrix[10] *= scale;
        matrix[14] *= scale;
    }

    let mut old_mode: GLint = 0;
    gles11::GetIntegerv(gles11::MATRIX_MODE, &mut old_mode);
    gles11::MatrixMode(gles11::PROJECTION);
    gles11::PushMatrix();
    gles11::LoadMatrixf(matrix.as_ptr());
    gles11::MatrixMode(old_mode as GLenum);
    true
}

unsafe fn native_proj_z_fix_end(active: bool) {
    if !active {
        return;
    }
    let mut old_mode: GLint = 0;
    gles11::GetIntegerv(gles11::MATRIX_MODE, &mut old_mode);
    gles11::MatrixMode(gles11::PROJECTION);
    gles11::PopMatrix();
    gles11::MatrixMode(old_mode as GLenum);
}

pub struct GLES1Native<'gl_ctx> {
    _gl_lifetime: PhantomData<&'gl_ctx ()>,
    /// Synthetic error queue for OpenGL ES 2.0 entry points that are not
    /// supported on a native ES 1.1 driver. When a guest app calls a
    /// shader-pipeline entry point (e.g. `glCreateShader`, `glUseProgram`,
    /// `glUniform1f`) on an ES 1.1 context, the ES 2.0 specification says the
    /// implementation must report `GL_INVALID_OPERATION`. The native ES 1.1
    /// driver wouldn't know about these calls at all — it never sees them —
    /// so [`GLES1Native::GetError`] would return `GL_NO_ERROR` and the guest
    /// would silently miss the failure. Instead we maintain a one-slot
    /// pending-error queue here; the ES 2.0 overrides below store
    /// `GL_INVALID_OPERATION` into it, and our `GetError` returns it (then
    /// clears it) before falling back to the underlying driver's error queue.
    pending_synthetic_error: std::cell::Cell<GLenum>,
    /// Mirror of [`GLES1NativeContext::pvrtc_native`]; copied at make_current
    /// time so the per-call `CompressedTexImage2D` path doesn't have to
    /// re-query `GL_EXTENSIONS`.
    pvrtc_native: bool,
    opening_fog: &'gl_ctx mut ds_opening_fog::OpeningFog,
}

impl GLES for GLES1Native<'_> {
    fn is_native_es1(&self) -> bool {
        true
    }
    unsafe fn driver_description(&self) -> String {
        let version = CStr::from_ptr(gles11::GetString(gles11::VERSION) as *const _);
        let vendor = CStr::from_ptr(gles11::GetString(gles11::VENDOR) as *const _);
        let renderer = CStr::from_ptr(gles11::GetString(gles11::RENDERER) as *const _);
        format!(
            "{} / {} / {}",
            version.to_string_lossy(),
            vendor.to_string_lossy(),
            renderer.to_string_lossy()
        )
    }

    // Generic state manipulation
    unsafe fn GetError(&mut self) -> GLenum {
        // If a shader-pipeline ES 2.0 entry point was invoked on this
        // ES 1.1-only backend, report the synthetic `GL_INVALID_OPERATION`
        // before consulting the real driver's queue.
        let synthetic = self.pending_synthetic_error.replace(gles11::NO_ERROR);
        if synthetic != gles11::NO_ERROR {
            return synthetic;
        }
        gles11::GetError()
    }
    unsafe fn Enable(&mut self, cap: GLenum) {
        if cap == gles11::DEPTH_TEST {
            NATIVE_DEPTH_TEST_ENABLED.store(true, Ordering::Relaxed);
        } else if cap == gles11::FOG {
            NATIVE_FOG_ENABLED.store(true, Ordering::Relaxed);
        }
        gles11::Enable(cap)
    }
    unsafe fn IsEnabled(&mut self, cap: GLenum) -> GLboolean {
        if cap == gles11::DEPTH_TEST {
            return NATIVE_DEPTH_TEST_ENABLED.load(Ordering::Relaxed) as GLboolean;
        }
        gles11::IsEnabled(cap)
    }
    unsafe fn Disable(&mut self, cap: GLenum) {
        if cap == gles11::DEPTH_TEST {
            NATIVE_DEPTH_TEST_ENABLED.store(false, Ordering::Relaxed);
        } else if cap == gles11::FOG {
            NATIVE_FOG_ENABLED.store(false, Ordering::Relaxed);
        }
        gles11::Disable(cap)
    }
    unsafe fn ClientActiveTexture(&mut self, texture: GLenum) {
        gles11::ClientActiveTexture(texture);
    }
    unsafe fn EnableClientState(&mut self, array: GLenum) {
        gles11::EnableClientState(array)
    }
    unsafe fn DisableClientState(&mut self, array: GLenum) {
        gles11::DisableClientState(array)
    }
    unsafe fn GetBooleanv(&mut self, pname: GLenum, params: *mut GLboolean) {
        gles11::GetBooleanv(pname, params)
    }
    unsafe fn GetFloatv(&mut self, pname: GLenum, params: *mut GLfloat) {
        gles11::GetFloatv(pname, params)
    }
    unsafe fn GetIntegerv(&mut self, pname: GLenum, params: *mut GLint) {
        gles11::GetIntegerv(pname, params)
    }
    unsafe fn GetFixedv(&mut self, pname: GLenum, params: *mut GLfixed) {
        gles11::GetFixedv(pname, params)
    }
    unsafe fn GetTexEnviv(&mut self, target: GLenum, pname: GLenum, params: *mut GLint) {
        gles11::GetTexEnviv(target, pname, params)
    }
    unsafe fn GetTexEnvfv(&mut self, target: GLenum, pname: GLenum, params: *mut GLfloat) {
        gles11::GetTexEnvfv(target, pname, params)
    }
    unsafe fn GetTexEnvxv(&mut self, target: GLenum, pname: GLenum, params: *mut GLfixed) {
        gles11::GetTexEnvxv(target, pname, params)
    }
    unsafe fn GetTexParameteriv(&mut self, target: GLenum, pname: GLenum, params: *mut GLint) {
        gles11::GetTexParameteriv(target, pname, params)
    }
    unsafe fn GetTexParameterfv(&mut self, target: GLenum, pname: GLenum, params: *mut GLfloat) {
        gles11::GetTexParameterfv(target, pname, params)
    }
    unsafe fn GetTexParameterxv(&mut self, target: GLenum, pname: GLenum, params: *mut GLfixed) {
        gles11::GetTexParameterxv(target, pname, params)
    }
    unsafe fn GetClipPlanef(&mut self, plane: GLenum, equation: *mut GLfloat) {
        gles11::GetClipPlanef(plane, equation)
    }
    unsafe fn GetClipPlanex(&mut self, plane: GLenum, equation: *mut GLfixed) {
        gles11::GetClipPlanex(plane, equation)
    }
    unsafe fn GetLightfv(&mut self, light: GLenum, pname: GLenum, params: *mut GLfloat) {
        gles11::GetLightfv(light, pname, params)
    }
    unsafe fn GetLightxv(&mut self, light: GLenum, pname: GLenum, params: *mut GLfixed) {
        gles11::GetLightxv(light, pname, params)
    }
    unsafe fn GetMaterialfv(&mut self, face: GLenum, pname: GLenum, params: *mut GLfloat) {
        gles11::GetMaterialfv(face, pname, params)
    }
    unsafe fn GetMaterialxv(&mut self, face: GLenum, pname: GLenum, params: *mut GLfixed) {
        gles11::GetMaterialxv(face, pname, params)
    }
    unsafe fn GetPointerv(&mut self, pname: GLenum, params: *mut *const GLvoid) {
        gles11::GetPointerv(pname, params as *mut _ as *const _)
    }
    unsafe fn Hint(&mut self, target: GLenum, mode: GLenum) {
        gles11::Hint(target, mode)
    }
    unsafe fn Finish(&mut self) {
        gles11::Finish()
    }
    unsafe fn Flush(&mut self) {
        gles11::Flush()
    }

    // MALI HACK: Прячем сломанные расширения
    unsafe fn GetString(&mut self, name: GLenum) -> *const GLubyte {
        if name == gles11::EXTENSIONS {
            static mut FILTERED_EXTS: *mut std::os::raw::c_char = std::ptr::null_mut();
            if FILTERED_EXTS.is_null() {
                let orig_ptr = gles11::GetString(name);
                if !orig_ptr.is_null() {
                    let orig_str = std::ffi::CStr::from_ptr(orig_ptr as *const _).to_string_lossy();
                    // Вырезаем OES_matrix_palette чтобы заставить AC2
                    // использовать CPU анимацию
                    let filtered = orig_str.replace("GL_OES_matrix_palette", "");
                    let c_str = std::ffi::CString::new(filtered).unwrap();
                    FILTERED_EXTS = c_str.into_raw();
                } else {
                    return std::ptr::null();
                }
            }
            return FILTERED_EXTS as *const GLubyte;
        }
        gles11::GetString(name)
    }

    // Other state manipulation
    unsafe fn AlphaFunc(&mut self, func: GLenum, ref_: GLclampf) {
        gles11::AlphaFunc(func, ref_)
    }
    unsafe fn AlphaFuncx(&mut self, func: GLenum, ref_: GLclampx) {
        gles11::AlphaFuncx(func, ref_)
    }
    unsafe fn BlendFunc(&mut self, sfactor: GLenum, dfactor: GLenum) {
        gles11::BlendFunc(sfactor, dfactor)
    }
    unsafe fn BlendEquationOES(&mut self, mode: GLenum) {
        gles11::BlendEquationOES(mode);
    }
    unsafe fn ColorMask(
        &mut self,
        red: GLboolean,
        green: GLboolean,
        blue: GLboolean,
        alpha: GLboolean,
    ) {
        gles11::ColorMask(red, green, blue, alpha)
    }
    unsafe fn ClipPlanef(&mut self, plane: GLenum, equation: *const GLfloat) {
        gles11::ClipPlanef(plane, equation)
    }
    unsafe fn ClipPlanex(&mut self, plane: GLenum, equation: *const GLfixed) {
        gles11::ClipPlanex(plane, equation)
    }
    unsafe fn CullFace(&mut self, mode: GLenum) {
        gles11::CullFace(mode)
    }
    unsafe fn DepthFunc(&mut self, func: GLenum) {
        NATIVE_DEPTH_FUNC_LESS.store(func == gles11::LESS, Ordering::Relaxed);
        gles11::DepthFunc(func)
    }
    unsafe fn DepthMask(&mut self, flag: GLboolean) {
        gles11::DepthMask(flag)
    }
    unsafe fn FrontFace(&mut self, mode: GLenum) {
        gles11::FrontFace(mode)
    }
    unsafe fn DepthRangef(&mut self, near: GLclampf, far: GLclampf) {
        gles11::DepthRangef(near, far)
    }
    unsafe fn DepthRangex(&mut self, near: GLclampx, far: GLclampx) {
        gles11::DepthRangex(near, far)
    }
    unsafe fn PolygonOffset(&mut self, factor: GLfloat, units: GLfloat) {
        gles11::PolygonOffset(factor, units)
    }
    unsafe fn PolygonOffsetx(&mut self, factor: GLfixed, units: GLfixed) {
        gles11::PolygonOffsetx(factor, units)
    }
    unsafe fn SampleCoverage(&mut self, value: GLclampf, invert: GLboolean) {
        gles11::SampleCoverage(value, invert)
    }
    unsafe fn SampleCoveragex(&mut self, value: GLclampx, invert: GLboolean) {
        gles11::SampleCoveragex(value, invert)
    }
    unsafe fn ShadeModel(&mut self, mode: GLenum) {
        gles11::ShadeModel(mode)
    }
    unsafe fn Scissor(&mut self, x: GLint, y: GLint, width: GLsizei, height: GLsizei) {
        gles11::Scissor(x, y, width, height)
    }
    unsafe fn Viewport(&mut self, x: GLint, y: GLint, width: GLsizei, height: GLsizei) {
        gles11::Viewport(x, y, width, height)
    }
    unsafe fn LineWidth(&mut self, val: GLfloat) {
        gles11::LineWidth(val)
    }
    unsafe fn LineWidthx(&mut self, val: GLfixed) {
        gles11::LineWidthx(val)
    }
    unsafe fn StencilFunc(&mut self, func: GLenum, ref_: GLint, mask: GLuint) {
        gles11::StencilFunc(func, ref_, mask);
    }
    unsafe fn StencilOp(&mut self, sfail: GLenum, dpfail: GLenum, dppass: GLenum) {
        gles11::StencilOp(sfail, dpfail, dppass);
    }
    unsafe fn StencilMask(&mut self, mask: GLuint) {
        gles11::StencilMask(mask);
    }
    unsafe fn LogicOp(&mut self, opcode: GLenum) {
        gles11::LogicOp(opcode);
    }

    // Points
    unsafe fn PointSize(&mut self, size: GLfloat) {
        gles11::PointSize(size)
    }
    unsafe fn PointSizex(&mut self, size: GLfixed) {
        gles11::PointSizex(size)
    }
    unsafe fn PointParameterf(&mut self, pname: GLenum, param: GLfloat) {
        gles11::PointParameterf(pname, param)
    }
    unsafe fn PointParameterx(&mut self, pname: GLenum, param: GLfixed) {
        gles11::PointParameterx(pname, param)
    }
    unsafe fn PointParameterfv(&mut self, pname: GLenum, params: *const GLfloat) {
        gles11::PointParameterfv(pname, params)
    }
    unsafe fn PointParameterxv(&mut self, pname: GLenum, params: *const GLfixed) {
        gles11::PointParameterxv(pname, params)
    }

    // Lighting and materials
    unsafe fn Fogf(&mut self, pname: GLenum, param: GLfloat) {
        gles11::Fogf(pname, param)
    }
    unsafe fn Fogx(&mut self, pname: GLenum, param: GLfixed) {
        gles11::Fogx(pname, param)
    }
    unsafe fn Fogfv(&mut self, pname: GLenum, params: *const GLfloat) {
        gles11::Fogfv(pname, params)
    }
    unsafe fn Fogxv(&mut self, pname: GLenum, params: *const GLfixed) {
        gles11::Fogxv(pname, params)
    }
    unsafe fn Lightf(&mut self, light: GLenum, pname: GLenum, param: GLfloat) {
        gles11::Lightf(light, pname, param)
    }
    unsafe fn Lightx(&mut self, light: GLenum, pname: GLenum, param: GLfixed) {
        gles11::Lightx(light, pname, param)
    }
    unsafe fn Lightfv(&mut self, light: GLenum, pname: GLenum, params: *const GLfloat) {
        gles11::Lightfv(light, pname, params)
    }
    unsafe fn Lightxv(&mut self, light: GLenum, pname: GLenum, params: *const GLfixed) {
        gles11::Lightxv(light, pname, params)
    }
    unsafe fn LightModelf(&mut self, pname: GLenum, param: GLfloat) {
        gles11::LightModelf(pname, param)
    }
    unsafe fn LightModelx(&mut self, pname: GLenum, param: GLfixed) {
        gles11::LightModelx(pname, param)
    }
    unsafe fn LightModelfv(&mut self, pname: GLenum, params: *const GLfloat) {
        gles11::LightModelfv(pname, params)
    }
    unsafe fn LightModelxv(&mut self, pname: GLenum, params: *const GLfixed) {
        gles11::LightModelxv(pname, params)
    }
    unsafe fn Materialf(&mut self, face: GLenum, pname: GLenum, param: GLfloat) {
        gles11::Materialf(face, pname, param)
    }
    unsafe fn Materialx(&mut self, face: GLenum, pname: GLenum, param: GLfixed) {
        gles11::Materialx(face, pname, param)
    }
    unsafe fn Materialfv(&mut self, face: GLenum, pname: GLenum, params: *const GLfloat) {
        gles11::Materialfv(face, pname, params)
    }
    unsafe fn Materialxv(&mut self, face: GLenum, pname: GLenum, params: *const GLfixed) {
        gles11::Materialxv(face, pname, params)
    }

    // Buffers
    unsafe fn IsBuffer(&mut self, buffer: GLuint) -> GLboolean {
        gles11::IsBuffer(buffer)
    }
    unsafe fn GenBuffers(&mut self, n: GLsizei, buffers: *mut GLuint) {
        gles11::GenBuffers(n, buffers)
    }
    unsafe fn DeleteBuffers(&mut self, n: GLsizei, buffers: *const GLuint) {
        gles11::DeleteBuffers(n, buffers)
    }
    unsafe fn BindBuffer(&mut self, target: GLenum, buffer: GLuint) {
        gles11::BindBuffer(target, buffer)
    }
    unsafe fn BufferData(
        &mut self,
        target: GLenum,
        size: GLsizeiptr,
        data: *const GLvoid,
        usage: GLenum,
    ) {
        gles11::BufferData(target, size, data, usage)
    }
    unsafe fn BufferSubData(
        &mut self,
        target: GLenum,
        offset: GLintptr,
        size: GLsizeiptr,
        data: *const GLvoid,
    ) {
        gles11::BufferSubData(target, offset, size, data)
    }

    // Non-pointers
    unsafe fn Color4f(&mut self, red: GLfloat, green: GLfloat, blue: GLfloat, alpha: GLfloat) {
        gles11::Color4f(red, green, blue, alpha)
    }
    unsafe fn Color4x(&mut self, red: GLfixed, green: GLfixed, blue: GLfixed, alpha: GLfixed) {
        gles11::Color4x(red, green, blue, alpha)
    }
    unsafe fn Color4ub(&mut self, red: GLubyte, green: GLubyte, blue: GLubyte, alpha: GLubyte) {
        gles11::Color4ub(red, green, blue, alpha)
    }
    unsafe fn Normal3f(&mut self, nx: GLfloat, ny: GLfloat, nz: GLfloat) {
        gles11::Normal3f(nx, ny, nz)
    }
    unsafe fn Normal3x(&mut self, nx: GLfixed, ny: GLfixed, nz: GLfixed) {
        gles11::Normal3x(nx, ny, nz)
    }

    // Pointers - Возвращаем твою изначальную правильную защиту от крашей Mali
    unsafe fn ColorPointer(
        &mut self,
        size: GLint,
        type_: GLenum,
        stride: GLsizei,
        pointer: *const GLvoid,
    ) {
        if pointer.is_null() {
            let mut bound_buffer: GLint = 0;
            gles11::GetIntegerv(gles11::ARRAY_BUFFER_BINDING, &mut bound_buffer);
            if bound_buffer == 0 {
                gles11::DisableClientState(gles11::COLOR_ARRAY);
                return;
            }
        }
        gles11::ColorPointer(size, type_, stride, pointer)
    }

    unsafe fn NormalPointer(&mut self, type_: GLenum, stride: GLsizei, pointer: *const GLvoid) {
        if pointer.is_null() {
            let mut bound_buffer: GLint = 0;
            gles11::GetIntegerv(gles11::ARRAY_BUFFER_BINDING, &mut bound_buffer);
            if bound_buffer == 0 {
                gles11::DisableClientState(gles11::NORMAL_ARRAY);
                return;
            }
        }
        gles11::NormalPointer(type_, stride, pointer)
    }

    unsafe fn TexCoordPointer(
        &mut self,
        size: GLint,
        type_: GLenum,
        stride: GLsizei,
        pointer: *const GLvoid,
    ) {
        if pointer.is_null() {
            let mut bound_buffer: GLint = 0;
            gles11::GetIntegerv(gles11::ARRAY_BUFFER_BINDING, &mut bound_buffer);
            if bound_buffer == 0 {
                gles11::DisableClientState(gles11::TEXTURE_COORD_ARRAY);
                return;
            }
        }
        gles11::TexCoordPointer(size, type_, stride, pointer)
    }

    unsafe fn VertexPointer(
        &mut self,
        size: GLint,
        type_: GLenum,
        stride: GLsizei,
        pointer: *const GLvoid,
    ) {
        if pointer.is_null() {
            let mut bound_buffer: GLint = 0;
            gles11::GetIntegerv(gles11::ARRAY_BUFFER_BINDING, &mut bound_buffer);
            if bound_buffer == 0 {
                gles11::DisableClientState(gles11::VERTEX_ARRAY);
                return;
            }
        }
        gles11::VertexPointer(size, type_, stride, pointer)
    }

    unsafe fn PointSizePointerOES(
        &mut self,
        type_: GLenum,
        stride: GLsizei,
        pointer: *const GLvoid,
    ) {
        gles11::PointSizePointerOES(type_, stride, pointer)
    }

    // Drawing
    unsafe fn DrawArrays(&mut self, mode: GLenum, first: GLint, count: GLsizei) {
        let proj_z_fix = native_proj_z_fix_begin();
        let opening_fog = native_opening_fog_begin(self.opening_fog);
        let no_fog = native_nofog_begin(count);
        let no_fog_ortho = native_nofog_ortho_begin();
        let texfilter = native_texfilter_begin(count);
        let texnodepth = native_texnodepth_begin(count);
        let no_cull = native_no_cull_begin(count);
        let texmat = native_texmat_identity_begin(count);
        let texflat = native_texflat_begin();
        let blood_depth = native_blood_depth_begin(mode, count);
        let pixel_trace = native_pixel_trace_begin();
        gles11::DrawArrays(mode, first, count);
        native_pixel_trace_end(pixel_trace, "arrays", count);
        native_blood_depth_end(blood_depth);
        native_texflat_end(texflat);
        native_texmat_identity_end(texmat);
        native_no_cull_end(no_cull);
        native_texnodepth_end(texnodepth);
        native_texfilter_end(texfilter);
        native_nofog_end(no_fog_ortho);
        native_nofog_end(no_fog);
        native_opening_fog_end(opening_fog);
        native_proj_z_fix_end(proj_z_fix);
    }

    unsafe fn DrawElements(
        &mut self,
        mode: GLenum,
        count: GLsizei,
        type_: GLenum,
        indices: *const GLvoid,
    ) {
        if indices.is_null() {
            let mut bound_buffer: GLint = 0;
            gles11::GetIntegerv(gles11::ELEMENT_ARRAY_BUFFER_BINDING, &mut bound_buffer);
            if bound_buffer == 0 {
                return;
            }
        }
        let proj_z_fix = native_proj_z_fix_begin();
        let opening_fog = native_opening_fog_begin(self.opening_fog);
        let no_fog = native_nofog_begin(count);
        let no_fog_ortho = native_nofog_ortho_begin();
        let texfilter = native_texfilter_begin(count);
        let texnodepth = native_texnodepth_begin(count);
        let no_cull = native_no_cull_begin(count);
        let texmat = native_texmat_identity_begin(count);
        let texflat = native_texflat_begin();
        let blood_depth = native_blood_depth_begin(mode, count);
        let pixel_trace = native_pixel_trace_begin();
        if !(blood_depth && native_draw_scaled_blood_elements(count, type_, indices)) {
            gles11::DrawElements(mode, count, type_, indices);
        }
        native_pixel_trace_end(pixel_trace, "elements", count);
        native_blood_depth_end(blood_depth);
        native_texflat_end(texflat);
        native_texmat_identity_end(texmat);
        native_no_cull_end(no_cull);
        native_texnodepth_end(texnodepth);
        native_texfilter_end(texfilter);
        native_nofog_end(no_fog_ortho);
        native_nofog_end(no_fog);
        native_opening_fog_end(opening_fog);
        native_proj_z_fix_end(proj_z_fix);
    }

    // GL_OES_draw_texture
    unsafe fn DrawTexsOES(&mut self, x: i16, y: i16, z: i16, width: i16, height: i16) {
        gles11::DrawTexsOES(x, y, z, width, height)
    }
    unsafe fn DrawTexiOES(&mut self, x: GLint, y: GLint, z: GLint, width: GLint, height: GLint) {
        gles11::DrawTexiOES(x, y, z, width, height)
    }
    unsafe fn DrawTexxOES(
        &mut self,
        x: GLfixed,
        y: GLfixed,
        z: GLfixed,
        width: GLfixed,
        height: GLfixed,
    ) {
        gles11::DrawTexxOES(x, y, z, width, height)
    }
    unsafe fn DrawTexfOES(
        &mut self,
        x: GLfloat,
        y: GLfloat,
        z: GLfloat,
        width: GLfloat,
        height: GLfloat,
    ) {
        gles11::DrawTexfOES(x, y, z, width, height)
    }
    unsafe fn DrawTexsvOES(&mut self, coords: *const i16) {
        gles11::DrawTexsvOES(coords)
    }
    unsafe fn DrawTexivOES(&mut self, coords: *const GLint) {
        gles11::DrawTexivOES(coords)
    }
    unsafe fn DrawTexxvOES(&mut self, coords: *const GLfixed) {
        gles11::DrawTexxvOES(coords)
    }
    unsafe fn DrawTexfvOES(&mut self, coords: *const GLfloat) {
        gles11::DrawTexfvOES(coords)
    }

    // Clearing
    unsafe fn Clear(&mut self, mask: GLbitfield) {
        gles11::Clear(mask)
    }
    unsafe fn ClearColor(
        &mut self,
        red: GLclampf,
        green: GLclampf,
        blue: GLclampf,
        alpha: GLclampf,
    ) {
        gles11::ClearColor(red, green, blue, alpha)
    }
    unsafe fn ClearColorx(
        &mut self,
        red: GLclampx,
        green: GLclampx,
        blue: GLclampx,
        alpha: GLclampx,
    ) {
        gles11::ClearColorx(red, green, blue, alpha)
    }
    unsafe fn ClearDepthf(&mut self, depth: GLclampf) {
        gles11::ClearDepthf(depth)
    }
    unsafe fn ClearDepthx(&mut self, depth: GLclampx) {
        gles11::ClearDepthx(depth)
    }
    unsafe fn ClearStencil(&mut self, s: GLint) {
        gles11::ClearStencil(s)
    }

    // Textures
    unsafe fn PixelStorei(&mut self, pname: GLenum, param: GLint) {
        gles11::PixelStorei(pname, param)
    }
    unsafe fn ReadPixels(
        &mut self,
        x: GLint,
        y: GLint,
        width: GLsizei,
        height: GLsizei,
        format: GLenum,
        type_: GLenum,
        pixels: *mut GLvoid,
    ) {
        gles11::ReadPixels(x, y, width, height, format, type_, pixels)
    }
    unsafe fn GenTextures(&mut self, n: GLsizei, textures: *mut GLuint) {
        gles11::GenTextures(n, textures)
    }
    unsafe fn DeleteTextures(&mut self, n: GLsizei, textures: *const GLuint) {
        gles11::DeleteTextures(n, textures)
    }
    unsafe fn ActiveTexture(&mut self, texture: GLenum) {
        gles11::ActiveTexture(texture)
    }
    unsafe fn IsTexture(&mut self, texture: GLuint) -> GLboolean {
        gles11::IsTexture(texture)
    }
    unsafe fn BindTexture(&mut self, target: GLenum, texture: GLuint) {
        gles11::BindTexture(target, texture)
    }
    unsafe fn TexParameteri(&mut self, target: GLenum, pname: GLenum, param: GLint) {
        gles11::TexParameteri(target, pname, param)
    }
    unsafe fn TexParameterf(&mut self, target: GLenum, pname: GLenum, param: GLfloat) {
        gles11::TexParameterf(target, pname, param)
    }
    unsafe fn TexParameterx(&mut self, target: GLenum, pname: GLenum, param: GLfixed) {
        gles11::TexParameterx(target, pname, param)
    }
    unsafe fn TexParameteriv(&mut self, target: GLenum, pname: GLenum, params: *const GLint) {
        gles11::TexParameteriv(target, pname, params)
    }
    unsafe fn TexParameterfv(&mut self, target: GLenum, pname: GLenum, params: *const GLfloat) {
        gles11::TexParameterfv(target, pname, params)
    }
    unsafe fn TexParameterxv(&mut self, target: GLenum, pname: GLenum, params: *const GLfixed) {
        gles11::TexParameterxv(target, pname, params)
    }

    unsafe fn TexImage2D(
        &mut self,
        target: GLenum,
        level: GLint,
        mut internalformat: GLint,
        width: GLsizei,
        height: GLsizei,
        border: GLint,
        format: GLenum,
        type_: GLenum,
        pixels: *const GLvoid,
    ) {
        if format == gles11::BGRA_EXT {
            internalformat = gles11::BGRA_EXT as GLint
        }
        gles11::TexImage2D(
            target,
            level,
            internalformat,
            width,
            height,
            border,
            format,
            type_,
            pixels,
        )
    }

    unsafe fn TexSubImage2D(
        &mut self,
        target: GLenum,
        level: GLint,
        xoffset: GLint,
        yoffset: GLint,
        width: GLsizei,
        height: GLsizei,
        format: GLenum,
        type_: GLenum,
        pixels: *const GLvoid,
    ) {
        gles11::TexSubImage2D(
            target, level, xoffset, yoffset, width, height, format, type_, pixels,
        )
    }

    unsafe fn CompressedTexImage2D(
        &mut self,
        target: GLenum,
        level: GLint,
        internalformat: GLenum,
        width: GLsizei,
        height: GLsizei,
        border: GLint,
        image_size: GLsizei,
        data: *const GLvoid,
    ) {
        // POSIX-style guard: a NULL data pointer with image_size==0 is
        // technically allowed by the spec for some texture-storage queries,
        // but in practice no iPhone OS app does this — it'd just be a guest
        // bug. Drop the call on the floor instead of dereferencing the
        // pointer.
        if data.is_null() && image_size > 0 {
            log!(
                "Warning: GLES1Native::CompressedTexImage2D: NULL data with \
                 non-zero image_size {image_size} (target={target:#x}, \
                 level={level}, format={internalformat:#x}, {width}x{height}); \
                 dropping upload."
            );
            return;
        }

        // Slice the guest payload exactly once. Even when the host driver
        // advertises PVRTC natively, we need a `&[u8]` for the paletted /
        // decode fallbacks below.
        let payload: &[u8] = if image_size > 0 {
            std::slice::from_raw_parts(data.cast::<u8>(), image_size as usize)
        } else {
            &[]
        };

        // PowerVR-class drivers (Apple's iPhone OS, plus desktop GL via the
        // Mesa PowerVR backend) support `GL_IMG_texture_compression_pvrtc`
        // natively, in which case the most efficient thing to do is to hand
        // the compressed payload straight to the driver. ARM Mali, Qualcomm
        // Adreno's ES 1.1 surface, the Mesa software rasteriser, etc. do
        // *not* advertise PVRTC, so we need to software-decode to RGBA
        // before uploading. The decision is based on the
        // `GL_EXTENSIONS` string queried at context creation; see
        // [`GLES1NativeContext::pvrtc_native`].
        if !self.pvrtc_native && !payload.is_empty() {
            if try_decode_pvrtc(
                self,
                target,
                level,
                internalformat,
                width,
                height,
                border,
                payload,
            ) {
                return;
            }
            // Apple-targeted apps also sometimes ship
            // `GL_OES_compressed_paletted_texture` data. Mali / Adreno ES 1.1
            // surfaces likewise don't advertise that extension, so a
            // straight passthrough would silently fail with
            // `GL_INVALID_ENUM`. Software-decode paletted textures to
            // uncompressed RGBA/RGB and upload via glTexImage2D.
            if let Some(PalettedTextureFormat {
                index_is_nibble,
                palette_entry_format,
                palette_entry_type,
            }) = PalettedTextureFormat::get_info(internalformat)
            {
                let palette_entry_size = match palette_entry_type {
                    gles11::UNSIGNED_BYTE => match palette_entry_format {
                        gles11::RGB => 3,
                        gles11::RGBA => 4,
                        _ => unreachable!(),
                    },
                    gles11::UNSIGNED_SHORT_5_6_5
                    | gles11::UNSIGNED_SHORT_4_4_4_4
                    | gles11::UNSIGNED_SHORT_5_5_5_1 => 2,
                    _ => unreachable!(),
                };
                let palette_entry_count: usize = if index_is_nibble { 16 } else { 256 };
                let palette_size = palette_entry_size * palette_entry_count;

                let index_count = width as usize * height as usize;
                let (index_word_size, index_word_count) = if index_is_nibble {
                    (1, index_count.div_ceil(2))
                } else {
                    (4, index_count.div_ceil(4))
                };
                let indices_size = index_word_size * index_word_count;

                let expected_size = palette_size + indices_size;
                if payload.len() < expected_size {
                    log!(
                        "Warning: GLES1Native::CompressedTexImage2D: paletted \
                         format {internalformat:#x} payload too small: got {} \
                         bytes, expected at least {expected_size} for \
                         {width}x{height}; skipping upload.",
                        payload.len()
                    );
                    return;
                }

                let (palette, indices) = payload.split_at(palette_size);

                let mut decoded = Vec::<u8>::with_capacity(palette_entry_size * index_count);
                for i in 0..index_count {
                    let index = if index_is_nibble {
                        (indices[i / 2] >> ((1 - (i % 2)) * 4)) & 0xf
                    } else {
                        indices[i]
                    } as usize;
                    let start = index * palette_entry_size;
                    let palette_entry = &palette[start..start + palette_entry_size];
                    decoded.extend_from_slice(palette_entry);
                }

                log_dbg!(
                    "GLES1Native: software-decoded paletted texture \
                     {width}x{height} (format {internalformat:#x})"
                );

                gles11::TexImage2D(
                    target,
                    level,
                    palette_entry_format as GLint,
                    width,
                    height,
                    border,
                    palette_entry_format,
                    palette_entry_type,
                    decoded.as_ptr() as *const _,
                );
                return;
            }
            // Unknown compressed format AND host driver doesn't advertise
            // PVRTC — passthrough would just produce GL_INVALID_ENUM. Log
            // once per (format) value so a misbehaving guest can't spam
            // the console.
            use std::sync::atomic::{AtomicBool, Ordering};
            static SEEN_UNKNOWN: AtomicBool = AtomicBool::new(false);
            if !SEEN_UNKNOWN.swap(true, Ordering::Relaxed) {
                log!(
                    "Warning: GLES1Native::CompressedTexImage2D: unknown \
                     compressed format {internalformat:#x} on a host that does \
                     not advertise PVRTC; passing through to driver but \
                     expecting GL_INVALID_ENUM. {width}x{height}, level {level}. \
                     [this log will only be shown once for unknown formats]"
                );
            }
        }

        // Either we're on a PVRTC-capable host (let the driver do its thing),
        // or we hit a non-PVRTC, non-paletted format on a non-PVRTC host
        // (let it fail loudly with GL_INVALID_ENUM, exactly like a real
        // device would).
        gles11::CompressedTexImage2D(
            target,
            level,
            internalformat,
            width,
            height,
            border,
            image_size,
            data,
        );
    }

    unsafe fn CompressedTexSubImage2D(
        &mut self,
        target: GLenum,
        level: GLint,
        xoffset: GLint,
        yoffset: GLint,
        width: GLsizei,
        height: GLsizei,
        format: GLenum,
        image_size: GLsizei,
        data: *const GLvoid,
    ) {
        // PVRTC sub-image updates aren't allowed by the IMG spec — Apple's
        // ES 1.1 driver returns GL_INVALID_OPERATION too — but we forward
        // anyway so the guest sees the expected error. Most apps never call
        // this entry point.
        if data.is_null() && image_size > 0 {
            log!(
                "Warning: GLES1Native::CompressedTexSubImage2D: NULL data with \
                 non-zero image_size {image_size}; dropping call."
            );
            return;
        }
        gles11::CompressedTexSubImage2D(
            target, level, xoffset, yoffset, width, height, format, image_size, data,
        );
    }

    unsafe fn CopyTexImage2D(
        &mut self,
        target: GLenum,
        level: GLint,
        internalformat: GLenum,
        x: GLint,
        y: GLint,
        width: GLsizei,
        height: GLsizei,
        border: GLint,
    ) {
        gles11::CopyTexImage2D(target, level, internalformat, x, y, width, height, border)
    }
    unsafe fn CopyTexSubImage2D(
        &mut self,
        target: GLenum,
        level: GLint,
        xoffset: GLint,
        yoffset: GLint,
        x: GLint,
        y: GLint,
        width: GLsizei,
        height: GLsizei,
    ) {
        gles11::CopyTexSubImage2D(target, level, xoffset, yoffset, x, y, width, height)
    }
    unsafe fn TexEnvf(&mut self, target: GLenum, pname: GLenum, param: GLfloat) {
        gles11::TexEnvf(target, pname, param)
    }
    unsafe fn TexEnvx(&mut self, target: GLenum, pname: GLenum, param: GLfixed) {
        gles11::TexEnvx(target, pname, param)
    }
    unsafe fn TexEnvi(&mut self, target: GLenum, pname: GLenum, param: GLint) {
        gles11::TexEnvi(target, pname, param)
    }

    unsafe fn TexEnvfv(&mut self, target: GLenum, pname: GLenum, params: *const GLfloat) {
        if target == gles11::TEXTURE_FILTER_CONTROL_EXT {
            assert!(pname == gles11::TEXTURE_LOD_BIAS_EXT);
            unsafe {
                if !CStr::from_ptr(gles11::GetString(gles11::EXTENSIONS) as _)
                    .to_str()
                    .unwrap()
                    .contains("EXT_texture_lod_bias")
                {
                    return;
                }
            };
        }
        gles11::TexEnvfv(target, pname, params)
    }

    unsafe fn TexEnvxv(&mut self, target: GLenum, pname: GLenum, params: *const GLfixed) {
        gles11::TexEnvxv(target, pname, params)
    }
    unsafe fn TexEnviv(&mut self, target: GLenum, pname: GLenum, params: *const GLint) {
        gles11::TexEnviv(target, pname, params)
    }
    unsafe fn MultiTexCoord4f(
        &mut self,
        target: GLenum,
        s: GLfloat,
        t: GLfloat,
        r: GLfloat,
        q: GLfloat,
    ) {
        gles11::MultiTexCoord4f(target, s, t, r, q)
    }
    unsafe fn MultiTexCoord4x(
        &mut self,
        target: GLenum,
        s: GLfixed,
        t: GLfixed,
        r: GLfixed,
        q: GLfixed,
    ) {
        gles11::MultiTexCoord4x(target, s, t, r, q)
    }

    // Matrix stack operations
    unsafe fn MatrixMode(&mut self, mode: GLenum) {
        gles11::MatrixMode(mode)
    }
    unsafe fn LoadIdentity(&mut self) {
        gles11::LoadIdentity()
    }
    unsafe fn LoadMatrixf(&mut self, m: *const GLfloat) {
        gles11::LoadMatrixf(m)
    }
    unsafe fn LoadMatrixx(&mut self, m: *const GLfixed) {
        gles11::LoadMatrixx(m)
    }
    unsafe fn MultMatrixf(&mut self, m: *const GLfloat) {
        gles11::MultMatrixf(m)
    }
    unsafe fn MultMatrixx(&mut self, m: *const GLfixed) {
        gles11::MultMatrixx(m)
    }
    unsafe fn PushMatrix(&mut self) {
        gles11::PushMatrix()
    }
    unsafe fn PopMatrix(&mut self) {
        gles11::PopMatrix();
    }
    unsafe fn Orthof(
        &mut self,
        left: GLfloat,
        right: GLfloat,
        bottom: GLfloat,
        top: GLfloat,
        near: GLfloat,
        far: GLfloat,
    ) {
        gles11::Orthof(left, right, bottom, top, near, far)
    }
    unsafe fn Orthox(
        &mut self,
        left: GLfixed,
        right: GLfixed,
        bottom: GLfixed,
        top: GLfixed,
        near: GLfixed,
        far: GLfixed,
    ) {
        gles11::Orthox(left, right, bottom, top, near, far)
    }
    unsafe fn Frustumf(
        &mut self,
        left: GLfloat,
        right: GLfloat,
        bottom: GLfloat,
        top: GLfloat,
        near: GLfloat,
        far: GLfloat,
    ) {
        let far = native_adjust_frustum_far(near, far);
        gles11::Frustumf(left, right, bottom, top, near, far)
    }
    unsafe fn Frustumx(
        &mut self,
        left: GLfixed,
        right: GLfixed,
        bottom: GLfixed,
        top: GLfixed,
        near: GLfixed,
        far: GLfixed,
    ) {
        let near_f = fixed_to_float(near);
        let far_f = fixed_to_float(far);
        let adjusted_far = native_adjust_frustum_far(near_f, far_f);
        if adjusted_far != far_f {
            gles11::Frustumf(
                fixed_to_float(left),
                fixed_to_float(right),
                fixed_to_float(bottom),
                fixed_to_float(top),
                near_f,
                adjusted_far,
            )
        } else {
            gles11::Frustumx(left, right, bottom, top, near, far)
        }
    }
    unsafe fn Rotatef(&mut self, angle: GLfloat, x: GLfloat, y: GLfloat, z: GLfloat) {
        gles11::Rotatef(angle, x, y, z)
    }
    unsafe fn Rotatex(&mut self, angle: GLfixed, x: GLfixed, y: GLfixed, z: GLfixed) {
        gles11::Rotatex(angle, x, y, z)
    }
    unsafe fn Scalef(&mut self, x: GLfloat, y: GLfloat, z: GLfloat) {
        gles11::Scalef(x, y, z)
    }
    unsafe fn Scalex(&mut self, x: GLfixed, y: GLfixed, z: GLfixed) {
        gles11::Scalex(x, y, z)
    }
    unsafe fn Translatef(&mut self, x: GLfloat, y: GLfloat, z: GLfloat) {
        gles11::Translatef(x, y, z)
    }
    unsafe fn Translatex(&mut self, x: GLfixed, y: GLfixed, z: GLfixed) {
        gles11::Translatex(x, y, z)
    }

    // OES_framebuffer_object -> EXT_framebuffer_object
    unsafe fn GenFramebuffersOES(&mut self, n: GLsizei, framebuffers: *mut GLuint) {
        gles11::GenFramebuffersOES(n, framebuffers)
    }
    unsafe fn GenRenderbuffersOES(&mut self, n: GLsizei, renderbuffers: *mut GLuint) {
        gles11::GenRenderbuffersOES(n, renderbuffers)
    }
    unsafe fn IsFramebufferOES(&mut self, renderbuffer: GLuint) -> GLboolean {
        gles11::IsFramebufferOES(renderbuffer)
    }
    unsafe fn IsRenderbufferOES(&mut self, renderbuffer: GLuint) -> GLboolean {
        gles11::IsRenderbufferOES(renderbuffer)
    }
    unsafe fn BindFramebufferOES(&mut self, target: GLenum, framebuffer: GLuint) {
        gles11::BindFramebufferOES(target, framebuffer)
    }
    unsafe fn BindRenderbufferOES(&mut self, target: GLenum, renderbuffer: GLuint) {
        gles11::BindRenderbufferOES(target, renderbuffer)
    }
    unsafe fn RenderbufferStorageOES(
        &mut self,
        target: GLenum,
        internalformat: GLenum,
        width: GLsizei,
        height: GLsizei,
    ) {
        // The SDL window's depth request does not affect guest-owned FBOs.
        // On Adreno the game's explicit D16 attachment otherwise replaces
        // our high-precision drawable depth and quantizes distant decks.
        let promote = internalformat == 0x81a5 // GL_DEPTH_COMPONENT16_OES
            && target == gles11::RENDERBUFFER_OES && width > 0 && height > 0
            && native_env_flag("TOUCHHLE_DS_NATIVE_DEPTH24")
            && CStr::from_ptr(gles11::GetString(gles11::EXTENSIONS) as _)
                .to_string_lossy().split_whitespace()
                .any(|extension| extension == "GL_OES_depth24");
        if promote {
            gles11::RenderbufferStorageOES(target, 0x81a6, width, height);
            let mut bits = 0;
            gles11::GetRenderbufferParameterivOES(target, 0x8d54, &mut bits);
            if bits >= 24 {
                log!("[DSnativeDepth] guest D16 promoted to D24: {}x{} actual_bits={}", width, height, bits);
                return;
            }
            log!("[DSnativeDepth] D24 unavailable (bits={}); retaining guest D16", bits);
        }
        gles11::RenderbufferStorageOES(target, internalformat, width, height)
    }
    unsafe fn FramebufferRenderbufferOES(
        &mut self,
        target: GLenum,
        attachment: GLenum,
        renderbuffertarget: GLenum,
        renderbuffer: GLuint,
    ) {
        gles11::FramebufferRenderbufferOES(target, attachment, renderbuffertarget, renderbuffer);
        if attachment == gles11::DEPTH_ATTACHMENT_OES
            && native_env_flag("TOUCHHLE_DS_NATIVE_DEPTH24") {
            let mut bits = 0;
            gles11::GetIntegerv(gles11::DEPTH_BITS, &mut bits);
            let status = gles11::CheckFramebufferStatusOES(target);
            log!("[DSnativeDepth] attached depth rb={} framebuffer_bits={} status=0x{:x}", renderbuffer, bits, status);
        }
    }
    unsafe fn FramebufferTexture2DOES(
        &mut self,
        target: GLenum,
        attachment: GLenum,
        textarget: GLenum,
        texture: GLuint,
        level: i32,
    ) {
        gles11::FramebufferTexture2DOES(target, attachment, textarget, texture, level)
    }
    unsafe fn GetFramebufferAttachmentParameterivOES(
        &mut self,
        target: GLenum,
        attachment: GLenum,
        pname: GLenum,
        params: *mut GLint,
    ) {
        gles11::GetFramebufferAttachmentParameterivOES(target, attachment, pname, params)
    }
    unsafe fn GetRenderbufferParameterivOES(
        &mut self,
        target: GLenum,
        pname: GLenum,
        params: *mut GLint,
    ) {
        gles11::GetRenderbufferParameterivOES(target, pname, params)
    }
    unsafe fn CheckFramebufferStatusOES(&mut self, target: GLenum) -> GLenum {
        gles11::CheckFramebufferStatusOES(target)
    }
    unsafe fn DeleteFramebuffersOES(&mut self, n: GLsizei, framebuffers: *const GLuint) {
        gles11::DeleteFramebuffersOES(n, framebuffers)
    }
    unsafe fn DeleteRenderbuffersOES(&mut self, n: GLsizei, renderbuffers: *const GLuint) {
        gles11::DeleteRenderbuffersOES(n, renderbuffers)
    }
    unsafe fn GenerateMipmapOES(&mut self, target: GLenum) {
        gles11::GenerateMipmapOES(target)
    }

    // Non-OES aliases for OES_framebuffer_object functions.
    unsafe fn GenFramebuffers(&mut self, n: GLsizei, framebuffers: *mut GLuint) {
        self.GenFramebuffersOES(n, framebuffers)
    }
    unsafe fn GenRenderbuffers(&mut self, n: GLsizei, renderbuffers: *mut GLuint) {
        self.GenRenderbuffersOES(n, renderbuffers)
    }
    unsafe fn IsFramebuffer(&mut self, framebuffer: GLuint) -> GLboolean {
        self.IsFramebufferOES(framebuffer)
    }
    unsafe fn IsRenderbuffer(&mut self, renderbuffer: GLuint) -> GLboolean {
        self.IsRenderbufferOES(renderbuffer)
    }
    unsafe fn BindFramebuffer(&mut self, target: GLenum, framebuffer: GLuint) {
        self.BindFramebufferOES(target, framebuffer)
    }
    unsafe fn BindRenderbuffer(&mut self, target: GLenum, renderbuffer: GLuint) {
        self.BindRenderbufferOES(target, renderbuffer)
    }
    unsafe fn RenderbufferStorage(
        &mut self,
        target: GLenum,
        internalformat: GLenum,
        width: GLsizei,
        height: GLsizei,
    ) {
        self.RenderbufferStorageOES(target, internalformat, width, height)
    }
    unsafe fn FramebufferRenderbuffer(
        &mut self,
        target: GLenum,
        attachment: GLenum,
        renderbuffertarget: GLenum,
        renderbuffer: GLuint,
    ) {
        self.FramebufferRenderbufferOES(target, attachment, renderbuffertarget, renderbuffer)
    }
    unsafe fn FramebufferTexture2D(
        &mut self,
        target: GLenum,
        attachment: GLenum,
        textarget: GLenum,
        texture: GLuint,
        level: i32,
    ) {
        self.FramebufferTexture2DOES(target, attachment, textarget, texture, level)
    }
    unsafe fn CheckFramebufferStatus(&mut self, target: GLenum) -> GLenum {
        self.CheckFramebufferStatusOES(target)
    }
    unsafe fn DeleteFramebuffers(&mut self, n: GLsizei, framebuffers: *const GLuint) {
        self.DeleteFramebuffersOES(n, framebuffers)
    }
    unsafe fn DeleteRenderbuffers(&mut self, n: GLsizei, renderbuffers: *const GLuint) {
        self.DeleteRenderbuffersOES(n, renderbuffers)
    }
    unsafe fn GenerateMipmap(&mut self, target: GLenum) {
        self.GenerateMipmapOES(target)
    }
    unsafe fn GetFramebufferAttachmentParameteriv(
        &mut self,
        target: GLenum,
        attachment: GLenum,
        pname: GLenum,
        params: *mut GLint,
    ) {
        self.GetFramebufferAttachmentParameterivOES(target, attachment, pname, params)
    }
    unsafe fn GetRenderbufferParameteriv(
        &mut self,
        target: GLenum,
        pname: GLenum,
        params: *mut GLint,
    ) {
        self.GetRenderbufferParameterivOES(target, pname, params)
    }
    unsafe fn GetBufferParameteriv(&mut self, target: GLenum, pname: GLenum, params: *mut GLint) {
        gles11::GetBufferParameteriv(target, pname, params)
    }
    unsafe fn MapBufferOES(&mut self, target: GLenum, access: GLenum) -> *mut GLvoid {
        gles11::MapBufferOES(target, access)
    }
    unsafe fn UnmapBufferOES(&mut self, target: GLenum) -> GLboolean {
        gles11::UnmapBufferOES(target)
    }

    // ===== OpenGL ES 2.0 shader-pipeline entry points =====
    //
    // None of these exist on a strict ES 1.1 driver. The ES 2.0 / ES 1.1
    // specifications agree that an implementation must report
    // `GL_INVALID_OPERATION` for "operation not supported by this profile",
    // so we route every entry point to [`record_es2_unsupported`] which:
    //   1. flags `GL_INVALID_OPERATION` in the synthetic error queue (read by
    //      our `GetError` override above), and
    //   2. logs a one-shot warning that the guest app is mixing ES 1.1 and
    //      ES 2.0 calls.
    // Methods with non-`()` return types pick the standard "failure" sentinel
    // value defined by the ES 2.0 spec: `0` for object names
    // (`glCreateShader`, `glCreateProgram`), `-1` for locations
    // (`glGetUniformLocation`, `glGetAttribLocation`), `GL_FALSE` for
    // boolean queries (`glIsShader`, `glIsProgram`).
    unsafe fn CreateShader(&mut self, _type_: GLenum) -> GLuint {
        self.record_es2_unsupported("CreateShader");
        0
    }
    unsafe fn DeleteShader(&mut self, _shader: GLuint) {
        self.record_es2_unsupported("DeleteShader");
    }
    unsafe fn ShaderSource(
        &mut self,
        _shader: GLuint,
        _count: GLsizei,
        _string: *const *const GLchar,
        _length: *const GLint,
    ) {
        self.record_es2_unsupported("ShaderSource");
    }
    unsafe fn CompileShader(&mut self, _shader: GLuint) {
        self.record_es2_unsupported("CompileShader");
    }
    unsafe fn GetShaderiv(&mut self, _shader: GLuint, _pname: GLenum, _params: *mut GLint) {
        self.record_es2_unsupported("GetShaderiv");
    }
    unsafe fn GetShaderInfoLog(
        &mut self,
        _shader: GLuint,
        _maxLength: GLsizei,
        _length: *mut GLsizei,
        _infoLog: *mut GLchar,
    ) {
        self.record_es2_unsupported("GetShaderInfoLog");
    }
    unsafe fn IsShader(&mut self, _shader: GLuint) -> GLboolean {
        self.record_es2_unsupported("IsShader");
        gles11::FALSE
    }
    unsafe fn CreateProgram(&mut self) -> GLuint {
        self.record_es2_unsupported("CreateProgram");
        0
    }
    unsafe fn DeleteProgram(&mut self, _program: GLuint) {
        self.record_es2_unsupported("DeleteProgram");
    }
    unsafe fn AttachShader(&mut self, _program: GLuint, _shader: GLuint) {
        self.record_es2_unsupported("AttachShader");
    }
    unsafe fn DetachShader(&mut self, _program: GLuint, _shader: GLuint) {
        self.record_es2_unsupported("DetachShader");
    }
    unsafe fn LinkProgram(&mut self, _program: GLuint) {
        self.record_es2_unsupported("LinkProgram");
    }
    unsafe fn UseProgram(&mut self, _program: GLuint) {
        self.record_es2_unsupported("UseProgram");
    }
    unsafe fn GetProgramiv(&mut self, _program: GLuint, _pname: GLenum, _params: *mut GLint) {
        self.record_es2_unsupported("GetProgramiv");
    }
    unsafe fn GetProgramInfoLog(
        &mut self,
        _program: GLuint,
        _maxLength: GLsizei,
        _length: *mut GLsizei,
        _infoLog: *mut GLchar,
    ) {
        self.record_es2_unsupported("GetProgramInfoLog");
    }
    unsafe fn IsProgram(&mut self, _program: GLuint) -> GLboolean {
        self.record_es2_unsupported("IsProgram");
        gles11::FALSE
    }
    unsafe fn ValidateProgram(&mut self, _program: GLuint) {
        self.record_es2_unsupported("ValidateProgram");
    }
    unsafe fn BindAttribLocation(
        &mut self,
        _program: GLuint,
        _index: GLuint,
        _name: *const GLchar,
    ) {
        self.record_es2_unsupported("BindAttribLocation");
    }
    unsafe fn GetAttribLocation(&mut self, _program: GLuint, _name: *const GLchar) -> GLint {
        self.record_es2_unsupported("GetAttribLocation");
        -1
    }
    unsafe fn GetUniformLocation(&mut self, _program: GLuint, _name: *const GLchar) -> GLint {
        self.record_es2_unsupported("GetUniformLocation");
        -1
    }
    unsafe fn GetActiveAttrib(
        &mut self,
        _program: GLuint,
        _index: GLuint,
        _bufSize: GLsizei,
        _length: *mut GLsizei,
        _size: *mut GLint,
        _type_: *mut GLenum,
        _name: *mut GLchar,
    ) {
        self.record_es2_unsupported("GetActiveAttrib");
    }
    unsafe fn GetActiveUniform(
        &mut self,
        _program: GLuint,
        _index: GLuint,
        _bufSize: GLsizei,
        _length: *mut GLsizei,
        _size: *mut GLint,
        _type_: *mut GLenum,
        _name: *mut GLchar,
    ) {
        self.record_es2_unsupported("GetActiveUniform");
    }
    unsafe fn EnableVertexAttribArray(&mut self, _index: GLuint) {
        self.record_es2_unsupported("EnableVertexAttribArray");
    }
    unsafe fn DisableVertexAttribArray(&mut self, _index: GLuint) {
        self.record_es2_unsupported("DisableVertexAttribArray");
    }
    unsafe fn VertexAttribPointer(
        &mut self,
        _index: GLuint,
        _size: GLint,
        _type_: GLenum,
        _normalized: GLboolean,
        _stride: GLsizei,
        _pointer: *const GLvoid,
    ) {
        self.record_es2_unsupported("VertexAttribPointer");
    }
    unsafe fn VertexAttrib1f(&mut self, _index: GLuint, _x: GLfloat) {
        self.record_es2_unsupported("VertexAttrib1f");
    }
    unsafe fn VertexAttrib2f(&mut self, _index: GLuint, _x: GLfloat, _y: GLfloat) {
        self.record_es2_unsupported("VertexAttrib2f");
    }
    unsafe fn VertexAttrib3f(&mut self, _index: GLuint, _x: GLfloat, _y: GLfloat, _z: GLfloat) {
        self.record_es2_unsupported("VertexAttrib3f");
    }
    unsafe fn VertexAttrib4f(
        &mut self,
        _index: GLuint,
        _x: GLfloat,
        _y: GLfloat,
        _z: GLfloat,
        _w: GLfloat,
    ) {
        self.record_es2_unsupported("VertexAttrib4f");
    }
    unsafe fn VertexAttrib1fv(&mut self, _index: GLuint, _v: *const GLfloat) {
        self.record_es2_unsupported("VertexAttrib1fv");
    }
    unsafe fn VertexAttrib2fv(&mut self, _index: GLuint, _v: *const GLfloat) {
        self.record_es2_unsupported("VertexAttrib2fv");
    }
    unsafe fn VertexAttrib3fv(&mut self, _index: GLuint, _v: *const GLfloat) {
        self.record_es2_unsupported("VertexAttrib3fv");
    }
    unsafe fn VertexAttrib4fv(&mut self, _index: GLuint, _v: *const GLfloat) {
        self.record_es2_unsupported("VertexAttrib4fv");
    }
    unsafe fn Uniform1f(&mut self, _location: GLint, _v0: GLfloat) {
        self.record_es2_unsupported("Uniform1f");
    }
    unsafe fn Uniform2f(&mut self, _location: GLint, _v0: GLfloat, _v1: GLfloat) {
        self.record_es2_unsupported("Uniform2f");
    }
    unsafe fn Uniform3f(&mut self, _location: GLint, _v0: GLfloat, _v1: GLfloat, _v2: GLfloat) {
        self.record_es2_unsupported("Uniform3f");
    }
    unsafe fn Uniform4f(
        &mut self,
        _location: GLint,
        _v0: GLfloat,
        _v1: GLfloat,
        _v2: GLfloat,
        _v3: GLfloat,
    ) {
        self.record_es2_unsupported("Uniform4f");
    }
    unsafe fn Uniform1i(&mut self, _location: GLint, _v0: GLint) {
        self.record_es2_unsupported("Uniform1i");
    }
    unsafe fn Uniform2i(&mut self, _location: GLint, _v0: GLint, _v1: GLint) {
        self.record_es2_unsupported("Uniform2i");
    }
    unsafe fn Uniform3i(&mut self, _location: GLint, _v0: GLint, _v1: GLint, _v2: GLint) {
        self.record_es2_unsupported("Uniform3i");
    }
    unsafe fn Uniform4i(
        &mut self,
        _location: GLint,
        _v0: GLint,
        _v1: GLint,
        _v2: GLint,
        _v3: GLint,
    ) {
        self.record_es2_unsupported("Uniform4i");
    }
    unsafe fn Uniform1fv(&mut self, _location: GLint, _count: GLsizei, _value: *const GLfloat) {
        self.record_es2_unsupported("Uniform1fv");
    }
    unsafe fn Uniform2fv(&mut self, _location: GLint, _count: GLsizei, _value: *const GLfloat) {
        self.record_es2_unsupported("Uniform2fv");
    }
    unsafe fn Uniform3fv(&mut self, _location: GLint, _count: GLsizei, _value: *const GLfloat) {
        self.record_es2_unsupported("Uniform3fv");
    }
    unsafe fn Uniform4fv(&mut self, _location: GLint, _count: GLsizei, _value: *const GLfloat) {
        self.record_es2_unsupported("Uniform4fv");
    }
    unsafe fn Uniform1iv(&mut self, _location: GLint, _count: GLsizei, _value: *const GLint) {
        self.record_es2_unsupported("Uniform1iv");
    }
    unsafe fn Uniform2iv(&mut self, _location: GLint, _count: GLsizei, _value: *const GLint) {
        self.record_es2_unsupported("Uniform2iv");
    }
    unsafe fn Uniform3iv(&mut self, _location: GLint, _count: GLsizei, _value: *const GLint) {
        self.record_es2_unsupported("Uniform3iv");
    }
    unsafe fn Uniform4iv(&mut self, _location: GLint, _count: GLsizei, _value: *const GLint) {
        self.record_es2_unsupported("Uniform4iv");
    }
    unsafe fn UniformMatrix2fv(
        &mut self,
        _location: GLint,
        _count: GLsizei,
        _transpose: GLboolean,
        _value: *const GLfloat,
    ) {
        self.record_es2_unsupported("UniformMatrix2fv");
    }
    unsafe fn UniformMatrix3fv(
        &mut self,
        _location: GLint,
        _count: GLsizei,
        _transpose: GLboolean,
        _value: *const GLfloat,
    ) {
        self.record_es2_unsupported("UniformMatrix3fv");
    }
    unsafe fn UniformMatrix4fv(
        &mut self,
        _location: GLint,
        _count: GLsizei,
        _transpose: GLboolean,
        _value: *const GLfloat,
    ) {
        self.record_es2_unsupported("UniformMatrix4fv");
    }
    unsafe fn BlendColor(&mut self, _r: GLclampf, _g: GLclampf, _b: GLclampf, _a: GLclampf) {
        self.record_es2_unsupported("BlendColor");
    }
    unsafe fn BlendEquation(&mut self, _mode: GLenum) {
        self.record_es2_unsupported("BlendEquation");
    }
    unsafe fn BlendEquationSeparate(&mut self, _modeRGB: GLenum, _modeAlpha: GLenum) {
        self.record_es2_unsupported("BlendEquationSeparate");
    }
    unsafe fn BlendFuncSeparate(
        &mut self,
        _srcRGB: GLenum,
        _dstRGB: GLenum,
        _srcAlpha: GLenum,
        _dstAlpha: GLenum,
    ) {
        self.record_es2_unsupported("BlendFuncSeparate");
    }
    unsafe fn StencilFuncSeparate(
        &mut self,
        _face: GLenum,
        _func: GLenum,
        _ref_: GLint,
        _mask: GLuint,
    ) {
        self.record_es2_unsupported("StencilFuncSeparate");
    }
    unsafe fn StencilOpSeparate(
        &mut self,
        _face: GLenum,
        _sfail: GLenum,
        _dpfail: GLenum,
        _dppass: GLenum,
    ) {
        self.record_es2_unsupported("StencilOpSeparate");
    }
    unsafe fn StencilMaskSeparate(&mut self, _face: GLenum, _mask: GLuint) {
        self.record_es2_unsupported("StencilMaskSeparate");
    }
    unsafe fn GetVertexAttribiv(&mut self, _index: GLuint, _pname: GLenum, _params: *mut GLint) {
        self.record_es2_unsupported("GetVertexAttribiv");
    }
    unsafe fn GetVertexAttribfv(&mut self, _index: GLuint, _pname: GLenum, _params: *mut GLfloat) {
        self.record_es2_unsupported("GetVertexAttribfv");
    }
    unsafe fn GetVertexAttribPointerv(
        &mut self,
        _index: GLuint,
        _pname: GLenum,
        _pointer: *mut *mut GLvoid,
    ) {
        self.record_es2_unsupported("GetVertexAttribPointerv");
    }
    unsafe fn GetUniformiv(&mut self, _program: GLuint, _location: GLint, _params: *mut GLint) {
        self.record_es2_unsupported("GetUniformiv");
    }
    unsafe fn GetUniformfv(&mut self, _program: GLuint, _location: GLint, _params: *mut GLfloat) {
        self.record_es2_unsupported("GetUniformfv");
    }
    unsafe fn GetAttachedShaders(
        &mut self,
        _program: GLuint,
        _maxCount: GLsizei,
        _count: *mut GLsizei,
        _shaders: *mut GLuint,
    ) {
        self.record_es2_unsupported("GetAttachedShaders");
    }
    unsafe fn GetShaderSource(
        &mut self,
        _shader: GLuint,
        _bufSize: GLsizei,
        _length: *mut GLsizei,
        _source: *mut GLchar,
    ) {
        self.record_es2_unsupported("GetShaderSource");
    }
    unsafe fn GetShaderPrecisionFormat(
        &mut self,
        _shadertype: GLenum,
        _precisiontype: GLenum,
        _range: *mut GLint,
        _precision: *mut GLint,
    ) {
        self.record_es2_unsupported("GetShaderPrecisionFormat");
    }
    unsafe fn ShaderBinary(
        &mut self,
        _count: GLsizei,
        _shaders: *const GLuint,
        _binaryformat: GLenum,
        _binary: *const GLvoid,
        _length: GLsizei,
    ) {
        self.record_es2_unsupported("ShaderBinary");
    }
    // `glReleaseShaderCompiler` is a hint, not a stateful operation; the
    // default `unsafe fn ReleaseShaderCompiler` in `gles_generic` already
    // returns `()` cleanly so we don't need to override it.
}

impl<'gl_ctx> GLES1Native<'gl_ctx> {
    /// Helper used by every `OpenGL ES 2.0` shader-pipeline override above
    /// to flag a synthetic `GL_INVALID_OPERATION` and emit a one-shot
    /// warning describing the offending call.
    fn record_es2_unsupported(&self, fn_name: &'static str) {
        // The synthetic error must be queued on *every* call, because the
        // ES 2.0 spec requires `GL_INVALID_OPERATION` to be reported each
        // time one of these entry points is hit on an ES 1.1 context.
        self.pending_synthetic_error.set(gles11::INVALID_OPERATION);

        // The accompanying human-readable warning, however, must only be
        // emitted once per distinct entry point. Apps such as Cut the Rope
        // and Angry Birds poll `glGetVertexAttribiv` every frame, which
        // previously flooded the log with thousands of identical lines and
        // added measurable per-frame overhead. Deduplicate via a
        // process-global set keyed by the (`'static`) function name.
        use std::collections::HashSet;
        use std::sync::Mutex;
        use std::sync::OnceLock;
        static LOGGED: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
        let logged = LOGGED.get_or_init(|| Mutex::new(HashSet::new()));
        let is_new = logged.lock().unwrap().insert(fn_name);
        if is_new {
            log!(
                "{} (OpenGL ES 2.0) called on a native ES 1.1 context; \
                 reporting GL_INVALID_OPERATION via glGetError as required \
                 by spec. [this log will only be shown once per entry point]",
                fn_name
            );
        }
    }
}

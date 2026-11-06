/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Utilities for presenting frames to the window using an abstract OpenGL ES
//! implementation.

use super::gles11_raw as gles11; // constants and types only
use super::gles2_raw as gles2;
use super::GLES;
use crate::matrix::Matrix;
use std::time::{Duration, Instant};

pub struct FpsCounter {
    time: std::time::Instant,
    frames: u32,
}
impl FpsCounter {
    pub fn start() -> Self {
        FpsCounter {
            time: Instant::now(),
            frames: 0,
        }
    }

    pub fn count_frame(&mut self, label: std::fmt::Arguments<'_>) {
        self.frames += 1;
        let now = Instant::now();
        let duration = now - self.time;
        if duration >= Duration::from_secs(1) {
            self.time = now;
            echo!(
                "touchHLE: {} FPS: {:.2}",
                label,
                std::mem::take(&mut self.frames) as f32 / duration.as_secs_f32()
            );
        }
    }
}

/// `TOUCHHLE_DS_PRESENT_SHARPEN=<0..2>`: strength of the contrast-adaptive
/// sharpen (a CAS / FSR-RCAS style unsharp mask) applied to the finished frame
/// while it is copied to the window.
///
/// This is the "FSR-like" half of the high-resolution story: let the game
/// render at a lower internal resolution (`TOUCHHLE_DS_SCREEN_SCALE`, which is
/// the expensive knob) so it runs faster, keep the window at the display
/// resolution, and use the present pass to put back the edge contrast the
/// scale-up softened. Unset or `0` (the default) leaves the picture exactly as
/// it was before this feature existed, so it is safe to leave in the tree.
/// Values are clamped to `0..=2`; `1.0` is the "typical" amount.
fn present_sharpen_amount() -> f32 {
    match std::env::var("TOUCHHLE_DS_PRESENT_SHARPEN") {
        Ok(v) => v
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite() && *v > 0.0)
            .map(|v| v.min(2.0))
            .unwrap_or(0.0),
        Err(_) => 0.0,
    }
}

/// Deprecated shadow-lift experiment, retained for comparison only.
/// `TOUCHHLE_DS_SHADOW_BOOST=0..1` defaults to zero (off). Higher values
/// amplify near-black colour quantization without recovering true-black detail.
/// See dev-docs/session-2026-10-26/SHADOW-ENHANCEMENT-DEPRECATED.md.
pub(crate) fn present_shadow_amount() -> f32 {
    std::env::var("TOUCHHLE_DS_SHADOW_BOOST")
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

/// `TOUCHHLE_DS_PRESENT_FSR=1|2`: enable the FSR1 spatial pass.
///
/// This is the same kind of final-image overlay used by desktop tools: a
/// 12-tap EASU pass renders the upscaled frame into an intermediate texture,
/// then an RCAS pass sharpens that result while presenting it to the window.
/// `=2` keeps EASU but skips RCAS for a faster, softer output.
/// It is spatial FSR1, not temporal FSR 2/3 or the ML FSR 4 runtime.
fn present_fsr_mode() -> u8 {
    std::env::var("TOUCHHLE_DS_PRESENT_FSR")
        .ok()
        .and_then(|v| v.trim().parse::<u8>().ok())
        .unwrap_or(0)
        .min(2)
}

/// `TOUCHHLE_DS_AA=1`: enable the final-image FXAA pass.
///
/// This is a real shader pass in the shared Android ES 2.0 context. The old
/// MSAA switch could not provide multisampling through Android's native ES 1.1
/// backend, so FXAA is the portable anti-aliasing option here.
fn present_aa_mode() -> bool {
    std::env::var("TOUCHHLE_DS_AA")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

/// The little shader program used by the present-layer sharpen. Locations are
/// cached with the program; `a_pos`/`a_uv` are bound to slots 6/7 so they never
/// collide with the guest's own attribute layout.
#[derive(Copy, Clone)]
struct PresentSharpenProgram {
    program: u32,
    a_pos: i32,
    a_uv: i32,
    u_tex: i32,
    u_tex_mat: i32,
    u_texel: i32,
    u_sharpen: i32,
    u_shadow: i32,
    u_fsr: i32,
    u_aa: i32,
    u_source_size: i32,
}

/// GL names and failure flags belong to one actual GL context, including FBOs
/// even when contexts share textures/programs. Never share this state between
/// the guest, Core Animation compositor or Android's private shader context.
#[derive(Default)]
pub struct PresentState {
    program: Option<PresentSharpenProgram>,
    shader_failed: bool,
    fsr_target: Option<PresentFsrTarget>,
    fsr_target_failed: bool,
}

impl PresentState {
    /// The owning GL context must be current. Do not call this from Drop:
    /// destruction of a Rust handle does not imply its GL context is current.
    pub(crate) unsafe fn release(&mut self, gles: &mut dyn GLES) {
        if let Some(target) = self.fsr_target.take() {
            gles.DeleteFramebuffersOES(1, &target.framebuffer);
            gles.DeleteTextures(1, &target.texture);
        }
        if let Some(program) = self.program.take() { gles.DeleteProgram(program.program); }
        self.shader_failed = false;
        self.fsr_target_failed = false;
    }
}

#[cfg(all(test, target_os = "windows"))]
#[path = "present_context_tests.rs"]
mod present_context_tests;

#[derive(Copy, Clone)]
struct PresentFsrTarget {
    framebuffer: u32,
    texture: u32,
    width: u32,
    height: u32,
}

/// Reads back a shader/program info log without panicking on a driver that
/// returns a bogus length.
unsafe fn gl_info_log(gles: &mut dyn GLES, obj: u32, is_program: bool) -> String {
    let mut len: i32 = 0;
    let mut buf = [0u8; 1024];
    if is_program {
        gles.GetProgramInfoLog(obj, 1024, &mut len, buf.as_mut_ptr() as *mut _);
    } else {
        gles.GetShaderInfoLog(obj, 1024, &mut len, buf.as_mut_ptr() as *mut _);
    }
    let n = (len.max(0) as usize).min(buf.len());
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// Compiles (once) the present-layer sharpen program.
///
/// The shader is written in GLSL ES 1.00 style on purpose: the ES 1.1 backend
/// (`gles1_on_gl2`) translates ES 1.00 source to desktop GLSL 1.20 for us, and
/// writing desktop GLSL here would confuse that translator.
unsafe fn present_sharpen_program(gles: &mut dyn GLES, state: &mut PresentState) -> Option<PresentSharpenProgram> {
    if let Some(p) = state.program {
        return Some(p);
    }
    if state.shader_failed {
        return None;
    }

    let vs_src = b"\
        attribute vec2 aPos;\n\
        attribute vec2 aUV;\n\
        uniform mat4 uTexMat;\n\
        varying vec2 vUV;\n\
        void main() {\n\
            gl_Position = vec4(aPos, 0.0, 1.0);\n\
            vUV = (uTexMat * vec4(aUV, 0.0, 1.0)).xy;\n\
        }\0";
    let fs_src = b"\
        precision mediump float;\n\
        varying vec2 vUV;\n\
        uniform sampler2D uTex;\n\
        uniform vec2 uTexel;\n\
        uniform vec2 uSourceSize;\n\
        uniform float uSharpen;\n\
        uniform float uShadow;\n\
        uniform float uFsr;\n\
        uniform float uAA;\n\
        vec4 sampleAt(vec2 uv) {\n\
            vec2 lo = uTexel * 0.5;\n\
            vec2 hi = vec2(1.0) - lo;\n\
            return texture2D(uTex, clamp(uv, lo, hi));\n\
        }\n\
        vec4 tap(vec2 o) { return sampleAt(vUV + o * uTexel); }\n\
        float luma2(vec3 c) { return c.b * 0.5 + (c.r * 0.5 + c.g); }\n\
\n\
        // Scalar FSR1 EASU direction estimator. The four calls correspond to\n\
        // the four bilinear cells of the 2x2 footprint.\n\
        void easuSet(inout vec2 dir, inout float len, float w,\n\
                     float lA, float lB, float lC, float lD, float lE) {\n\
            float dc = lD - lC;\n\
            float cb = lC - lB;\n\
            float lenX = 1.0 / max(max(abs(dc), abs(cb)), 1.0 / 32768.0);\n\
            float dirX = lD - lB;\n\
            dir.x += dirX * w;\n\
            lenX = clamp(abs(dirX) * lenX, 0.0, 1.0);\n\
            lenX *= lenX;\n\
            len += lenX * w;\n\
            float ec = lE - lC;\n\
            float ca = lC - lA;\n\
            float lenY = 1.0 / max(max(abs(ec), abs(ca)), 1.0 / 32768.0);\n\
            float dirY = lE - lA;\n\
            dir.y += dirY * w;\n\
            lenY = clamp(abs(dirY) * lenY, 0.0, 1.0);\n\
            lenY *= lenY;\n\
            len += lenY * w;\n\
        }\n\
\n\
        // One of the 12 EASU taps. This is the anisotropic lanczos-2\n\
        // approximation from the FidelityFX FSR1 scalar path.\n\
        void easuTap(inout vec3 aC, inout float aW, vec2 off, vec2 dir,\n\
                     vec2 len2, float lob, float clp, vec3 c) {\n\
            vec2 v;\n\
            v.x = off.x * dir.x + off.y * dir.y;\n\
            v.y = off.x * (-dir.y) + off.y * dir.x;\n\
            v *= len2;\n\
            float d2 = min(v.x * v.x + v.y * v.y, clp);\n\
            float wB = (2.0 / 5.0) * d2 - 1.0;\n\
            float wA = lob * d2 - 1.0;\n\
            wB *= wB;\n\
            wA *= wA;\n\
            wB = (25.0 / 16.0) * wB - (25.0 / 16.0 - 1.0);\n\
            float w = wB * wA;\n\
            aC += c * w;\n\
            aW += w;\n\
        }\n\
\n\
        vec3 easu(vec2 uv) {\n\
            vec2 pp = uv * uSourceSize - vec2(0.5);\n\
            vec2 fp = floor(pp);\n\
            vec2 ppf = pp - fp;\n\
            vec2 inv = uTexel;\n\
            vec2 origin = (fp + vec2(0.5)) * inv;\n\
            vec4 b = sampleAt(origin + vec2(0.0, -1.0) * inv);\n\
            vec4 c = sampleAt(origin + vec2(1.0, -1.0) * inv);\n\
            vec4 e = sampleAt(origin + vec2(-1.0, 0.0) * inv);\n\
            vec4 f = sampleAt(origin);\n\
            vec4 g = sampleAt(origin + vec2(1.0, 0.0) * inv);\n\
            vec4 h = sampleAt(origin + vec2(2.0, 0.0) * inv);\n\
            vec4 i = sampleAt(origin + vec2(-1.0, 1.0) * inv);\n\
            vec4 j = sampleAt(origin + vec2(0.0, 1.0) * inv);\n\
            vec4 k = sampleAt(origin + vec2(1.0, 1.0) * inv);\n\
            vec4 l = sampleAt(origin + vec2(2.0, 1.0) * inv);\n\
            vec4 n = sampleAt(origin + vec2(0.0, 2.0) * inv);\n\
            vec4 o = sampleAt(origin + vec2(1.0, 2.0) * inv);\n\
            float bL = luma2(b.rgb);\n\
            float cL = luma2(c.rgb);\n\
            float eL = luma2(e.rgb);\n\
            float fL = luma2(f.rgb);\n\
            float gL = luma2(g.rgb);\n\
            float hL = luma2(h.rgb);\n\
            float iL = luma2(i.rgb);\n\
            float jL = luma2(j.rgb);\n\
            float kL = luma2(k.rgb);\n\
            float lL = luma2(l.rgb);\n\
            float nL = luma2(n.rgb);\n\
            float oL = luma2(o.rgb);\n\
            vec2 dir = vec2(0.0);\n\
            float len = 0.0;\n\
            easuSet(dir, len, (1.0 - ppf.x) * (1.0 - ppf.y), bL, eL, fL, gL, jL);\n\
            easuSet(dir, len, ppf.x * (1.0 - ppf.y), cL, fL, gL, hL, kL);\n\
            easuSet(dir, len, (1.0 - ppf.x) * ppf.y, fL, iL, jL, kL, nL);\n\
            easuSet(dir, len, ppf.x * ppf.y, gL, jL, kL, lL, oL);\n\
            float dirR = dot(dir, dir);\n\
            if (dirR < 1.0 / 32768.0) {\n\
                dir = vec2(1.0, 0.0);\n\
            } else {\n\
                dir *= inversesqrt(dirR);\n\
            }\n\
            len = len * 0.5;\n\
            len *= len;\n\
            float ax = max(abs(dir.x), abs(dir.y));\n\
            float stretch = dot(dir, dir) / max(ax, 1.0 / 32768.0);\n\
            vec2 len2 = vec2(1.0 + (stretch - 1.0) * len, 1.0 - 0.5 * len);\n\
            float lob = 0.5 + ((1.0 / 4.0 - 0.04) - 0.5) * len;\n\
            float clp = 1.0 / max(lob, 1.0 / 32768.0);\n\
            vec3 min4 = min(min(f.rgb, g.rgb), min(j.rgb, k.rgb));\n\
            vec3 max4 = max(max(f.rgb, g.rgb), max(j.rgb, k.rgb));\n\
            vec3 aC = vec3(0.0);\n\
            float aW = 0.0;\n\
            easuTap(aC, aW, vec2(0.0, -1.0) - ppf, dir, len2, lob, clp, b.rgb);\n\
            easuTap(aC, aW, vec2(1.0, -1.0) - ppf, dir, len2, lob, clp, c.rgb);\n\
            easuTap(aC, aW, vec2(-1.0, 1.0) - ppf, dir, len2, lob, clp, i.rgb);\n\
            easuTap(aC, aW, vec2(0.0, 1.0) - ppf, dir, len2, lob, clp, j.rgb);\n\
            easuTap(aC, aW, vec2(0.0, 0.0) - ppf, dir, len2, lob, clp, f.rgb);\n\
            easuTap(aC, aW, vec2(-1.0, 0.0) - ppf, dir, len2, lob, clp, e.rgb);\n\
            easuTap(aC, aW, vec2(1.0, 1.0) - ppf, dir, len2, lob, clp, k.rgb);\n\
            easuTap(aC, aW, vec2(2.0, 1.0) - ppf, dir, len2, lob, clp, l.rgb);\n\
            easuTap(aC, aW, vec2(2.0, 0.0) - ppf, dir, len2, lob, clp, h.rgb);\n\
            easuTap(aC, aW, vec2(1.0, 0.0) - ppf, dir, len2, lob, clp, g.rgb);\n\
            easuTap(aC, aW, vec2(1.0, 2.0) - ppf, dir, len2, lob, clp, o.rgb);\n\
            easuTap(aC, aW, vec2(0.0, 2.0) - ppf, dir, len2, lob, clp, n.rgb);\n\
            return clamp(aC / max(aW, 1.0 / 32768.0), min4, max4);\n\
        }\n\
\n\
        // RCAS from AMD's FSR1 scalar implementation. Keeping this in the\n\
        // second pass prevents EASU from being fed an already-sharpened image.\n\
        vec3 rcas(vec3 b, vec3 d, vec3 e, vec3 f, vec3 h, float sharpness) {\n\
            float bL = luma2(b);\n\
            float dL = luma2(d);\n\
            float eL = luma2(e);\n\
            float fL = luma2(f);\n\
            float hL = luma2(h);\n\
            float nz = 0.25 * bL + 0.25 * dL + 0.25 * fL + 0.25 * hL - eL;\n\
            float range = max(max(max(bL, dL), max(eL, fL)), hL)\n\
                        - min(min(min(bL, dL), min(eL, fL)), hL);\n\
            nz = clamp(abs(nz) / max(range, 1.0 / 32768.0), 0.0, 1.0);\n\
            nz = -0.5 * nz + 1.0;\n\
            vec3 mn4 = min(min(min(b, d), f), h);\n\
            vec3 mx4 = max(max(max(b, d), f), h);\n\
            vec3 hitMin = min(mn4, e) / max(4.0 * mx4, vec3(1.0 / 32768.0));\n\
            vec3 hitMax = (vec3(1.0) - max(mx4, e))\n\
                        / max(4.0 * mn4 - vec3(4.0), vec3(-1.0 / 32768.0));\n\
            vec3 lobe = max(-vec3(0.25 - 1.0 / 16.0),\n\
                            min(max(-hitMin, hitMax), vec3(0.0)));\n\
            lobe = lobe * nz * sharpness;\n\
            return clamp((e + lobe * (b + d + f + h)) / max(vec3(1.0) + 4.0 * lobe, vec3(1.0 / 32768.0)), 0.0, 1.0);\n\
        }\n\
        // Compact FXAA: luma edge detection plus directional blur. The pass\n\
        // works on either the original frame or the already-upscaled FSR\n\
        // target because uTexel always describes the currently sampled texture.\n\
        vec3 fxaa(vec2 uv) {\n\
            vec4 center = sampleAt(uv);\n\
            float lNW = luma2(tap(vec2(-1.0, -1.0)).rgb);\n\
            float lNE = luma2(tap(vec2( 1.0, -1.0)).rgb);\n\
            float lSW = luma2(tap(vec2(-1.0,  1.0)).rgb);\n\
            float lSE = luma2(tap(vec2( 1.0,  1.0)).rgb);\n\
            float lM = (lNW + lNE + lSW + lSE) * 0.25;\n\
            float lMin = min(lM, min(min(lNW, lNE), min(lSW, lSE)));\n\
            float lMax = max(lM, max(max(lNW, lNE), max(lSW, lSE)));\n\
            if (lMax - lMin < max(0.0312, lMax * 0.125)) {\n\
                return center.rgb;\n\
            }\n\
            vec2 dir = vec2(-((lNW + lNE) - (lSW + lSE)),\n\
                             ((lNW + lSW) - (lNE + lSE)));\n\
            float reduce = max((lNW + lNE + lSW + lSE) * (1.0 / 32.0), 1.0 / 128.0);\n\
            float rcpMin = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);\n\
            dir = clamp(dir * rcpMin, vec2(-8.0), vec2(8.0)) * uTexel;\n\
            vec3 rgbA = 0.5 * (\n\
                sampleAt(uv + dir * (1.0 / 3.0 - 0.5)).rgb +\n\
                sampleAt(uv + dir * (2.0 / 3.0 - 0.5)).rgb\n\
            );\n\
            vec3 rgbB = rgbA * 0.5 + 0.25 * (\n\
                sampleAt(uv - dir * 0.5).rgb +\n\
                sampleAt(uv + dir * 0.5).rgb\n\
            );\n\
            float lB = luma2(rgbB);\n\
            return (lB < lMin || lB > lMax) ? rgbA : rgbB;\n\
        }\n\
        // Deprecated: lifts near-black quantization too; cannot recover true black.\n\
        vec3 shadowLift(vec3 rgb) {\n\
            float peak = max(max(rgb.r, rgb.g), rgb.b);\n\
            if (uShadow <= 0.0 || peak <= 0.0) return rgb;\n\
            float weight = 1.0 - smoothstep(0.02, 0.60, peak);\n\
            float lifted = peak + (sqrt(peak) - peak) * uShadow * weight;\n\
            return rgb * (lifted / peak);\n\
        }\n\
        void main() {\n\
            vec4 c = tap(vec2(0.0));\n\
            if (uFsr < 0.5 && uSharpen <= 0.0001 && uAA < 0.5 && uShadow <= 0.0) {\n\
                gl_FragColor = c;\n\
                return;\n\
            }\n\
            vec4 n = tap(vec2(0.0, -1.0));\n\
            vec4 s = tap(vec2(0.0, 1.0));\n\
            vec4 e = tap(vec2(1.0, 0.0));\n\
            vec4 w = tap(vec2(-1.0, 0.0));\n\
            vec3 outRgb = c.rgb;\n\
            if (uFsr > 2.5) {\n\
                outRgb = c.rgb;\n\
            } else if (uFsr > 1.5) {\n\
                float sharpness = uSharpen > 0.0 ? min(uSharpen, 1.0) : 0.87;\n\
                outRgb = rcas(n.rgb, w.rgb, c.rgb, e.rgb, s.rgb, sharpness);\n\
            } else if (uFsr > 0.5) {\n\
                outRgb = easu(vUV);\n\
            } else {\n\
                vec4 blur = (n + s + e + w) * 0.25;\n\
                vec4 sharp = c + (c - blur) * uSharpen;\n\
                vec3 lo = min(min(n.rgb, s.rgb), min(e.rgb, w.rgb));\n\
                vec3 hi = max(max(n.rgb, s.rgb), max(e.rgb, w.rgb));\n\
                lo = min(lo, c.rgb);\n\
                hi = max(hi, c.rgb);\n\
                outRgb = clamp(sharp.rgb, lo, hi);\n\
            }\n\
            if (uAA > 0.5) {\n\
                outRgb = fxaa(vUV);\n\
            }\n\
            gl_FragColor = vec4(shadowLift(outRgb), c.a);\n\
        }\0";

    let vs = gles.CreateShader(gles2::VERTEX_SHADER);
    let vs_ptr = vs_src.as_ptr() as *const _;
    let vs_len = (vs_src.len() - 1) as i32;
    gles.ShaderSource(vs, 1, &vs_ptr, &vs_len);
    gles.CompileShader(vs);
    let mut ok: i32 = 0;
    gles.GetShaderiv(vs, gles2::COMPILE_STATUS, &mut ok);
    if ok == 0 {
        let msg = gl_info_log(gles, vs, false);
        log!("Warning: present sharpen vertex shader compile failed: {msg}");
        gles.DeleteShader(vs);
        state.shader_failed = true;
        return None;
    }

    let fs = gles.CreateShader(gles2::FRAGMENT_SHADER);
    let fs_ptr = fs_src.as_ptr() as *const _;
    let fs_len = (fs_src.len() - 1) as i32;
    gles.ShaderSource(fs, 1, &fs_ptr, &fs_len);
    gles.CompileShader(fs);
    gles.GetShaderiv(fs, gles2::COMPILE_STATUS, &mut ok);
    if ok == 0 {
        let msg = gl_info_log(gles, fs, false);
        log!("Warning: present sharpen fragment shader compile failed: {msg}");
        gles.DeleteShader(vs);
        gles.DeleteShader(fs);
        state.shader_failed = true;
        return None;
    }

    let program = gles.CreateProgram();
    gles.AttachShader(program, vs);
    gles.AttachShader(program, fs);
    gles.BindAttribLocation(program, 6, b"aPos\0".as_ptr() as *const _);
    gles.BindAttribLocation(program, 7, b"aUV\0".as_ptr() as *const _);
    gles.LinkProgram(program);
    gles.GetProgramiv(program, gles2::LINK_STATUS, &mut ok);
    if ok == 0 {
        let msg = gl_info_log(gles, program, true);
        log!("Warning: present sharpen program link failed: {msg}");
        gles.DeleteShader(vs);
        gles.DeleteShader(fs);
        gles.DeleteProgram(program);
        state.shader_failed = true;
        return None;
    }

    let result = PresentSharpenProgram {
        program,
        a_pos: gles.GetAttribLocation(program, b"aPos\0".as_ptr() as *const _),
        a_uv: gles.GetAttribLocation(program, b"aUV\0".as_ptr() as *const _),
        u_tex: gles.GetUniformLocation(program, b"uTex\0".as_ptr() as *const _),
        u_tex_mat: gles.GetUniformLocation(program, b"uTexMat\0".as_ptr() as *const _),
        u_texel: gles.GetUniformLocation(program, b"uTexel\0".as_ptr() as *const _),
        u_sharpen: gles.GetUniformLocation(program, b"uSharpen\0".as_ptr() as *const _),
        u_shadow: gles.GetUniformLocation(program, b"uShadow\0".as_ptr() as *const _),
        u_fsr: gles.GetUniformLocation(program, b"uFsr\0".as_ptr() as *const _),
        u_aa: gles.GetUniformLocation(program, b"uAA\0".as_ptr() as *const _),
        u_source_size: gles.GetUniformLocation(program, b"uSourceSize\0".as_ptr() as *const _),
    };
    state.program = Some(result);
    gles.DeleteShader(vs);
    gles.DeleteShader(fs);
    log!("[DSpresent-cache] initialized context-owned shader program={}", program);
    Some(result)
}

/// Draws the present quad through the sharpen shader instead of the
/// fixed-function pipeline. Returns `false` when the shader is unavailable, in
/// which case the caller must fall back to the normal path.
///
/// `source_size` is the size of the texture bound to unit 0 (`GL_TEXTURE_2D`),
/// so one texel step is `1/size` — the sample offsets must be in *source* texel
/// units, not in output/window units, or the mask would change with the
/// scale-up factor.
pub(crate) unsafe fn draw_present_sharpen_quad(
    gles: &mut dyn GLES,
    state: &mut PresentState,
    tex_mat: &Matrix<4>,
    source_size: (u32, u32),
    amount: f32,
    fsr: f32,
    aa: f32,
) -> bool {
    let Some(p) = present_sharpen_program(gles, state) else {
        return false;
    };
    if p.a_pos < 0 || p.a_uv < 0 || p.u_tex < 0 || p.u_tex_mat < 0 {
        // The linker threw away something we need (no driver should, but a
        // program that lost its uniforms would silently do nothing).
        log_once!("Warning: present sharpen program is missing an attribute/uniform; disabled");
        state.shader_failed = true;
        return false;
    }
    if fsr > 0.0 && p.u_fsr < 0 {
        log_once!("Warning: present program has no FSR uniform; EASU pass not submitted");
        return false;
    }

    // Same texture-unit convention as the ES 2.0 presenter: sample unit 0.
    gles.ActiveTexture(gles2::TEXTURE0);
    let mut old_program: i32 = 0;
    gles.GetIntegerv(gles2::CURRENT_PROGRAM, &mut old_program);
    gles.UseProgram(p.program);
    gles.Uniform1i(p.u_tex, 0);
    if p.u_tex_mat >= 0 {
        gles.UniformMatrix4fv(
            p.u_tex_mat,
            1,
            gles2::FALSE,
            tex_mat.columns().as_ptr() as *const _,
        );
    }
    if p.u_texel >= 0 {
        gles.Uniform2f(
            p.u_texel,
            1.0 / source_size.0 as f32,
            1.0 / source_size.1 as f32,
        );
    }
    if p.u_source_size >= 0 {
        gles.Uniform2f(p.u_source_size, source_size.0 as f32, source_size.1 as f32);
    }
    if p.u_sharpen >= 0 {
        gles.Uniform1f(p.u_sharpen, amount);
    }
    if p.u_fsr >= 0 {
        gles.Uniform1f(p.u_fsr, fsr);
    }
    if p.u_shadow >= 0 {
        // EASU first renders an intermediate texture; apply the enhancement
        // once, in the final RCAS/copy pass. The fast direct path below is
        // disabled when shadow lift needs that final pass.
        let shadow = if fsr > 0.5 && fsr < 1.5 { 0.0 } else { present_shadow_amount() };
        gles.Uniform1f(p.u_shadow, shadow);
    }
    if p.u_aa >= 0 {
        gles.Uniform1f(p.u_aa, aa);
    }

    let vertices: [f32; 12] = [
        -1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0,
    ];
    let tex_coords: [f32; 12] = [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    // Client-side arrays, exactly like the fixed-function quad: present_frame
    // binds ARRAY_BUFFER 0 before it draws.
    gles.BindBuffer(gles11::ARRAY_BUFFER, 0);
    gles.EnableVertexAttribArray(p.a_pos as u32);
    gles.VertexAttribPointer(
        p.a_pos as u32,
        2,
        gles2::FLOAT,
        gles2::FALSE,
        0,
        vertices.as_ptr() as *const _,
    );
    gles.EnableVertexAttribArray(p.a_uv as u32);
    gles.VertexAttribPointer(
        p.a_uv as u32,
        2,
        gles2::FLOAT,
        gles2::FALSE,
        0,
        tex_coords.as_ptr() as *const _,
    );
    gles.DrawArrays(gles2::TRIANGLES, 0, 6);
    gles.DisableVertexAttribArray(p.a_pos as u32);
    gles.DisableVertexAttribArray(p.a_uv as u32);
    // The present paths on ES 1.1 intentionally don't save/restore the current
    // program (nothing there binds one), so we must put back whatever was
    // bound before our pass — otherwise we'd leak our program into the guest.
    gles.UseProgram(old_program as u32);

    {
        static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log!(
                "[DSsharpen] present pass active: amount={} fsr={} source={}x{} (texel={}, {})",
                amount,
                fsr,
                source_size.0,
                source_size.1,
                1.0 / source_size.0 as f32,
                1.0 / source_size.1 as f32
            );
        }
    }
    true
}

/// Lazily creates (or resizes) the intermediate texture used by the two-pass
/// FSR present overlay. The texture is allocated at the final host viewport
/// size, so the first pass performs EASU at the display resolution and the
/// second pass runs RCAS over that already-upscaled image.
unsafe fn ensure_present_fsr_target(
    gles: &mut dyn GLES,
    state: &mut PresentState,
    width: u32,
    height: u32,
) -> Option<PresentFsrTarget> {
    if width == 0 || height == 0 || state.fsr_target_failed {
        return None;
    }

    if let Some(target) = state.fsr_target {
        if target.width == width && target.height == height {
            return Some(target);
        }
    }

    let mut old_fbo: i32 = 0;
    let mut old_texture: i32 = 0;
    gles.GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut old_fbo);
    gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut old_texture);

    if let Some(old) = state.fsr_target.take() {
        let old_fbos = [old.framebuffer];
        let old_textures = [old.texture];
        gles.DeleteFramebuffersOES(1, old_fbos.as_ptr());
        gles.DeleteTextures(1, old_textures.as_ptr());
    }

    let mut texture: gles11::types::GLuint = 0;
    gles.GenTextures(1, &mut texture);
    gles.BindTexture(gles11::TEXTURE_2D, texture);
    gles.TexImage2D(
        gles11::TEXTURE_2D,
        0,
        gles11::RGBA as _,
        width as _,
        height as _,
        0,
        gles11::RGBA,
        gles11::UNSIGNED_BYTE,
        std::ptr::null(),
    );
    gles.TexParameteri(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_MIN_FILTER,
        gles11::LINEAR as _,
    );
    gles.TexParameteri(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_MAG_FILTER,
        gles11::LINEAR as _,
    );
    gles.TexParameteri(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_WRAP_S,
        gles11::CLAMP_TO_EDGE as _,
    );
    gles.TexParameteri(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_WRAP_T,
        gles11::CLAMP_TO_EDGE as _,
    );

    let mut framebuffer: gles11::types::GLuint = 0;
    gles.GenFramebuffersOES(1, &mut framebuffer);
    gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, framebuffer);
    gles.FramebufferTexture2DOES(
        gles11::FRAMEBUFFER_OES,
        gles11::COLOR_ATTACHMENT0_OES,
        gles11::TEXTURE_2D,
        texture,
        0,
    );
    let status = gles.CheckFramebufferStatusOES(gles11::FRAMEBUFFER_OES);

    if status != gles11::FRAMEBUFFER_COMPLETE_OES {
        log!(
            "Warning: FSR present target {}x{} incomplete (status {:#x}); falling back to single-pass present",
            width,
            height,
            status
        );
        let textures = [texture];
        let framebuffers = [framebuffer];
        gles.DeleteTextures(1, textures.as_ptr());
        gles.DeleteFramebuffersOES(1, framebuffers.as_ptr());
        gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, old_fbo as _);
        gles.BindTexture(gles11::TEXTURE_2D, old_texture as _);
        state.fsr_target_failed = true;
        return None;
    }

    let target = PresentFsrTarget {
        framebuffer,
        texture,
        width,
        height,
    };
    state.fsr_target = Some(target);
    log!("[DSpresent-cache] initialized context-owned FSR target={} texture={} size={}x{}", framebuffer, texture, width, height);
    gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, old_fbo as _);
    gles.BindTexture(gles11::TEXTURE_2D, old_texture as _);
    Some(target)
}

/// Optional final-window capture for A/B testing the complete present stack.
///
/// `TOUCHHLE_DS_PRESENT_CAPTURE=<path.ppm>` writes the backbuffer after EASU,
/// RCAS, cursor and debug overlays have all been drawn. Unlike the F7 frame
/// dump this includes the present-layer image processing. Set
/// `TOUCHHLE_DS_PRESENT_CAPTURE_AT` (default 600) chooses the frame number, or
/// `TOUCHHLE_DS_PRESENT_CAPTURE_AFTER_MS` chooses a wall-clock delay instead.
unsafe fn maybe_capture_present_backbuffer(gles: &mut dyn GLES, viewport: (u32, u32, u32, u32)) {
    let Ok(path) = std::env::var("TOUCHHLE_DS_PRESENT_CAPTURE") else {
        return;
    };
    if path.trim().is_empty() || viewport.2 == 0 || viewport.3 == 0 {
        return;
    }

    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    static CAPTURED: AtomicBool = AtomicBool::new(false);
    static FRAME: AtomicU32 = AtomicU32::new(0);
    let frame = FRAME.fetch_add(1, Ordering::Relaxed);
    let at = std::env::var("TOUCHHLE_DS_PRESENT_CAPTURE_AT")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(600);
    let trigger=std::env::var("TOUCHHLE_DS_PRESENT_CAPTURE_TRIGGER").ok().map(std::path::PathBuf::from);
    let manual=trigger.as_ref().is_some_and(|p|p.is_file());
    let due = if trigger.is_some() {
        manual
    } else if let Ok(value) = std::env::var("TOUCHHLE_DS_PRESENT_CAPTURE_AFTER_MS") {
        let after_ms = value.trim().parse::<u64>().unwrap_or(0);
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis() as u64
            >= after_ms
    } else {
        frame == at
    };
    if !due || (!manual && CAPTURED.swap(true, Ordering::Relaxed)) {
        return;
    }
    if manual {if let Some(p)=trigger {let _=std::fs::remove_file(p);}}

    let width = viewport.2 as usize;
    let height = viewport.3 as usize;
    let mut rgba = vec![0u8; width * height * 4];
    gles.ReadPixels(
        viewport.0 as _,
        viewport.1 as _,
        viewport.2 as _,
        viewport.3 as _,
        gles11::RGBA,
        gles11::UNSIGNED_BYTE,
        rgba.as_mut_ptr() as *mut _,
    );

    let mut ppm = Vec::with_capacity(32 + width * height * 3);
    ppm.extend_from_slice(format!("P6\n{} {}\n255\n", width, height).as_bytes());
    for y in (0..height).rev() {
        for x in 0..width {
            let i = (y * width + x) * 4;
            ppm.extend_from_slice(&rgba[i..i + 3]);
        }
    }
    match std::fs::write(&path, ppm) {
        Ok(()) => log!("[DSpresent] captured final frame {} at {}", frame, path),
        Err(err) => log!(
            "Warning: failed to capture present frame {} to {}: {}",
            frame,
            path,
            err
        ),
    }
}

/// Present the the latest frame (e.g. the app's splash screen or rendering
/// output), provided as a texture bound to `GL_TEXTURE_2D`, by drawing it on
/// the window. It may be rotated, scaled and/or letterboxed as necessary. The
/// virtual cursor is also drawn if it should be currently visible.
///
/// The provided context must be current.
///
/// `source_size` is the size, in texels, of the frame texture bound to
/// `GL_TEXTURE_2D` on unit 0 — i.e. the image `present_frame` is about to
/// scale/rotate to the window. It is only needed for the optional
/// present-layer sharpen (`TOUCHHLE_DS_PRESENT_SHARPEN`); `None` disables it
/// for this call, which is exactly the pre-existing behaviour.
pub unsafe fn present_frame(
    gles: &mut dyn GLES,
    state: &mut PresentState,
    viewport: (u32, u32, u32, u32),
    rotation_matrix: Matrix<2>,
    virtual_cursor_visible_at: Option<(f32, f32, bool)>,
    source_size: Option<(u32, u32)>,
    text_overlay: Option<&mut crate::ds_host_overlay::GpuState>,
) -> bool {
    // While this is a generic utility, it is closely tied to
    // crate::frameworks::opengles::eagl::present_renderbuffer, which handles
    // backing up and restoring OpenGL ES state that this function might touch,
    // so these need to be updated in tandem.

    use gles11::types::*;

    // Draw the quad
    gles.Viewport(
        viewport.0 as _,
        viewport.1 as _,
        viewport.2 as _,
        viewport.3 as _,
    );
    gles.ClearColor(0.0, 0.0, 0.0, 1.0);
    // PERF: only the color buffer needs clearing here. The present quad runs
    // with depth/stencil testing disabled (the caller disables caps before
    // drawing), so clearing DEPTH|STENCIL is a wasted full-viewport pass —
    // noticeable at scale-hack 4x on software rasterizers.
    gles.Clear(gles11::COLOR_BUFFER_BIT);
    gles.BindBuffer(gles11::ARRAY_BUFFER, 0);
    // Stretch the full rendered frame to fill the active host viewport.
    //
    // This does NOT crop or shift the texture. The whole renderbuffer is
    // sampled from normal 0..1 texture coordinates and mapped to a full-screen
    // quad. This is the correct "fill the current window" behavior for
    // PotatoGold-style landscape tests.
    if std::env::var_os("TOUCHHLE_PRESENT_STRETCH_TO_VIEWPORT").is_some() {
        log_once!(
            "TOUCHHLE_PRESENT_STRETCH_TO_VIEWPORT=1: stretching full rendered frame to the active viewport [this log will only be shown once]"
        );
    }

    let vertices: [f32; 12] = [
        -1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0,
    ];
    gles.EnableClientState(gles11::VERTEX_ARRAY);
    gles.VertexPointer(2, gles11::FLOAT, 0, vertices.as_ptr() as *const GLvoid);

    let tex_coords: [f32; 12] = [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    gles.EnableClientState(gles11::TEXTURE_COORD_ARRAY);
    gles.TexCoordPointer(2, gles11::FLOAT, 0, tex_coords.as_ptr() as *const GLvoid);
    // Apply the device-rotation matrix to the TEXTURE matrix, but rotate
    // around the centre of the tex coord square (0.5, 0.5) instead of the
    // origin. The naive `LoadMatrixf(rotation_matrix)` rotates around (0, 0),
    // which sends standard [0, 1]² UVs out of range — e.g. for a 90° rotation
    // (v, -u) reaches v' = -u ∈ [-1, 0]. On lenient drivers (Mesa, Apple
    // PowerVR) GL_REPEAT wrap quietly maps that back into [0, 1], but
    // strict drivers (Qualcomm Adreno's native ES 1.1 path) treat the
    // resulting sample of an NPOT texture (renderbuffer is typically
    // 320x480) as undefined and produce a mangled / black presented
    // frame. Pre- and post-translating by (0.5, 0.5) keeps tex coords in
    // [0, 1]² for any 90°/180°/270°/identity device rotation while
    // producing the same visual output as before on lenient drivers.
    let r = Matrix::<4>::from(&rotation_matrix);
    let to_center = Matrix::<4>::translate_3d(-0.5, -0.5, 0.0);
    let from_center = Matrix::<4>::translate_3d(0.5, 0.5, 0.0);
    // Note: Matrix::multiply(&other) computes `other · self` in
    // linear-algebra terms (other is applied AFTER self), so to get
    // `from_center · r · to_center` we chain in the order
    // to_center.multiply(&r).multiply(&from_center).
    let centered_rotation = to_center.multiply(&r).multiply(&from_center);
    // Optional contrast-adaptive sharpening of the finished frame (see
    // `present_sharpen_amount`). Unlike the fixed-function path this samples
    // the frame texture directly, so the caller's `GL_TEXTURE_ENV` mode and
    // texture matrix are irrelevant; the rotation is baked into `uTexMat`.
    let solid_color = std::env::var_os("TOUCHHLE_PRESENT_SOLID_COLOR").is_some();
    // The native Android ES 1.1 backend deliberately has no shader
    // entry points. The Android-only present helper creates a small shared
    // ES 2.0 context for these passes instead, so don't attempt (and fail)
    // shader compilation in the guest ES 1.1 context itself.
    let shader_capable = gles.is_es2() || !gles.is_native_es1();
    let sharpen_amount = match source_size {
        Some((w, h)) if shader_capable && w > 0 && h > 0 => present_sharpen_amount(),
        _ => 0.0,
    };
    let fsr_mode = present_fsr_mode();
    let fsr_source_redundant = source_size.is_some_and(|(w, h)| {
        w > 0
            && h > 0
            && viewport.2 > 0
            && viewport.3 > 0
            && w as u64 * h as u64 * 100
                >= viewport.2 as u64 * viewport.3 as u64 * 115
    });
    if fsr_mode != 0 && fsr_source_redundant {
        log_once!(
            "[DSsharpen] FSR disabled: source is already >=115% of viewport area"
        );
    }
    let fsr = fsr_mode != 0 && shader_capable && !fsr_source_redundant;
    let aa = present_aa_mode() && shader_capable;
    let shadow = if shader_capable { present_shadow_amount() } else { 0.0 };
    let mut sharpened = false;
    let mut fsr_final_submitted = false;

    // EASU's "fast" mode has no second RCAS dependency. Draw it directly to
    // the window unless FXAA also needs a post-EASU source texture.
    if fsr && fsr_mode >= 2 && !aa && shadow == 0.0 && !solid_color && viewport.2 > 0 && viewport.3 > 0 {
        if let Some(size) = source_size {
            sharpened =
                draw_present_sharpen_quad(gles, state, &centered_rotation, size, sharpen_amount, 1.0, 0.0);
            fsr_final_submitted = sharpened;
        }
    }

    if !sharpened && fsr && !solid_color && viewport.2 > 0 && viewport.3 > 0 {
        if let Some(size) = source_size {
            let mut old_fbo: i32 = 0;
            gles.GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut old_fbo);
            let mut old_texture: i32 = 0;
            gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut old_texture);

            if let Some(target) = ensure_present_fsr_target(gles, state, viewport.2, viewport.3) {
                // First pass: source texture -> EASU -> final-sized texture.
                gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, target.framebuffer);
                gles.Viewport(0, 0, target.width as _, target.height as _);
                gles.ClearColor(0.0, 0.0, 0.0, 1.0);
                gles.Clear(gles11::COLOR_BUFFER_BIT);
                let first = draw_present_sharpen_quad(
                    gles,
                    state,
                    &centered_rotation,
                    size,
                    sharpen_amount,
                    1.0,
                    0.0,
                );

                // Second pass: EASU texture -> RCAS (quality mode) or copy
                // (fast mode), optionally followed by FXAA. This pass always
                // samples the final-sized intermediate so FXAA works on
                // reconstructed pixels rather than on the low-resolution input.
                gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, old_fbo as _);
                gles.Viewport(
                    viewport.0 as _,
                    viewport.1 as _,
                    viewport.2 as _,
                    viewport.3 as _,
                );
                gles.ClearColor(0.0, 0.0, 0.0, 1.0);
                gles.Clear(gles11::COLOR_BUFFER_BIT);
                gles.BindTexture(gles11::TEXTURE_2D, target.texture);
                let identity = Matrix::<4>::identity();
                let second = draw_present_sharpen_quad(
                    gles,
                    state,
                    &identity,
                    (target.width, target.height),
                    sharpen_amount,
                    if fsr_mode >= 2 { 3.0 } else { 2.0 },
                    if aa { 1.0 } else { 0.0 },
                );
                gles.BindTexture(gles11::TEXTURE_2D, old_texture as _);
                gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, old_fbo as _);
                sharpened = first && second;
                fsr_final_submitted = sharpened;
            }
        }
    }
    if !sharpened && (sharpen_amount > 0.0 || aa || shadow > 0.0) && !solid_color {
        if let Some(size) = source_size {
            sharpened = draw_present_sharpen_quad(
                gles,
                state,
                &centered_rotation,
                size,
                sharpen_amount,
                0.0,
                if aa { 1.0 } else { 0.0 },
            );
        }
    }
    if !sharpened {
        gles.MatrixMode(gles11::TEXTURE);
        gles.LoadMatrixf(centered_rotation.columns().as_ptr() as *const _);
        if solid_color {
            // Diagnostic: draw the present quad without texturing, in red, to
            // tell "the quad draw is broken" apart from "the present texture is
            // empty".
            log_once!("TOUCHHLE_PRESENT_SOLID_COLOR=1: drawing present quad untextured (red)");
            gles.Disable(gles11::TEXTURE_2D);
            gles.Color4f(1.0, 0.0, 0.0, 1.0);
        } else {
            gles.Enable(gles11::TEXTURE_2D);
        }
        gles.DrawArrays(gles11::TRIANGLES, 0, 6);
    }
    if std::env::var_os("TOUCHHLE_DS_PRESENT_PROBE").is_some() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        if n % 120 == 119 {
            // Three-by-three final-backbuffer sample. The centre alone cannot
            // distinguish a valid frame from a large black/empty region, and
            // corners are useful when checking rotation and viewport origin.
            let mut grid = [[0u8; 4]; 9];
            let positions = [0.125f32, 0.5, 0.875];
            let mut i = 0;
            for fy in positions {
                for fx in positions {
                    let x = viewport.0 as f32 + (viewport.2.saturating_sub(1) as f32 * fx);
                    let y = viewport.1 as f32 + (viewport.3.saturating_sub(1) as f32 * fy);
                    gles.ReadPixels(
                        x.round() as i32,
                        y.round() as i32,
                        1,
                        1,
                        gles11::RGBA,
                        gles11::UNSIGNED_BYTE,
                        grid[i].as_mut_ptr() as *mut _,
                    );
                    i += 1;
                }
            }
            log!(
                "[DSpresent] probe frame={} grid={:?} fsr={} sharpen={}",
                n,
                grid,
                fsr_mode,
                sharpen_amount
            );
        }
    }
    // clean this up so we don't need to worry about it in e.g. Core Animation
    gles.MatrixMode(gles11::TEXTURE);
    gles.LoadIdentity();

    // Dead Space debug aid: when `TOUCHHLE_DS_STICK_OVERLAY=x,y,w,h` is set,
    // draw the virtual-stick rectangle the keyboard drives (see
    // `--stick-to-touch`), so the area can be calibrated visually instead of
    // guessed. Coordinates are in logical iOS points; the logical screen size
    // can be overridden with `TOUCHHLE_DS_LOGICAL_SCREEN=w,h` (default 480x320,
    // i.e. landscape iPhone).
    if let Ok(spec) = std::env::var("TOUCHHLE_DS_STICK_OVERLAY") {
        let n: Vec<f32> = spec
            .split(',')
            .filter_map(|v| v.trim().parse().ok())
            .collect();
        if n.len() == 4 {
            let (ls_w, ls_h) = std::env::var("TOUCHHLE_DS_LOGICAL_SCREEN")
                .ok()
                .and_then(|v| {
                    let p: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    if p.len() == 2 && p[0] > 0.0 && p[1] > 0.0 {
                        Some((p[0], p[1]))
                    } else {
                        None
                    }
                })
                .unwrap_or((480.0, 320.0));
            log!(
                "[DS overlay] stick rect {} {} {} {} (logical {}x{})",
                n[0],
                n[1],
                n[2],
                n[3],
                ls_w,
                ls_h
            );
            let ndc_x = |x: f32| x / ls_w * 2.0 - 1.0;
            let ndc_y = |y: f32| 1.0 - y / ls_h * 2.0;
            let x0 = ndc_x(n[0]);
            let y0 = ndc_y(n[1]);
            let x1 = ndc_x(n[0] + n[2]);
            let y1 = ndc_y(n[1] + n[3]);
            let rect: [f32; 12] = [x0, y0, x1, y0, x1, y1, x1, y1, x0, y1, x0, y0];
            gles.DisableClientState(gles11::TEXTURE_COORD_ARRAY);
            gles.Disable(gles11::TEXTURE_2D);
            gles.Disable(gles11::DEPTH_TEST);
            gles.Enable(gles11::BLEND);
            gles.BlendFunc(gles11::SRC_ALPHA, gles11::ONE_MINUS_SRC_ALPHA);
            gles.Color4f(1.0, 0.2, 0.2, 0.30);
            gles.VertexPointer(2, gles11::FLOAT, 0, rect.as_ptr() as *const GLvoid);
            gles.DrawArrays(gles11::TRIANGLES, 0, 6);
            gles.Color4f(1.0, 1.0, 1.0, 1.0);
            gles.Disable(gles11::BLEND);
        }
    }

    // Display virtual cursor
    let frontend_pointer = crate::ds_frontend_controller::capture_menu();
    if let Some((x, y, pressed)) = virtual_cursor_visible_at.filter(|_| !crate::window::ds_native_pointer_enabled() && !frontend_pointer) {
        let (vx, vy, vw, vh) = viewport;
        let x = x - vx as f32;
        let y = y - vy as f32;

        gles.DisableClientState(gles11::TEXTURE_COORD_ARRAY);
        gles.Disable(gles11::TEXTURE_2D);

        gles.Enable(gles11::BLEND);
        gles.BlendFunc(gles11::ONE, gles11::ONE_MINUS_SRC_ALPHA);
        gles.Color4f(0.0, 0.0, 0.0, if pressed { 2.0 / 3.0 } else { 1.0 / 3.0 });

        let radius = 10.0;

        let mut vertices = vertices;
        for i in (0..vertices.len()).step_by(2) {
            vertices[i] = (vertices[i] * radius + x) / (vw as f32 / 2.0) - 1.0;
            vertices[i + 1] = 1.0 - (vertices[i + 1] * radius + y) / (vh as f32 / 2.0);
        }
        gles.VertexPointer(2, gles11::FLOAT, 0, vertices.as_ptr() as *const GLvoid);
        gles.DrawArrays(gles11::TRIANGLES, 0, 6);
    }
    #[cfg(target_os="windows")]
    let mut gpu_text_selected = false;
    #[cfg(target_os="windows")]
    if let (Some(state), Some(size)) = (text_overlay, source_size) {
        state.draw(gles, size, rotation_matrix);
        // CA/forced CPU composition passes no GPU state. Availability alone
        // must not claim that this frame selected the GPU card path.
        gpu_text_selected = crate::ds_host_overlay::gpu_text_gate_enabled();
    }
    #[cfg(not(target_os="windows"))]
    let _ = text_overlay;
    #[cfg(target_os="windows")]
    crate::ds_monitor::draw(gles,viewport,source_size,crate::ds_monitor::PresentStatus {
        fsr_requested: fsr_mode, fsr_eligible: fsr,
        fsr_final_submitted, shader_submitted: sharpened,
        source_redundant: fsr_source_redundant, gpu_text_selected,
    });
    if crate::window::ds_native_pointer_enabled() || frontend_pointer {
        if let Some(at)=virtual_cursor_visible_at {
            gles.DisableClientState(gles11::TEXTURE_COORD_ARRAY);
            gles.Disable(gles11::TEXTURE_2D);
            gles.Disable(gles11::BLEND);
            gles.EnableClientState(gles11::VERTEX_ARRAY);
            let(vertices,color)=crate::ds_pointer::backing(viewport,at);
            gles.Color4f(color[0],color[1],color[2],color[3]);
            gles.VertexPointer(2,gles11::FLOAT,0,vertices.as_ptr().cast());
            gles.DrawArrays(gles11::TRIANGLES,0,12);
            for (vertices,color) in crate::ds_pointer::layers(viewport,at) {
                gles.Color4f(color[0],color[1],color[2],color[3]);
                gles.VertexPointer(2,gles11::FLOAT,0,vertices.as_ptr().cast());
                gles.DrawArrays(gles11::TRIANGLES,0,12);
            }
        }
    }
    let frontend_overlay = crate::ds_frontend_controller::overlay();
    if !frontend_overlay.is_empty() {
        gles.DisableClientState(gles11::TEXTURE_COORD_ARRAY);
        gles.Disable(gles11::TEXTURE_2D);
        gles.Disable(gles11::BLEND);
        gles.EnableClientState(gles11::VERTEX_ARRAY);
        let mut vertices = Vec::with_capacity(frontend_overlay.len() * 12);
        let mut colors = Vec::with_capacity(frontend_overlay.len() * 24);
        for (quad, color) in frontend_overlay {
            vertices.extend_from_slice(&quad);
            // Twelve coordinates describe six 2D vertices. Keep the color
            // stream aligned with those vertices across every primitive.
            for _ in 0..6 {
                colors.extend_from_slice(&color);
            }
        }
        gles.EnableClientState(gles11::COLOR_ARRAY);
        gles.ColorPointer(4, gles11::FLOAT, 0, colors.as_ptr().cast());
        gles.VertexPointer(2, gles11::FLOAT, 0, vertices.as_ptr().cast());
        gles.DrawArrays(gles11::TRIANGLES, 0, (vertices.len() / 2) as _);
        gles.DisableClientState(gles11::COLOR_ARRAY);
        gles.Color4f(1.0, 1.0, 1.0, 1.0);
    }
    super::ds_input_prompts::draw(gles);
    crate::ds_diagnostics::capture_problem_frame(gles,viewport);
    maybe_capture_present_backbuffer(gles, viewport);
    sharpened
}

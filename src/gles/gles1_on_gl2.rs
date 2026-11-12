/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Implementation of OpenGL ES 1.1 on top of OpenGL 2.1 compatibility profile.
//!
//! The standard graphics drivers on most desktop operating systems do not
//! provide OpenGL ES 1.1, so we must provide it ourselves somehow.
//!
//! OpenGL ES 1.1 is based on OpenGL 1.5. Much of its core functionality (e.g.
//! the fixed-function pipeline) is considered legacy and not available in the
//! "core profile" for modern OpenGL versions, nor is it available at all in
//! later versions of OpenGL ES. However, OpenGL also has the "compatibility
//! profile" which still offers this legacy functionality.
//!
//! OpenGL 2.1 is the latest version that has a compatibility profile available
//! on macOS. It's also a version supported on various other OSes.
//! It is therefore a convenient target for our implementation.

#[path = "ds_blood_size.rs"]
mod ds_blood_size;
#[path = "ds_opening_fog.rs"]
mod ds_opening_fog;
#[path = "gpu_profile.rs"]
mod gpu_profile;
#[path = "ds_gl_state_cache.rs"]
mod ds_gl_state_cache;

#[path = "trace_file.rs"]
mod trace_file;
/// Test-inbox-only replay of pre-gate formatting; normal processes stay lazy.
pub(crate) fn set_matrix_trace_format_eager(eager: bool) {
    trace_file::set_eager_for_private_probe(eager);
}

use super::gl21compat_raw as gl21;
use super::gl21compat_raw::types::*;
use super::gles11_raw as gles11; // constants only
use super::gles_generic::GLES;
use super::util::{
    fixed_to_float, float_to_fixed, matrix_fixed_to_float, try_decode_pvrtc, PalettedTextureFormat,
    ParamTable, ParamType,
};
use super::GLESContext;
use crate::window::{GLContext, GLVersion, Window};
use std::collections::{HashMap, HashSet};
use std::ffi::CStr;

// Count every actual backend readback, including internal texture conversion.
// Same calls/arguments/return path; no new GL reads. One scope per call.
unsafe fn ds_observed_read_pixels(x:GLint,y:GLint,width:GLsizei,height:GLsizei,
    format:GLenum,type_:GLenum,pixels:*mut GLvoid){
    let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::ReadPixels,
        width.max(0) as u64*height.max(0) as u64,((format as u64)<<32)|type_ as u64);
    gl21::ReadPixels(x,y,width,height,format,type_,pixels);
}

/// Monitor-only desktop query. NVX values describe whole-adapter memory,
/// not this process's allocation. Unsupported drivers report no value.
#[cfg(target_os = "windows")]
pub(crate) unsafe fn ds_monitor_gpu() -> (String, Option<(i32,i32)>) {
    let renderer=gl21::GetString(gl21::RENDERER);
    let name=if renderer.is_null(){"unavailable".to_owned()}else{CStr::from_ptr(renderer as *const _).to_string_lossy().into_owned()};
    let software=name.to_ascii_lowercase();
    if ["llvmpipe","softpipe","software","swiftshader"].iter().any(|s|software.contains(s)){return (name,None)}
    let extensions=gl21::GetString(gl21::EXTENSIONS);
    if extensions.is_null() || !CStr::from_ptr(extensions as *const _).to_string_lossy().split_whitespace().any(|s|s=="GL_NVX_gpu_memory_info") {return (name,None)}
    let (mut total,mut available)=(0,0);
    gl21::GetIntegerv(0x9048,&mut total);
    gl21::GetIntegerv(0x904b,&mut available);
    (name,(total>0&&available>=0&&available<=total).then_some((total/1024,available/1024)))
}

/// List of capabilities shared by OpenGL ES 1.1 and OpenGL 2.1.
///
/// Note: There can be arbitrarily many lights or clip planes, depending on
/// implementation limits. We might eventually need to check those rather than
/// just providing the minimum.
pub const CAPABILITIES: &[GLenum] = &[
    gl21::ALPHA_TEST,
    gl21::BLEND,
    gl21::COLOR_LOGIC_OP,
    gl21::CLIP_PLANE0,
    gl21::CLIP_PLANE1,
    gl21::CLIP_PLANE2,
    gl21::CLIP_PLANE3,
    gl21::CLIP_PLANE4,
    gl21::CLIP_PLANE5,
    gl21::LIGHT0,
    gl21::LIGHT1,
    gl21::LIGHT2,
    gl21::LIGHT3,
    gl21::LIGHT4,
    gl21::LIGHT5,
    gl21::LIGHT6,
    gl21::LIGHT7,
    gl21::COLOR_MATERIAL,
    gl21::CULL_FACE,
    gl21::DEPTH_TEST,
    gl21::DITHER,
    gl21::FOG,
    gl21::LIGHTING,
    gl21::LINE_SMOOTH,
    gl21::MULTISAMPLE,
    gl21::NORMALIZE,
    gl21::POINT_SMOOTH,
    gl21::POLYGON_OFFSET_FILL,
    gl21::RESCALE_NORMAL,
    gl21::SAMPLE_ALPHA_TO_COVERAGE,
    gl21::SAMPLE_ALPHA_TO_ONE,
    gl21::SAMPLE_COVERAGE,
    gl21::SCISSOR_TEST,
    gl21::STENCIL_TEST,
    gl21::TEXTURE_2D,
    // Same as POINT_SPRITE_OES from the GLES extension
    gl21::POINT_SPRITE,
];

#[inline(always)]
fn capability_supported(cap: GLenum) -> bool {
    matches!(
        cap,
        gl21::ALPHA_TEST
            | gl21::BLEND
            | gl21::COLOR_LOGIC_OP
            | gl21::CLIP_PLANE0
            | gl21::CLIP_PLANE1
            | gl21::CLIP_PLANE2
            | gl21::CLIP_PLANE3
            | gl21::CLIP_PLANE4
            | gl21::CLIP_PLANE5
            | gl21::LIGHT0
            | gl21::LIGHT1
            | gl21::LIGHT2
            | gl21::LIGHT3
            | gl21::LIGHT4
            | gl21::LIGHT5
            | gl21::LIGHT6
            | gl21::LIGHT7
            | gl21::COLOR_MATERIAL
            | gl21::CULL_FACE
            | gl21::DEPTH_TEST
            | gl21::DITHER
            | gl21::FOG
            | gl21::LIGHTING
            | gl21::LINE_SMOOTH
            | gl21::MULTISAMPLE
            | gl21::NORMALIZE
            | gl21::POINT_SMOOTH
            | gl21::POLYGON_OFFSET_FILL
            | gl21::RESCALE_NORMAL
            | gl21::SAMPLE_ALPHA_TO_COVERAGE
            | gl21::SAMPLE_ALPHA_TO_ONE
            | gl21::SAMPLE_COVERAGE
            | gl21::SCISSOR_TEST
            | gl21::STENCIL_TEST
            | gl21::TEXTURE_2D
            | gl21::POINT_SPRITE
    )
}

pub const UNSUPPORTED_CAPABILITIES: &[GLenum] = &[
    0x8620, // GL_VERTEX_PROGRAM_NV
    gl21::TEXTURE,
];

/// Subset of [CAPABILITIES] that is part of desktop OpenGL 2.1 but NOT part of
/// the OpenGL ES 1.1 spec. A strict native ES 1.1 driver (e.g. ARM Mali on
/// Android) will return `GL_INVALID_ENUM` from `glEnable`, `glDisable` and
/// `glGetBooleanv` when handed any of these enums. The GL 2.1 emulation
/// backend [`super::gles1_on_gl2::GLES1OnGL2`] accepts them all because it
/// runs on a desktop GL 2.1 driver. Use [`super::GLES::is_native_es1`] to
/// decide whether to skip these in present-time state save/restore loops.
pub const CAPABILITIES_GL21_ONLY: &[GLenum] = &[
    // ES 1.1 has no logical-op stage; this enum is desktop-only.
    gl21::COLOR_LOGIC_OP,
];

pub struct ArrayInfo {
    /// Enum used by `glEnableClientState`, `glDisableClientState` and
    /// `glGetBoolean`.
    pub name: GLenum,
    /// Buffer binding enum for `glGetInteger`.
    pub buffer_binding: GLenum,
    /// Size enum for `glGetInteger`.
    size: Option<GLenum>,
    /// Stride enum for `glGetInteger`.
    stride: GLenum,
    /// Pointer enum for `glGetPointer`.
    pub pointer: GLenum,
}

struct ArrayStateBackup {
    size: Option<GLint>,
    stride: GLsizei,
    pointer: *const GLvoid,
    buffer_binding: GLuint,
}

/// List of arrays shared by OpenGL ES 1.1 and OpenGL 2.1.
///
/// TODO: GL_POINT_SIZE_ARRAY_OES?
pub const ARRAYS: &[ArrayInfo] = &[
    ArrayInfo {
        name: gl21::COLOR_ARRAY,
        buffer_binding: gl21::COLOR_ARRAY_BUFFER_BINDING,
        size: Some(gl21::COLOR_ARRAY_SIZE),
        stride: gl21::COLOR_ARRAY_STRIDE,
        pointer: gl21::COLOR_ARRAY_POINTER,
    },
    ArrayInfo {
        name: gl21::NORMAL_ARRAY,
        buffer_binding: gl21::NORMAL_ARRAY_BUFFER_BINDING,
        size: None,
        stride: gl21::NORMAL_ARRAY_STRIDE,
        pointer: gl21::NORMAL_ARRAY_POINTER,
    },
    ArrayInfo {
        name: gl21::TEXTURE_COORD_ARRAY,
        buffer_binding: gl21::TEXTURE_COORD_ARRAY_BUFFER_BINDING,
        size: Some(gl21::TEXTURE_COORD_ARRAY_SIZE),
        stride: gl21::TEXTURE_COORD_ARRAY_STRIDE,
        pointer: gl21::TEXTURE_COORD_ARRAY_POINTER,
    },
    ArrayInfo {
        name: gl21::VERTEX_ARRAY,
        buffer_binding: gl21::VERTEX_ARRAY_BUFFER_BINDING,
        size: Some(gl21::VERTEX_ARRAY_SIZE),
        stride: gl21::VERTEX_ARRAY_STRIDE,
        pointer: gl21::VERTEX_ARRAY_POINTER,
    },
];

/// Table of `glGet` parameters shared by OpenGL ES 1.1 and OpenGL 2.1.
const GET_PARAMS: ParamTable = ParamTable(&[
    (gl21::ACTIVE_TEXTURE, ParamType::Int, 1),
    (gl21::ALIASED_POINT_SIZE_RANGE, ParamType::Float, 2),
    (gl21::ALIASED_LINE_WIDTH_RANGE, ParamType::Float, 2),
    (gl21::ALPHA_BITS, ParamType::Int, 1),
    (gl21::ALPHA_TEST, ParamType::Boolean, 1),
    (gl21::ALPHA_TEST_FUNC, ParamType::Int, 1),
    (gl21::ALPHA_TEST_REF, ParamType::FloatSpecial, 1), // TODO correct type
    (gl21::ARRAY_BUFFER_BINDING, ParamType::Int, 1),
    (gl21::BLEND, ParamType::Boolean, 1),
    (gl21::BLEND_DST, ParamType::Int, 1),
    (gl21::BLEND_SRC, ParamType::Int, 1),
    (gl21::BLUE_BITS, ParamType::Int, 1),
    (gl21::CLIENT_ACTIVE_TEXTURE, ParamType::Int, 1),
    // TODO: arbitrary number of clip planes?
    (gl21::CLIP_PLANE0, ParamType::Boolean, 1),
    (gl21::CLIP_PLANE1, ParamType::Boolean, 1),
    (gl21::CLIP_PLANE2, ParamType::Boolean, 1),
    (gl21::CLIP_PLANE3, ParamType::Boolean, 1),
    (gl21::CLIP_PLANE4, ParamType::Boolean, 1),
    (gl21::CLIP_PLANE5, ParamType::Boolean, 1),
    (gl21::COLOR_ARRAY, ParamType::Boolean, 1),
    (gl21::COLOR_ARRAY_BUFFER_BINDING, ParamType::Int, 1),
    (gl21::COLOR_ARRAY_SIZE, ParamType::Int, 1),
    (gl21::COLOR_ARRAY_STRIDE, ParamType::Int, 1),
    (gl21::COLOR_ARRAY_TYPE, ParamType::Int, 1),
    (gl21::COLOR_CLEAR_VALUE, ParamType::FloatSpecial, 4), // TODO correct type
    (gl21::COLOR_LOGIC_OP, ParamType::Boolean, 1),
    (gl21::COLOR_MATERIAL, ParamType::Boolean, 1),
    (gl21::COLOR_WRITEMASK, ParamType::Boolean, 4),
    // TODO: COMPRESSED_TEXTURE_FORMATS (needs to return only supported formats)
    (gl21::CULL_FACE, ParamType::Boolean, 1),
    (gl21::CULL_FACE_MODE, ParamType::Int, 1),
    (gl21::CURRENT_COLOR, ParamType::FloatSpecial, 4), // TODO correct type
    // TODO: CURRENT_NORMAL (has special type conversion behavior)
    (gl21::CURRENT_TEXTURE_COORDS, ParamType::Float, 4),
    (gl21::DEPTH_BITS, ParamType::Int, 1),
    // TODO: DEPTH_CLEAR_VALUE (has special type conversion behavior)
    (gl21::DEPTH_FUNC, ParamType::Int, 1),
    // TODO: DEPTH_RANGE (has special type conversion behavior)
    (gl21::DEPTH_TEST, ParamType::Boolean, 1),
    (gl21::DEPTH_WRITEMASK, ParamType::Boolean, 1),
    (gl21::DITHER, ParamType::Boolean, 1),
    (gl21::ELEMENT_ARRAY_BUFFER_BINDING, ParamType::Int, 1),
    (gl21::FOG, ParamType::Boolean, 1),
    // TODO: FOG_COLOR (has special type conversion behavior)
    (gl21::FOG_HINT, ParamType::Int, 1),
    (gl21::FOG_MODE, ParamType::Int, 1),
    (gl21::FOG_DENSITY, ParamType::Float, 1),
    (gl21::FOG_START, ParamType::Float, 1),
    (gl21::FOG_END, ParamType::Float, 1),
    (gl21::FRONT_FACE, ParamType::Int, 1),
    (gl21::GREEN_BITS, ParamType::Int, 1),
    // TODO: IMPLEMENTATION_COLOR_READ_FORMAT_OES? (not shared)
    // TODO: IMPLEMENTATION_COLOR_READ_TYPE_OES? (not shared)
    // TODO: LIGHT_MODEL_AMBIENT (has special type conversion behavior)
    (gl21::LIGHT_MODEL_TWO_SIDE, ParamType::Boolean, 1),
    // TODO: arbitrary number of lights?
    (gl21::LIGHT0, ParamType::Boolean, 1),
    (gl21::LIGHT1, ParamType::Boolean, 1),
    (gl21::LIGHT2, ParamType::Boolean, 1),
    (gl21::LIGHT3, ParamType::Boolean, 1),
    (gl21::LIGHT4, ParamType::Boolean, 1),
    (gl21::LIGHT5, ParamType::Boolean, 1),
    (gl21::LIGHT6, ParamType::Boolean, 1),
    (gl21::LIGHT7, ParamType::Boolean, 1),
    (gl21::LIGHTING, ParamType::Boolean, 1),
    (gl21::LINE_SMOOTH, ParamType::Boolean, 1),
    (gl21::LINE_SMOOTH_HINT, ParamType::Int, 1),
    (gl21::LINE_WIDTH, ParamType::Float, 1),
    (gl21::LOGIC_OP_MODE, ParamType::Int, 1),
    (gl21::MATRIX_MODE, ParamType::Int, 1),
    (gl21::MAX_CLIP_PLANES, ParamType::Int, 1),
    (gl21::MAX_LIGHTS, ParamType::Int, 1),
    (gl21::MAX_MODELVIEW_STACK_DEPTH, ParamType::Int, 1),
    (gl21::MAX_PROJECTION_STACK_DEPTH, ParamType::Int, 1),
    (gl21::MAX_TEXTURE_MAX_ANISOTROPY_EXT, ParamType::Float, 1),
    (gl21::MAX_TEXTURE_SIZE, ParamType::Int, 1),
    (gl21::MAX_TEXTURE_STACK_DEPTH, ParamType::Int, 1),
    (gl21::MAX_TEXTURE_UNITS, ParamType::Int, 1),
    (gl21::MAX_VIEWPORT_DIMS, ParamType::Int, 1),
    (gl21::MODELVIEW_MATRIX, ParamType::Float, 16),
    (gl21::MODELVIEW_STACK_DEPTH, ParamType::Int, 1),
    (gl21::MULTISAMPLE, ParamType::Boolean, 1),
    (gl21::NORMAL_ARRAY, ParamType::Boolean, 1),
    (gl21::NORMAL_ARRAY_BUFFER_BINDING, ParamType::Int, 1),
    (gl21::NORMAL_ARRAY_STRIDE, ParamType::Int, 1),
    (gl21::NORMAL_ARRAY_TYPE, ParamType::Int, 1),
    (gl21::NORMALIZE, ParamType::Boolean, 1),
    (gl21::PACK_ALIGNMENT, ParamType::Int, 1),
    (gl21::PERSPECTIVE_CORRECTION_HINT, ParamType::Int, 1),
    (gl21::POINT_DISTANCE_ATTENUATION, ParamType::Float, 3),
    (gl21::POINT_FADE_THRESHOLD_SIZE, ParamType::Float, 1),
    (gl21::POINT_SIZE, ParamType::Float, 1),
    // TODO: POINT_SIZE_ARRAY_OES etc? (not shared)
    (gl21::POINT_SIZE_MAX, ParamType::Float, 1),
    (gl21::POINT_SIZE_MIN, ParamType::Float, 1),
    (gl21::POINT_SIZE_RANGE, ParamType::Float, 2),
    (gl21::POINT_SMOOTH, ParamType::Boolean, 2),
    (gl21::POINT_SMOOTH_HINT, ParamType::Int, 2),
    (gl21::POINT_SPRITE, ParamType::Boolean, 1),
    (gl21::POLYGON_OFFSET_FACTOR, ParamType::Float, 1),
    (gl21::POLYGON_OFFSET_FILL, ParamType::Boolean, 1),
    (gl21::POLYGON_OFFSET_UNITS, ParamType::Float, 1),
    (gl21::PROJECTION_MATRIX, ParamType::Float, 16),
    (gl21::PROJECTION_STACK_DEPTH, ParamType::Int, 1),
    (gl21::RED_BITS, ParamType::Int, 1),
    (gl21::RESCALE_NORMAL, ParamType::Boolean, 1),
    (gl21::SAMPLE_ALPHA_TO_COVERAGE, ParamType::Boolean, 1),
    (gl21::SAMPLE_ALPHA_TO_ONE, ParamType::Boolean, 1),
    (gl21::SAMPLE_BUFFERS, ParamType::Int, 1),
    (gl21::SAMPLE_COVERAGE, ParamType::Boolean, 1),
    (gl21::SAMPLE_COVERAGE_INVERT, ParamType::Boolean, 1),
    (gl21::SAMPLE_COVERAGE_VALUE, ParamType::Float, 1),
    (gl21::SAMPLES, ParamType::Int, 1),
    (gl21::SCISSOR_BOX, ParamType::Int, 4),
    (gl21::SCISSOR_TEST, ParamType::Boolean, 1),
    (gl21::SHADE_MODEL, ParamType::Int, 1),
    (gl21::SMOOTH_LINE_WIDTH_RANGE, ParamType::Float, 2),
    (gl21::SMOOTH_POINT_SIZE_RANGE, ParamType::Float, 2),
    (gl21::STENCIL_BITS, ParamType::Int, 1),
    (gl21::STENCIL_CLEAR_VALUE, ParamType::Int, 1),
    (gl21::STENCIL_FAIL, ParamType::Int, 1),
    (gl21::STENCIL_FUNC, ParamType::Int, 1),
    (gl21::STENCIL_PASS_DEPTH_FAIL, ParamType::Int, 1),
    (gl21::STENCIL_PASS_DEPTH_PASS, ParamType::Int, 1),
    (gl21::STENCIL_REF, ParamType::Int, 1),
    (gl21::STENCIL_TEST, ParamType::Boolean, 1),
    (gl21::STENCIL_VALUE_MASK, ParamType::Int, 1),
    (gl21::STENCIL_WRITEMASK, ParamType::Int, 1),
    (gl21::SUBPIXEL_BITS, ParamType::Int, 1),
    (gl21::TEXTURE_2D, ParamType::Boolean, 1),
    (gl21::TEXTURE_BINDING_2D, ParamType::Int, 1),
    (gl21::TEXTURE_COORD_ARRAY, ParamType::Boolean, 1),
    (gl21::TEXTURE_COORD_ARRAY_BUFFER_BINDING, ParamType::Int, 1),
    (gl21::TEXTURE_COORD_ARRAY_SIZE, ParamType::Int, 1),
    (gl21::TEXTURE_COORD_ARRAY_STRIDE, ParamType::Int, 1),
    (gl21::TEXTURE_COORD_ARRAY_TYPE, ParamType::Int, 1),
    (gl21::TEXTURE_MATRIX, ParamType::Float, 16),
    (gl21::TEXTURE_STACK_DEPTH, ParamType::Int, 1),
    (gl21::UNPACK_ALIGNMENT, ParamType::Int, 1),
    (gl21::VIEWPORT, ParamType::Int, 4),
    (gl21::VERTEX_ARRAY, ParamType::Boolean, 1),
    (gl21::VERTEX_ARRAY_BUFFER_BINDING, ParamType::Int, 1),
    (gl21::VERTEX_ARRAY_SIZE, ParamType::Int, 1),
    (gl21::VERTEX_ARRAY_STRIDE, ParamType::Int, 1),
    (gl21::VERTEX_ARRAY_TYPE, ParamType::Int, 1),
    // OES_framebuffer_object -> EXT_framebuffer_object
    (gl21::FRAMEBUFFER_BINDING_EXT, ParamType::Int, 1),
    (gl21::RENDERBUFFER_BINDING_EXT, ParamType::Int, 1),
    // EXT_texture_lod_bias
    (gl21::MAX_TEXTURE_LOD_BIAS_EXT, ParamType::Float, 1),
    // OES_matrix_palette -> ARB_matrix_palette
    (gl21::MAX_PALETTE_MATRICES_ARB, ParamType::Int, 1),
    // OES_matrix_palette -> ARB_vertex_blend
    (gl21::MAX_VERTEX_UNITS_ARB, ParamType::Int, 1),
    // OpenGL ES 2.0 / GL 2.0
    (gl21::CURRENT_PROGRAM, ParamType::Int, 1),
    (gl21::MAX_VERTEX_ATTRIBS, ParamType::Int, 1),
    (gl21::MAX_VERTEX_UNIFORM_COMPONENTS, ParamType::Int, 1),
    (gl21::MAX_FRAGMENT_UNIFORM_COMPONENTS, ParamType::Int, 1),
    (gl21::MAX_VARYING_FLOATS, ParamType::Int, 1),
    (gl21::MAX_TEXTURE_IMAGE_UNITS, ParamType::Int, 1),
    (gl21::MAX_VERTEX_TEXTURE_IMAGE_UNITS, ParamType::Int, 1),
    (gl21::MAX_COMBINED_TEXTURE_IMAGE_UNITS, ParamType::Int, 1),
    (gl21::MAX_RENDERBUFFER_SIZE_EXT, ParamType::Int, 1),
]);

const UNSUPPORTED_GET_PARAMS: ParamTable = ParamTable(&[
    (gl21::COMPRESSED_TEXTURE_FORMATS, ParamType::Int, 0), // Dynamically sized
]);

const POINT_PARAMS: ParamTable = ParamTable(&[
    (gl21::POINT_SIZE_MIN, ParamType::Float, 1),
    (gl21::POINT_SIZE_MAX, ParamType::Float, 1),
    (gl21::POINT_DISTANCE_ATTENUATION, ParamType::Float, 3),
    (gl21::POINT_FADE_THRESHOLD_SIZE, ParamType::Float, 1),
    (gl21::POINT_SMOOTH, ParamType::Boolean, 1),
]);

/// Table of `glFog` parameters shared by OpenGL ES 1.1 and OpenGL 2.1.
const FOG_PARAMS: ParamTable = ParamTable(&[
    // Despite only having f, fv, x and xv setters in OpenGL ES 1.1, this is
    // an integer! (You're meant to use the x/xv setter.)
    (gl21::FOG_MODE, ParamType::Int, 1),
    (gl21::FOG_DENSITY, ParamType::Float, 1),
    (gl21::FOG_START, ParamType::Float, 1),
    (gl21::FOG_END, ParamType::Float, 1),
    (gl21::FOG_COLOR, ParamType::FloatSpecial, 4), // TODO correct type
]);

/// Table of `glLight` parameters shared by OpenGL ES 1.1 and OpenGL 2.1.
const LIGHT_PARAMS: ParamTable = ParamTable(&[
    (gl21::AMBIENT, ParamType::Float, 4),
    (gl21::DIFFUSE, ParamType::Float, 4),
    (gl21::SPECULAR, ParamType::Float, 4),
    (gl21::POSITION, ParamType::Float, 4),
    (gl21::SPOT_CUTOFF, ParamType::Float, 1),
    (gl21::SPOT_DIRECTION, ParamType::Float, 3),
    (gl21::SPOT_EXPONENT, ParamType::Float, 1),
    (gl21::CONSTANT_ATTENUATION, ParamType::Float, 1),
    (gl21::LINEAR_ATTENUATION, ParamType::Float, 1),
    (gl21::QUADRATIC_ATTENUATION, ParamType::Float, 1),
]);

const LIGHT_MODEL_PARAMS: ParamTable = ParamTable(&[
    (gl21::LIGHT_MODEL_AMBIENT, ParamType::Float, 4),
    (gl21::LIGHT_MODEL_TWO_SIDE, ParamType::Boolean, 1),
]);

/// Table of `glMaterial` parameters shared by OpenGL ES 1.1 and OpenGL 2.1.
const MATERIAL_PARAMS: ParamTable = ParamTable(&[
    (gl21::AMBIENT, ParamType::Float, 4),
    (gl21::DIFFUSE, ParamType::Float, 4),
    (gl21::SPECULAR, ParamType::Float, 4),
    (gl21::EMISSION, ParamType::Float, 4),
    (gl21::SHININESS, ParamType::Float, 1),
    // Not a true parameter: it's equivalent to calling glMaterial twice, once
    // for GL_AMBIENT and once for GL_DIFFUSE.
    (gl21::AMBIENT_AND_DIFFUSE, ParamType::Float, 4),
]);

/// Table of `glTexEnv` parameters for the `GL_TEXTURE_ENV` target shared by
/// OpenGL ES 1.1 and OpenGL 2.1.
const TEX_ENV_PARAMS: ParamTable = ParamTable(&[
    (gl21::TEXTURE_ENV_MODE, ParamType::Int, 1),
    (gl21::COORD_REPLACE, ParamType::Int, 1),
    (gl21::COMBINE_RGB, ParamType::Int, 1),
    (gl21::COMBINE_ALPHA, ParamType::Int, 1),
    (gl21::SRC0_RGB, ParamType::Int, 1),
    (gl21::SRC1_RGB, ParamType::Int, 1),
    (gl21::SRC2_RGB, ParamType::Int, 1),
    (gl21::SRC0_ALPHA, ParamType::Int, 1),
    (gl21::SRC1_ALPHA, ParamType::Int, 1),
    (gl21::SRC2_ALPHA, ParamType::Int, 1),
    (gl21::OPERAND0_RGB, ParamType::Int, 1),
    (gl21::OPERAND1_RGB, ParamType::Int, 1),
    (gl21::OPERAND2_RGB, ParamType::Int, 1),
    (gl21::OPERAND0_ALPHA, ParamType::Int, 1),
    (gl21::OPERAND1_ALPHA, ParamType::Int, 1),
    (gl21::OPERAND2_ALPHA, ParamType::Int, 1),
    (gl21::TEXTURE_ENV_COLOR, ParamType::Float, 4),
    (gl21::RGB_SCALE, ParamType::Float, 1),
    (gl21::ALPHA_SCALE, ParamType::Float, 1),
]);

/// Table of `glTexParameter` parameters.
const TEX_PARAMS: ParamTable = ParamTable(&[
    (gl21::TEXTURE_MIN_FILTER, ParamType::Int, 1),
    (gl21::TEXTURE_MAG_FILTER, ParamType::Int, 1),
    (gl21::TEXTURE_WRAP_S, ParamType::Int, 1),
    (gl21::TEXTURE_WRAP_T, ParamType::Int, 1),
    (gl21::GENERATE_MIPMAP, ParamType::Int, 1),
    (gl21::TEXTURE_MAX_ANISOTROPY_EXT, ParamType::Float, 1),
    (gl21::MAX_TEXTURE_MAX_ANISOTROPY_EXT, ParamType::Float, 1),
]);

const UNSUPPORTED_TEX_PARAMS: ParamTable =
    ParamTable(&[(gl21::TEXTURE_MAX_LEVEL, ParamType::Float, 1)]);

const MATRIX_PALETTE_MIN_MATRICES: usize = 9;
const MATRIX_PALETTE_DEFAULT_UNITS: usize = 3;
const MATRIX_PALETTE_MAX_UNITS: usize = MATRIX_PALETTE_DEFAULT_UNITS;

/// Column-major flat 4×4 identity matrix.
const MATRIX_IDENTITY: [GLfloat; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, //
];

/// Multiply two column-major flat 4×4 matrices, returning `a * b` using the
/// OpenGL convention (so that applying the result to a column vector is
/// equivalent to applying `b` then `a`). `m[col * 4 + row]` indexing.
fn mat4_multiply(a: &[GLfloat; 16], b: &[GLfloat; 16]) -> [GLfloat; 16] {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0f32;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = sum;
        }
    }
    out
}

/// Transform a 4-component column vector by a column-major flat 4×4 matrix.
fn mat4_transform(m: &[GLfloat; 16], v: [GLfloat; 4]) -> [GLfloat; 4] {
    let mut out = [0.0f32; 4];
    for row in 0..4 {
        out[row] = m[row] * v[0] + m[4 + row] * v[1] + m[8 + row] * v[2] + m[12 + row] * v[3];
    }
    out
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum MatrixModeState {
    ModelView,
    Projection,
    Texture,
    MatrixPalette,
}

#[derive(Copy, Clone, Debug)]
struct MatrixArrayPointerState {
    size: GLint,
    type_: GLenum,
    stride: GLsizei,
    pointer: *const GLvoid,
    enabled: bool,
    buffer_binding: GLuint,
}

impl Default for MatrixArrayPointerState {
    fn default() -> Self {
        MatrixArrayPointerState {
            size: 0,
            type_: 0,
            stride: 0,
            pointer: std::ptr::null(),
            enabled: false,
            buffer_binding: 0,
        }
    }
}

pub struct GLES1OnGL2State {
    gpu_profile: gpu_profile::State,
    state_cache: ds_gl_state_cache::Cache,
    scene:crate::ds_scene_diagnostics::RenderState,
    input_prompt_atlas: Option<GLuint>,
    opening_fog: ds_opening_fog::OpeningFog,
    pointer_is_fixed_point: [bool; ARRAYS.len()],
    fixed_point_texture_units: HashSet<GLenum>,
    fixed_point_translation_buffers: [Vec<GLfloat>; ARRAYS.len()],
    matrix_mode: MatrixModeState,
    matrix_palette_enabled: bool,
    current_palette_matrix: GLuint,
    palette_matrices: Vec<[GLfloat; 16]>,
    palette_weight_state: MatrixArrayPointerState,
    palette_index_state: MatrixArrayPointerState,
    /// `GL_OES_draw_texture`: `GL_TEXTURE_CROP_RECT_OES` is per-texture-object
    /// state that OpenGL 2.1 knows nothing about, so we keep it here, keyed by
    /// texture name. Missing means the spec default of (0, 0, 0, 0).
    texture_crop_rects: HashMap<GLuint, [GLint; 4]>,
}

/// Texture-id restriction for the projection-z workaround.
///
/// Compressing the projection z row is a blunt instrument: it rescales depth
/// for *every* draw, which is why large factors visibly corrupt near geometry
/// (posters etc.). `TOUCHHLE_DS_PROJ_Z_SCALE_TEX` restricts it to draws whose
/// currently-bound texture is one of the listed ids, so the sky layers can be
/// pulled back inside the frustum while the rest of the scene keeps its
/// original depth. Unset ⇒ global (legacy) behaviour.
/// Clip-space z range of the vertices about to be drawn, or None when the
/// vertex data cannot be read (server-side VBO, or no array bound).
///
/// Used by the TOUCHHLE_DS_PROJ_Z_SCALE_CLIP_ONLY mode: the projection z
/// compression is only applied to draws that would actually be clipped by the
/// far/near plane, so layers that are already inside the frustum (nebula,
/// posters, HUD) keep their original depth. This avoids the blunt-instrument
/// side effects of applying a global z scale to every draw.
unsafe fn ds_draw_z_ndc_range(count: GLsizei) -> Option<(f32, f32)> {
    let mut vertex_size: GLint = 0;
    let mut vertex_type: GLint = 0;
    let mut vertex_stride: GLint = 0;
    let mut vertex_buffer: GLint = 0;
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_SIZE, &mut vertex_size);
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_TYPE, &mut vertex_type);
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_STRIDE, &mut vertex_stride);
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_BUFFER_BINDING, &mut vertex_buffer);
    let mut vertex_ptr: *mut GLvoid = std::ptr::null_mut();
    #[allow(clippy::unnecessary_mut_passed)]
    gl21::GetPointerv(gl21::VERTEX_ARRAY_POINTER, &mut vertex_ptr);
    if vertex_buffer != 0 || vertex_ptr.is_null() || vertex_size <= 0 || count <= 0 {
        return None;
    }
    let vertex_size = vertex_size.clamp(0, 4) as usize;
    let elem_size = match vertex_type as GLenum {
        gl21::BYTE | gl21::UNSIGNED_BYTE => 1usize,
        gl21::SHORT | gl21::UNSIGNED_SHORT => 2,
        _ => 4,
    };
    let stride = if vertex_stride == 0 {
        vertex_size * elem_size
    } else {
        vertex_stride as usize
    };
    let mut mv = [0.0f32; 16];
    gl21::GetFloatv(gl21::MODELVIEW_MATRIX, mv.as_mut_ptr());
    let mut pj = [0.0f32; 16];
    gl21::GetFloatv(gl21::PROJECTION_MATRIX, pj.as_mut_ptr());
    let sample = crate::ds_draw_env::var("TOUCHHLE_DS_PROJ_Z_SAMPLE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(64)
        .clamp(1, 4096);
    let n = count.clamp(1, sample as GLsizei) as usize;
    let mut zmin = f32::MAX;
    let mut zmax = f32::MIN;
    for i in 0..n {
        let mut v = [0.0f32; 4];
        v[3] = 1.0;
        for c in 0..vertex_size {
            let ptr = (vertex_ptr as *const u8).add(i * stride + c * elem_size);
            v[c] = match vertex_type as GLenum {
                gl21::FLOAT => (ptr as *const f32).read_unaligned(),
                gles11::FIXED => (ptr as *const i32).read_unaligned() as f32 / 65536.0,
                gl21::UNSIGNED_BYTE => (ptr as *const u8).read_unaligned() as f32,
                gl21::BYTE => (ptr as *const i8).read_unaligned() as f32,
                gl21::UNSIGNED_SHORT => (ptr as *const u16).read_unaligned() as f32,
                gl21::SHORT => (ptr as *const i16).read_unaligned() as f32,
                _ => 0.0,
            };
        }
        let ex = mv[0] * v[0] + mv[4] * v[1] + mv[8] * v[2] + mv[12] * v[3];
        let ey = mv[1] * v[0] + mv[5] * v[1] + mv[9] * v[2] + mv[13] * v[3];
        let ez = mv[2] * v[0] + mv[6] * v[1] + mv[10] * v[2] + mv[14] * v[3];
        let ew = mv[3] * v[0] + mv[7] * v[1] + mv[11] * v[2] + mv[15] * v[3];
        let cz = pj[2] * ex + pj[6] * ey + pj[10] * ez + pj[14] * ew;
        let cw = pj[3] * ex + pj[7] * ey + pj[11] * ez + pj[15] * ew;
        if cw == 0.0 || !cz.is_finite() || !cw.is_finite() {
            continue;
        }
        let zn = cz / cw;
        if !zn.is_finite() {
            continue;
        }
        if zn < zmin {
            zmin = zn;
        }
        if zn > zmax {
            zmax = zn;
        }
    }
    if zmin > zmax {
        None
    } else {
        Some((zmin, zmax))
    }
}

/// NDC XY span of the vertices about to be drawn. Full-screen space layers
/// use client vertex arrays and cover both axes; local docking-bay batches do
/// not. This lets the depth tie-break target the background stack even when it
/// shares an atlas with foreground geometry.
unsafe fn ds_draw_ndc_span(count: GLsizei) -> Option<(f32, f32)> {
    let mut vertex_size: GLint = 0;
    let mut vertex_type: GLint = 0;
    let mut vertex_stride: GLint = 0;
    let mut vertex_buffer: GLint = 0;
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_SIZE, &mut vertex_size);
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_TYPE, &mut vertex_type);
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_STRIDE, &mut vertex_stride);
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_BUFFER_BINDING, &mut vertex_buffer);
    let mut vertex_ptr: *mut GLvoid = std::ptr::null_mut();
    #[allow(clippy::unnecessary_mut_passed)]
    gl21::GetPointerv(gl21::VERTEX_ARRAY_POINTER, &mut vertex_ptr);
    if vertex_buffer != 0 || vertex_ptr.is_null() || vertex_size <= 0 || count <= 0 {
        return None;
    }
    let vertex_size = vertex_size.clamp(0, 4) as usize;
    let elem_size = match vertex_type as GLenum {
        gl21::BYTE | gl21::UNSIGNED_BYTE => 1usize,
        gl21::SHORT | gl21::UNSIGNED_SHORT => 2,
        _ => 4,
    };
    let stride = if vertex_stride == 0 {
        vertex_size * elem_size
    } else {
        vertex_stride as usize
    };
    let mut mv = [0.0f32; 16];
    gl21::GetFloatv(gl21::MODELVIEW_MATRIX, mv.as_mut_ptr());
    let mut pj = [0.0f32; 16];
    gl21::GetFloatv(gl21::PROJECTION_MATRIX, pj.as_mut_ptr());
    let n = count.clamp(1, 4096) as usize;
    let mut xmin = f32::MAX;
    let mut xmax = f32::MIN;
    let mut ymin = f32::MAX;
    let mut ymax = f32::MIN;
    for i in 0..n {
        let mut v = [0.0f32; 4];
        v[3] = 1.0;
        for c in 0..vertex_size {
            let ptr = (vertex_ptr as *const u8).add(i * stride + c * elem_size);
            v[c] = match vertex_type as GLenum {
                gl21::FLOAT => (ptr as *const f32).read_unaligned(),
                gles11::FIXED => (ptr as *const i32).read_unaligned() as f32 / 65536.0,
                gl21::UNSIGNED_BYTE => (ptr as *const u8).read_unaligned() as f32,
                gl21::BYTE => (ptr as *const i8).read_unaligned() as f32,
                gl21::UNSIGNED_SHORT => (ptr as *const u16).read_unaligned() as f32,
                gl21::SHORT => (ptr as *const i16).read_unaligned() as f32,
                _ => 0.0,
            };
        }
        let ex = mv[0] * v[0] + mv[4] * v[1] + mv[8] * v[2] + mv[12] * v[3];
        let ey = mv[1] * v[0] + mv[5] * v[1] + mv[9] * v[2] + mv[13] * v[3];
        let ez = mv[2] * v[0] + mv[6] * v[1] + mv[10] * v[2] + mv[14] * v[3];
        let ew = mv[3] * v[0] + mv[7] * v[1] + mv[11] * v[2] + mv[15] * v[3];
        let cx = pj[0] * ex + pj[4] * ey + pj[8] * ez + pj[12] * ew;
        let cy = pj[1] * ex + pj[5] * ey + pj[9] * ez + pj[13] * ew;
        let cw = pj[3] * ex + pj[7] * ey + pj[11] * ez + pj[15] * ew;
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

/// Interaction-ring probe (log only).
///
/// `TOUCHHLE_DS_RING_TEX=75,76` makes every draw that uses one of those
/// textures log its screen-space bounding box in landscape iOS points:
///
///   [DSring] frame=N tex=T cnt=C ios=(x,y) size=(w,h)
///
/// The ring is a 3D-projected object whose screen size changes with distance,
/// so a fixed pixel-area test is unreliable (user, 2026-10-08). This probe
/// identifies the ring by *texture id* instead, independent of screen size.
pub fn ds_ring_textures() -> &'static [GLint] {
    use std::sync::OnceLock;
    static TEXTURES: OnceLock<Vec<GLint>> = OnceLock::new();
    TEXTURES
        .get_or_init(|| {
            crate::ds_draw_env::var("TOUCHHLE_DS_RING_TEX")
                .map(|spec| {
                    spec.split(',')
                        .filter_map(|p| p.trim().parse().ok())
                        .collect()
                })
                .unwrap_or_default()
        })
        .as_slice()
}

/// Log (and remember) a draw that uses one of the configured ring textures.
pub unsafe fn ds_ring_probe(kind: &str, count: GLsizei) {
    let texs = ds_ring_textures();
    if texs.is_empty() {
        return;
    }
    let mut cur: GLint = 0;
    gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut cur);
    if !texs.contains(&cur) {
        return;
    }
    // read the client-side vertex array (first 16 vertices is plenty for a quad)
    let mut vsize: GLint = 0;
    let mut vtype: GLint = 0;
    let mut vstride: GLint = 0;
    let mut vbuf: GLint = 0;
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_SIZE, &mut vsize);
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_TYPE, &mut vtype);
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_STRIDE, &mut vstride);
    gl21::GetIntegerv(gl21::VERTEX_ARRAY_BUFFER_BINDING, &mut vbuf);
    let mut vptr: *mut GLvoid = std::ptr::null_mut();
    #[allow(clippy::unnecessary_mut_passed)]
    gl21::GetPointerv(gl21::VERTEX_ARRAY_POINTER, &mut vptr);
    if vbuf != 0 || vptr.is_null() || vsize <= 0 || count <= 0 {
        log!(
            "[DSring] tex={} {} cnt={} (server-side vertices; no bbox)",
            cur,
            kind,
            count
        );
        return;
    }
    let vsize = vsize.clamp(0, 4) as usize;
    let esz = match vtype as GLenum {
        gl21::BYTE | gl21::UNSIGNED_BYTE => 1usize,
        gl21::SHORT | gl21::UNSIGNED_SHORT => 2,
        _ => 4,
    };
    let stride = if vstride == 0 {
        vsize * esz
    } else {
        vstride as usize
    };
    let mut mv = [0.0f32; 16];
    gl21::GetFloatv(gl21::MODELVIEW_MATRIX, mv.as_mut_ptr());
    let mut pj = [0.0f32; 16];
    gl21::GetFloatv(gl21::PROJECTION_MATRIX, pj.as_mut_ptr());
    let mut xn = f32::MAX;
    let mut xx = f32::MIN;
    let mut yn = f32::MAX;
    let mut yx = f32::MIN;
    let n = count.clamp(1, 16) as usize;
    for i in 0..n {
        let mut v = [0.0f32; 4];
        v[3] = 1.0;
        for c in 0..vsize {
            let p = (vptr as *const u8).add(i * stride + c * esz);
            v[c] = match vtype as GLenum {
                gl21::FLOAT => (p as *const f32).read_unaligned(),
                gles11::FIXED => (p as *const i32).read_unaligned() as f32 / 65536.0,
                gl21::UNSIGNED_BYTE => (p as *const u8).read_unaligned() as f32,
                gl21::BYTE => (p as *const i8).read_unaligned() as f32,
                gl21::UNSIGNED_SHORT => (p as *const u16).read_unaligned() as f32,
                gl21::SHORT => (p as *const i16).read_unaligned() as f32,
                _ => 0.0,
            };
        }
        let ex = mv[0] * v[0] + mv[4] * v[1] + mv[8] * v[2] + mv[12] * v[3];
        let ey = mv[1] * v[0] + mv[5] * v[1] + mv[9] * v[2] + mv[13] * v[3];
        let ez = mv[2] * v[0] + mv[6] * v[1] + mv[10] * v[2] + mv[14] * v[3];
        let ew = mv[3] * v[0] + mv[7] * v[1] + mv[11] * v[2] + mv[15] * v[3];
        let cx = pj[0] * ex + pj[4] * ey + pj[8] * ez + pj[12] * ew;
        let cy = pj[1] * ex + pj[5] * ey + pj[9] * ez + pj[13] * ew;
        let cw = pj[3] * ex + pj[7] * ey + pj[11] * ez + pj[15] * ew;
        if cw == 0.0 || !cw.is_finite() {
            continue;
        }
        let nx = cx / cw;
        let ny = cy / cw;
        if !nx.is_finite() || !ny.is_finite() {
            continue;
        }
        if nx < xn {
            xn = nx;
        }
        if nx > xx {
            xx = nx;
        }
        if ny < yn {
            yn = ny;
        }
        if ny > yx {
            yx = ny;
        }
    }
    if xn > xx || yn > yx {
        return;
    }
    log!(
        "[DSring] frame={} tex={} {} cnt={} ndc=[{:.3},{:.3}]-[{:.3},{:.3}] centre=({:.3},{:.3})",
        crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),
        cur,
        kind,
        count,
        xn,
        yn,
        xx,
        yx,
        (xn + xx) / 2.0,
        (yn + yx) / 2.0
    );
}

unsafe fn proj_z_scale_tex_filter() -> Option<Vec<GLint>> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_PROJ_Z_SCALE_TEX").ok()?;
    let ids: Vec<GLint> = spec
        .split(',')
        .filter_map(|p| p.trim().parse::<GLint>().ok())
        .collect();
    if ids.is_empty() {
        None
    } else {
        Some(ids)
    }
}

/// Content key of the texture bound right now, i.e. of the picture a draw is
/// about to sample.
///
/// GL texture *names* get recycled: one name can hold completely unrelated
/// pictures at different moments of a single run (a real dump had four
/// different images under `tex 2`). So `TOUCHHLE_DS_*_TEX` switches that match
/// on the name are only valid for an instant; anything meant to reliably
/// identify "that one picture" must match this key instead.
unsafe fn bound_texture_key() -> Option<u32> {
    let mut cur: GLint = 0;
    gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut cur);
    crate::ds_texid::lookup(cur as u32).map(|ident| ident.key)
}

/// `TOUCHHLE_DS_<kind>_HASH=<key>[,<key>...]` as hexadecimal content keys
/// (the same values the texture dump writes into `manifest.csv`).
unsafe fn tex_hash_matches_spec(spec: &str) -> bool {
    let keys = crate::ds_texid::parse_keys(spec);
    if keys.is_empty() {
        return false;
    }
    match bound_texture_key() {
        Some(key) if keys.contains(&key) => {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static LOGGED: AtomicUsize = AtomicUsize::new(0);
            if LOGGED.fetch_add(1, Ordering::Relaxed) < 20 {
                log!("texture content key {:08x} matched", key);
            }
            true
        }
        _ => false,
    }
}

/// Same list but for the "is it in the list" test, without logging.
unsafe fn tex_hash_in_spec(spec: &str) -> bool {
    let keys = crate::ds_texid::parse_keys(spec);
    if keys.is_empty() {
        return false;
    }
    matches!(bound_texture_key(), Some(key) if keys.contains(&key))
}

/// Is the currently bound texture the one this debug switch means?
///
/// Accepts either the legacy numeric GL name (`id`) or a list of upload content
/// keys (`keys`). Prefer the keys: Dead Space recycles GL names, so a numeric
/// selection only holds for as long as that name happens to carry the wanted
/// picture (see `crate::ds_texid`).
unsafe fn tex_selected(id: Option<GLint>, keys: &[u32]) -> bool {
    let mut cur: GLint = 0;
    gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut cur);
    if let Some(want) = id {
        if want == cur {
            return true;
        }
    }
    if keys.is_empty() {
        return false;
    }
    matches!(crate::ds_texid::lookup(cur as u32), Some(ident) if keys.contains(&ident.key))
}

/// Runtime z-fix mode, cycled with **Shift+F6** so a SINGLE run can A/B the
/// fix without cross-run reproducibility problems (frame numbers are not
/// comparable between runs - see REPORT 13.137).
///
/// 0 = use the environment variables exactly as before (default behaviour)
/// 1 = z fix completely OFF
/// 2 = global z scale 0.85 (strong blunt fix)
/// 3 = clip-only + 0.85
/// 4 = minimal necessary scale
pub static DS_Z_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Advance the runtime z-fix mode; returns the new mode so the caller can log it.
pub fn ds_z_mode_cycle() -> u8 {
    use std::sync::atomic::Ordering;
    let next = (DS_Z_MODE.load(Ordering::Relaxed) + 1) % 5;
    DS_Z_MODE.store(next, Ordering::Relaxed);
    next
}

/// Human-readable name of a runtime z-fix mode.
pub fn ds_z_mode_name(m: u8) -> &'static str {
    match m {
        1 => "z fix OFF",
        2 => "global z=0.85",
        3 => "clip-only + 0.85",
        4 => "minimal scale",
        _ => "env/default",
    }
}

/// Apply the prologue far-plane correction where the projection is defined,
/// independent of menu/loading frame timing.
fn ds_adjust_opening_frustum_far(near: f32, far: f32) -> f32 {
    if crate::ds_draw_env::contains("TOUCHHLE_DS_NO_PROJ_Z_FIX") {
        return far;
    }
    let setting = crate::ds_draw_env::var("TOUCHHLE_DS_PROJ_Z_INFINITE")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase());
    if setting.as_deref() == Some("0") || near <= 0.0 || far <= near {
        return far;
    }
    let opening_projection = near >= 0.01 && near <= 0.1 && far >= 20.0 && far <= 40.0;
    if setting.as_deref() == Some("1") || opening_projection {
        1.0e9
    } else {
        far
    }
}

/// Debug aid: when `TOUCHHLE_DS_PROJ_Z_ZERO` is set, replace the projection
/// matrix's z row with zeros around a draw so clip-space z is always 0. This
/// takes the far/near planes out of the equation and tells us whether missing
/// background geometry is being clipped by the frustum.
unsafe fn proj_z_zero_begin(count: GLsizei) -> bool {
    // Workaround for a class of Dead Space background draws whose clip-space z
    // lands just past the far plane (measured ~1.001-1.006) so the whole layer
    // is discarded and the scene renders black. Compressing the projection's z
    // row by a small factor pulls those vertices back inside the frustum while
    // preserving depth ordering.
    // Runtime override (Shift+F6) takes precedence over the env vars.
    let ds_mode = DS_Z_MODE.load(std::sync::atomic::Ordering::Relaxed);
    if ds_mode == 1 {
        return false;
    }
    let force_clip_only = ds_mode == 3;
    let force_minimal = ds_mode == 4;
    let force_sc: Option<f32> = match ds_mode {
        2 | 3 => Some(0.85),
        _ => None,
    };
    let zero = crate::ds_draw_env::contains("TOUCHHLE_DS_PROJ_Z_ZERO");
    if !zero && crate::ds_draw_env::contains("TOUCHHLE_DS_NO_PROJ_Z_FIX") {
        return false;
    }
    // Optional: only touch draws that use one of the listed textures.
    if let Some(ids) = proj_z_scale_tex_filter() {
        let mut cur: GLint = 0;
        gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut cur);
        if !ids.contains(&cur) {
            return false;
        }
    }
    // Same restriction, but addressed by upload content instead of the GL
    // name, which Dead Space recycles (see `crate::ds_texid`).
    if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_PROJ_Z_SCALE_TEX_HASH") {
        if !tex_hash_in_spec(&spec) {
            return false;
        }
    }
    // Clip-only mode: leave already-inside geometry completely alone.
    if force_clip_only || crate::ds_draw_env::contains("TOUCHHLE_DS_PROJ_Z_SCALE_CLIP_ONLY") {
        match ds_draw_z_ndc_range(count) {
            // Whole draw is inside [-1,1] in z: there is no clipping to fix.
            Some((zmin, zmax)) if zmin >= -1.0 && zmax <= 1.0 => return false,
            // Clipped, or unreadable vertex data: apply the fix.
            _ => {}
        }
    }
    // Minimal-scale mode: derive the correction from this draw own z range
    // instead of using one fixed factor for everything. A draw whose farthest
    // vertex is only 0.3% past the far plane gets a 0.997 scale, not 0.85, so
    // layers that are only slightly clipped are not pushed around.
    let mut sc_override: Option<f32> = None;
    if force_minimal || crate::ds_draw_env::contains("TOUCHHLE_DS_PROJ_Z_SCALE_MINIMAL") {
        let floor = crate::ds_draw_env::var("TOUCHHLE_DS_PROJ_Z_SCALE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.9);
        match ds_draw_z_ndc_range(count) {
            Some((zmin, zmax)) => {
                if zmin >= -1.0 && zmax <= 1.0 {
                    return false;
                }
                let mut s = 1.0f32;
                if zmax > 1.0 {
                    s = s.min(1.0 / zmax);
                }
                if zmin < -1.0 {
                    s = s.min(1.0 / (-zmin).max(1.0));
                }
                sc_override = Some(s.max(floor).min(1.0));
            }
            // Unreadable vertex data: leave the draw completely alone.
            None => return false,
        }
    }
    let mut m = [0.0f32; 16];
    gl21::GetFloatv(gl21::PROJECTION_MATRIX, m.as_mut_ptr());
    let infinite_setting = crate::ds_draw_env::var("TOUCHHLE_DS_PROJ_Z_INFINITE").ok();
    // The prologue camera uses near=0.05/far=30 and animates geometry past the
    // guest's declared far plane. The software GL path drops those pieces even
    // though the original fixed-function ES renderer keeps them. Scope the
    // correction by projection signature rather than frame number: menu/loading
    // speed varies between launches, so the old 3000..4500 window could miss
    // the prologue entirely on a user's normal save/menu flow.
    let opening_projection =
        m[10] >= -1.006 && m[10] <= -1.001 && m[14] >= -0.105 && m[14] <= -0.095;
    let infinite_far = infinite_setting.as_deref() == Some("1")
        || (infinite_setting.is_none() && opening_projection);
    let sc = if zero || infinite_far {
        0.0
    } else if let Some(s) = sc_override {
        s
    } else if let Some(s) = force_sc {
        s
    } else {
        crate::ds_draw_env::var("TOUCHHLE_DS_PROJ_Z_SCALE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(1.0)
    };
    if infinite_far && !zero {
        // The guest's finite far plane clips real geometry in the prologue.
        // Re-target only the depth coefficient to an infinite far plane. This
        // keeps the near-plane mapping and XY projection intact, unlike a
        // global compression of the whole Z row.
        if m[10] < -1.0 {
            m[10] = -1.0;
        }
    } else {
        m[2] *= sc;
        m[6] *= sc;
        m[10] *= sc;
        m[14] *= sc;
    }
    let mut old_mode: GLint = 0;
    gl21::GetIntegerv(gl21::MATRIX_MODE, &mut old_mode);
    gl21::MatrixMode(gl21::PROJECTION);
    gl21::PushMatrix();
    gl21::LoadMatrixf(m.as_ptr());
    gl21::MatrixMode(old_mode as GLenum);
    true
}
unsafe fn proj_z_zero_end(active: bool) {
    if !active {
        return;
    }
    let mut old_mode: GLint = 0;
    gl21::GetIntegerv(gl21::MATRIX_MODE, &mut old_mode);
    gl21::MatrixMode(gl21::PROJECTION);
    gl21::PopMatrix();
    gl21::MatrixMode(old_mode as GLenum);
}

/// Detect the prologue camera from its projection coefficients. Transparency
/// repairs must not touch the opening fly-in, where the same atlas also
/// contributes opaque cover geometry.
unsafe fn opening_projection_active() -> bool {
    let mut m = [0.0f32; 16];
    gl21::GetFloatv(gl21::PROJECTION_MATRIX, m.as_mut_ptr());
    m[10] >= -1.006 && m[10] <= -0.999 && m[14] >= -0.105 && m[14] <= -0.095
}

/// Dead Space debug aid: `TOUCHHLE_DS_ALPHA_OVERRIDE=<0..1>` replaces the
/// current colour's alpha for every draw. The intro backdrop is drawn with
/// SRC_ALPHA blending and a current-colour alpha of 0, which makes it fully
/// transparent; forcing the alpha back to 1 shows whether that is the cause.
unsafe fn alpha_override_begin() -> Option<[f32; 4]> {
    let a: f32 = match crate::ds_draw_env::var("TOUCHHLE_DS_ALPHA_OVERRIDE")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        Some(v) => v,
        None => return None,
    };
    let mut old = [1.0f32; 4];
    gl21::GetFloatv(gl21::CURRENT_COLOR, old.as_mut_ptr());
    gl21::Color4f(old[0], old[1], old[2], a);
    Some(old)
}

unsafe fn alpha_override_end(old: Option<[f32; 4]>) {
    if let Some(c) = old {
        gl21::Color4f(c[0], c[1], c[2], c[3]);
    }
}

struct TexUnitDisableBackup {
    active: GLint,
    enabled: Vec<(GLenum, bool)>,
}

/// Diagnostic switch for isolating multi-texture passes:
/// `TOUCHHLE_DS_DISABLE_TEX_UNIT="1[,2...]"`. The guest's texture-unit enable
/// bits and active unit are restored immediately after the draw.
unsafe fn tex_unit_disable_begin() -> Option<TexUnitDisableBackup> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_DISABLE_TEX_UNIT").ok()?;
    if spec.trim().is_empty() {
        return None;
    }
    let mut active: GLint = 0;
    gl21::GetIntegerv(gl21::ACTIVE_TEXTURE, &mut active);
    let mut enabled = Vec::new();
    for item in spec.split(',') {
        let Ok(unit) = item.trim().parse::<GLenum>() else {
            continue;
        };
        if unit >= 8 {
            continue;
        }
        let tex_enum = gl21::TEXTURE0 + unit;
        gl21::ActiveTexture(tex_enum);
        let was_enabled = gl21::IsEnabled(gl21::TEXTURE_2D) != 0;
        enabled.push((tex_enum, was_enabled));
        if was_enabled {
            gl21::Disable(gl21::TEXTURE_2D);
        }
    }
    gl21::ActiveTexture(active as GLenum);
    if enabled.is_empty() {
        None
    } else {
        log_once!("TOUCHHLE_DS_DISABLE_TEX_UNIT: disabling selected units during draws");
        Some(TexUnitDisableBackup { active, enabled })
    }
}

unsafe fn tex_unit_disable_end(backup: Option<TexUnitDisableBackup>) {
    let Some(backup) = backup else {
        return;
    };
    for (tex_enum, was_enabled) in backup.enabled {
        gl21::ActiveTexture(tex_enum);
        if was_enabled {
            gl21::Enable(gl21::TEXTURE_2D);
        } else {
            gl21::Disable(gl21::TEXTURE_2D);
        }
    }
    gl21::ActiveTexture(backup.active as GLenum);
}

/// Dead Space debug aid: `TOUCHHLE_DS_TEXFILTER=<gl texture id>` /
/// `TOUCHHLE_DS_TEXFILTER_HASH=<key[,key...]>` forces GL_NEAREST min/mag
/// filtering (i.e. no mipmaps) on that ONE texture only, so we can tell whether
/// heavy minification is averaging fine detail away. Prefer the `_HASH` form:
/// GL names are recycled, so the numeric form only holds while that name
/// happens to carry the wanted picture (see `crate::ds_texid`).
///
/// `TOUCHHLE_DS_TEXFILTER_LINEAR_HASH=<key[,key...]>` does the same with
/// GL_LINEAR and can be limited to small draw calls with
/// `TOUCHHLE_DS_TEXFILTER_LINEAR_MAX_COUNT`. This is the safe repair for an
/// atlas-backed poster quad: it avoids sampling a corrupt/bleeding mip chain
/// without changing filter state for the atlas's larger wall batches.
unsafe fn texfilter_begin(count: GLsizei) -> Option<(GLint, GLint)> {
    let id: Option<GLint> = crate::ds_draw_env::var("TOUCHHLE_DS_TEXFILTER")
        .ok()
        .and_then(|v| v.parse().ok());
    let keys = crate::ds_draw_env::var("TOUCHHLE_DS_TEXFILTER_HASH")
        .map(|v| crate::ds_texid::parse_keys(&v))
        .unwrap_or_default();
    let linear_spec = crate::ds_draw_env::var("TOUCHHLE_DS_TEXFILTER_LINEAR_HASH").ok();
    let mip_spec = crate::ds_draw_env::var("TOUCHHLE_DS_TEXFILTER_MIP_HASH").ok();
    if id.is_none() && keys.is_empty() && linear_spec.is_none() && mip_spec.is_none() {
        return None;
    }
    let mut old_min: GLint = 0;
    let mut old_mag: GLint = 0;
    gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_MIN_FILTER, &mut old_min);
    gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_MAG_FILTER, &mut old_mag);
    let filter = if mip_spec
        .as_deref()
        .map(crate::ds_texid::parse_keys)
        .map(|keys| tex_selected(None, &keys))
        .unwrap_or(false)
    {
        gl21::TexParameteri(
            gl21::TEXTURE_2D,
            gl21::TEXTURE_MAG_FILTER,
            gl21::LINEAR as GLint,
        );
        log_once!("TOUCHHLE_DS_TEXFILTER_MIP_HASH: forcing LINEAR_MIPMAP_LINEAR");
        gl21::LINEAR_MIPMAP_LINEAR as GLint
    } else if tex_selected(id, &keys) {
        gl21::TexParameteri(
            gl21::TEXTURE_2D,
            gl21::TEXTURE_MAG_FILTER,
            gl21::NEAREST as GLint,
        );
        log_once!("TOUCHHLE_DS_TEXFILTER: forcing NEAREST/NEAREST");
        gl21::NEAREST as GLint
    } else {
        let linear_keys = linear_spec
            .as_deref()
            .map(crate::ds_texid::parse_keys)
            .unwrap_or_default();
        let max_count = crate::ds_draw_env::var("TOUCHHLE_DS_TEXFILTER_LINEAR_MAX_COUNT")
            .ok()
            .and_then(|value| value.parse::<GLsizei>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(GLsizei::MAX);
        if linear_keys.is_empty()
            || count <= 0
            || count > max_count
            || !tex_selected(None, &linear_keys)
        {
            return None;
        }
        gl21::TexParameteri(
            gl21::TEXTURE_2D,
            gl21::TEXTURE_MAG_FILTER,
            gl21::LINEAR as GLint,
        );
        log_once!("TOUCHHLE_DS_TEXFILTER_LINEAR: forcing LINEAR/LINEAR");
        gl21::LINEAR as GLint
    };
    gl21::TexParameteri(gl21::TEXTURE_2D, gl21::TEXTURE_MIN_FILTER, filter);
    Some((old_min, old_mag))
}

unsafe fn texfilter_end(old: Option<(GLint, GLint)>) {
    if let Some((mn, mg)) = old {
        gl21::TexParameteri(gl21::TEXTURE_2D, gl21::TEXTURE_MIN_FILTER, mn);
        gl21::TexParameteri(gl21::TEXTURE_2D, gl21::TEXTURE_MAG_FILTER, mg);
    }
}

struct PixelTraceBackup {
    land: (f32, f32),
    pixel: [u8; 4],
    depth: f32,
}

static PIXEL_DARK_TRACE_ARMED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static PIXEL_DARK_TRACE_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static PIXEL_DARK_TRACE_FRAME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PIXEL_DARK_TRACE_MATCH_RUN: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static PIXEL_DARK_TRACE_OPPOSITE_RUN: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static PIXEL_DARK_TRACE_LAST_OPPOSITE_RUN: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static PIXEL_DARK_TRACE_DUMPED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static PIXEL_DARK_TRACE_SAW_BRIGHT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn pixel_trace_trigger_spec() -> Option<(f32, f32, f32)> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_DARK")
        .or_else(|_| crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_BRIGHT"))
        .ok()?;
    let mut parts = spec.split(',');
    let land_x = parts.next()?.trim().parse::<f32>().ok()?;
    let land_y = parts.next()?.trim().parse::<f32>().ok()?;
    let threshold = parts
        .next()
        .and_then(|value| value.trim().parse::<f32>().ok())
        .unwrap_or(24.0);
    Some((land_x, land_y, threshold))
}

fn pixel_trace_trigger_is_bright() -> bool {
    crate::ds_draw_env::contains("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_BRIGHT")
}

fn frame_trace_trigger_threshold() -> Option<f32> {
    crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_FRAME_DARK")
        .ok()?
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
}

fn frame_trace_trigger_point() -> (f32, f32) {
    crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_POINT")
        .ok()
        .and_then(|spec| {
            let mut parts = spec.split(',');
            let x = parts.next()?.trim().parse::<f32>().ok()?;
            let y = parts.next()?.trim().parse::<f32>().ok()?;
            Some((x, y))
        })
        .unwrap_or((240.0, 40.0))
}

unsafe fn pixel_trace_host_point(land_x: f32, land_y: f32) -> Option<(i32, i32)> {
    let mut viewport = [0i32; 4];
    gl21::GetIntegerv(gl21::VIEWPORT, viewport.as_mut_ptr());
    if viewport[2] <= 0 || viewport[3] <= 0 {
        return None;
    }
    // The guest renders into a portrait framebuffer. This is the inverse of
    // the transform used by the landscape-space UV probe.
    let host_x = ((land_y / 320.0) * viewport[2] as f32).round() as i32;
    let host_y = ((land_x / 480.0) * viewport[3] as f32).round() as i32;
    Some((host_x, host_y))
}

unsafe fn pixel_trace_dump_trigger(frame: u64) {
    let Some(dir) = crate::ds_draw_env::var_os("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_DUMP") else {
        return;
    };
    if PIXEL_DARK_TRACE_DUMPED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let mut viewport = [0i32; 4];
    gl21::GetIntegerv(gl21::VIEWPORT, viewport.as_mut_ptr());
    let (width, height) = (viewport[2], viewport[3]);
    if width <= 0 || height <= 0 || width > 4096 || height > 4096 {
        return;
    }
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    ds_observed_read_pixels(
        0,
        0,
        width,
        height,
        gl21::RGBA,
        gl21::UNSIGNED_BYTE,
        rgba.as_mut_ptr() as *mut GLvoid,
    );
    let mut ppm = Vec::with_capacity((width * height * 3) as usize + 32);
    ppm.extend_from_slice(format!("P6\n{} {}\n255\n", width, height).as_bytes());
    // GL framebuffers are bottom-up. Write top-down for ordinary image tools.
    for y in (0..height).rev() {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            ppm.extend_from_slice(&rgba[i..i + 3]);
        }
    }
    let directory = std::path::PathBuf::from(dir);
    let _ = std::fs::create_dir_all(&directory);
    let path = directory.join(format!("dark_trigger_f{}.ppm", frame));
    match std::fs::write(&path, ppm) {
        Ok(()) => log!("[DSpixelTrigger] saved {}", path.display()),
        Err(e) => log!("[DSpixelTrigger] dump failed: {}", e),
    }
}

/// Diagnostic: `TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_DARK="x,y[,luma]"` watches one
/// landscape-space pixel at frame boundaries and arms the per-draw trace for
/// exactly one frame when that pixel turns dark. This avoids fixed guest-frame
/// windows, which move between runs because the opening takes a variable time
/// to reach each camera cut.
unsafe fn pixel_trace_trigger_tick() {
    // Arm in the actual scene with F7, rather than guessing a frame number
    // that varies with menu timing and load speed. No effect unless opted in.
    if crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE_WAIT_FOR_MARK").as_deref() == Ok("1")
        && !crate::window::DS_TRACE_MARKED.load(std::sync::atomic::Ordering::Relaxed)
    {
        PIXEL_DARK_TRACE_ARMED.store(false, std::sync::atomic::Ordering::Relaxed);
        return;
    }
    let point_spec = pixel_trace_trigger_spec();
    let frame_threshold = frame_trace_trigger_threshold();
    if point_spec.is_none() && frame_threshold.is_none() {
        PIXEL_DARK_TRACE_ARMED.store(false, std::sync::atomic::Ordering::Relaxed);
        PIXEL_DARK_TRACE_REPORTED.store(false, std::sync::atomic::Ordering::Relaxed);
        PIXEL_DARK_TRACE_MATCH_RUN.store(0, std::sync::atomic::Ordering::Relaxed);
        PIXEL_DARK_TRACE_OPPOSITE_RUN.store(0, std::sync::atomic::Ordering::Relaxed);
        PIXEL_DARK_TRACE_LAST_OPPOSITE_RUN.store(0, std::sync::atomic::Ordering::Relaxed);
        PIXEL_DARK_TRACE_SAW_BRIGHT.store(false, std::sync::atomic::Ordering::Relaxed);
        return;
    }
    let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    if let Ok(value) = crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE_FROM_FRAME") {
        if let Ok(from) = value.parse::<u64>() {
            if frame < from {
                PIXEL_DARK_TRACE_ARMED.store(false, std::sync::atomic::Ordering::Relaxed);
                PIXEL_DARK_TRACE_REPORTED.store(false, std::sync::atomic::Ordering::Relaxed);
                PIXEL_DARK_TRACE_MATCH_RUN.store(0, std::sync::atomic::Ordering::Relaxed);
                PIXEL_DARK_TRACE_OPPOSITE_RUN.store(0, std::sync::atomic::Ordering::Relaxed);
                PIXEL_DARK_TRACE_LAST_OPPOSITE_RUN.store(0, std::sync::atomic::Ordering::Relaxed);
                PIXEL_DARK_TRACE_SAW_BRIGHT.store(false, std::sync::atomic::Ordering::Relaxed);
                return;
            }
        }
    }
    if let Ok(value) = crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE_TO_FRAME") {
        if let Ok(to) = value.parse::<u64>() {
            if frame > to {
                PIXEL_DARK_TRACE_ARMED.store(false, std::sync::atomic::Ordering::Relaxed);
                PIXEL_DARK_TRACE_REPORTED.store(false, std::sync::atomic::Ordering::Relaxed);
                PIXEL_DARK_TRACE_MATCH_RUN.store(0, std::sync::atomic::Ordering::Relaxed);
                PIXEL_DARK_TRACE_OPPOSITE_RUN.store(0, std::sync::atomic::Ordering::Relaxed);
                PIXEL_DARK_TRACE_LAST_OPPOSITE_RUN.store(0, std::sync::atomic::Ordering::Relaxed);
                PIXEL_DARK_TRACE_SAW_BRIGHT.store(false, std::sync::atomic::Ordering::Relaxed);
                return;
            }
        }
    }
    let reported = PIXEL_DARK_TRACE_REPORTED.load(std::sync::atomic::Ordering::Relaxed);
    if reported {
        let trigger_frame = PIXEL_DARK_TRACE_FRAME.load(std::sync::atomic::Ordering::Relaxed);
        if trigger_frame == 0 || frame > trigger_frame {
            PIXEL_DARK_TRACE_ARMED.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        return;
    }
    let (dark, bright, threshold, detail) = if let Some(threshold) = frame_threshold {
        let mut viewport = [0i32; 4];
        gl21::GetIntegerv(gl21::VIEWPORT, viewport.as_mut_ptr());
        let (width, height) = (viewport[2], viewport[3]);
        if width <= 0 || height <= 0 || width > 4096 || height > 4096 {
            return;
        }
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        ds_observed_read_pixels(
            0,
            0,
            width,
            height,
            gl21::RGBA,
            gl21::UNSIGNED_BYTE,
            rgba.as_mut_ptr() as *mut GLvoid,
        );
        let step = 8i32;
        let mut sum = 0.0f32;
        let mut count = 0u32;
        for y in (0..height).step_by(step as usize) {
            for x in (0..width).step_by(step as usize) {
                let i = ((y * width + x) * 4) as usize;
                sum += 0.2126 * rgba[i] as f32
                    + 0.7152 * rgba[i + 1] as f32
                    + 0.0722 * rgba[i + 2] as f32;
                count += 1;
            }
        }
        let mean = if count == 0 { 0.0 } else { sum / count as f32 };
        (
            mean <= threshold,
            mean >= threshold * 2.0,
            threshold,
            format!("mean={:.2}", mean),
        )
    } else {
        let (land_x, land_y, threshold) = point_spec.unwrap();
        let Some((host_x, host_y)) = pixel_trace_host_point(land_x, land_y) else {
            return;
        };
        let mut pixel = [0u8; 4];
        ds_observed_read_pixels(
            host_x,
            host_y,
            1,
            1,
            gl21::RGBA,
            gl21::UNSIGNED_BYTE,
            pixel.as_mut_ptr() as *mut GLvoid,
        );
        let luma = 0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32;
        let bright_trigger = pixel_trace_trigger_is_bright();
        (
            if bright_trigger {
                luma >= threshold
            } else {
                luma <= threshold
            },
            if bright_trigger {
                luma <= threshold * 0.5
            } else {
                luma >= threshold * 2.0
            },
            threshold,
            format!(
                "point=({:.1},{:.1}) pixel={:?} luma={:.2}",
                land_x, land_y, pixel, luma
            ),
        )
    };
    let require_bright = crate::ds_draw_env::contains("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_AFTER_BRIGHT");
    if require_bright && bright {
        PIXEL_DARK_TRACE_SAW_BRIGHT.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let match_run = if dark {
        PIXEL_DARK_TRACE_MATCH_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
    } else {
        PIXEL_DARK_TRACE_MATCH_RUN.store(0, std::sync::atomic::Ordering::Relaxed);
        0
    };
    if bright {
        let run =
            PIXEL_DARK_TRACE_OPPOSITE_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        PIXEL_DARK_TRACE_LAST_OPPOSITE_RUN.store(run, std::sync::atomic::Ordering::Relaxed);
    } else {
        let run = PIXEL_DARK_TRACE_OPPOSITE_RUN.swap(0, std::sync::atomic::Ordering::Relaxed);
        if run > 0 {
            PIXEL_DARK_TRACE_LAST_OPPOSITE_RUN.store(run, std::sync::atomic::Ordering::Relaxed);
        }
    }
    let last_opposite_run =
        PIXEL_DARK_TRACE_LAST_OPPOSITE_RUN.load(std::sync::atomic::Ordering::Relaxed);
    let confirm_frames = crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_CONFIRM_FRAMES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let opposite_frames = crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE_TRIGGER_OPPOSITE_FRAMES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let eligible = (!require_bright
        || PIXEL_DARK_TRACE_SAW_BRIGHT.load(std::sync::atomic::Ordering::Relaxed))
        && match_run >= confirm_frames
        && last_opposite_run >= opposite_frames;
    let armed = dark && eligible && !reported;
    PIXEL_DARK_TRACE_ARMED.store(armed, std::sync::atomic::Ordering::Relaxed);
    PIXEL_DARK_TRACE_REPORTED.store(dark && eligible, std::sync::atomic::Ordering::Relaxed);
    if armed {
        PIXEL_DARK_TRACE_FRAME.store(frame, std::sync::atomic::Ordering::Relaxed);
        log!(
            "[DSpixelTrigger] f={} {} threshold={:.2} armed",
            frame,
            detail,
            threshold
        );
        pixel_trace_dump_trigger(frame);
    }
}

/// Diagnostic: `TOUCHHLE_DS_PIXEL_TRACE="x,y"` logs a draw whenever it changes
/// the RGBA value at one landscape-space point. Optional
/// `TOUCHHLE_DS_PIXEL_TRACE_FROM_FRAME` and `_TO_FRAME` bound the very
/// expensive readback window.
unsafe fn pixel_trace_begin() -> Option<PixelTraceBackup> {
    let (land_x, land_y) = match crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE") {
        Ok(spec) => {
            let mut parts = spec.split(',');
            (
                parts.next()?.trim().parse::<f32>().ok()?,
                parts.next()?.trim().parse::<f32>().ok()?,
            )
        }
        Err(_) => {
            if !PIXEL_DARK_TRACE_ARMED.load(std::sync::atomic::Ordering::Relaxed) {
                return None;
            }
            pixel_trace_trigger_spec()
                .map(|(land_x, land_y, _)| (land_x, land_y))
                .unwrap_or_else(frame_trace_trigger_point)
        }
    };
    if pixel_trace_trigger_spec().is_some()
        && !PIXEL_DARK_TRACE_ARMED.load(std::sync::atomic::Ordering::Relaxed)
    {
        return None;
    }
    let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    if let Ok(value) = crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE_FROM_FRAME") {
        if let Ok(from) = value.parse::<u64>() {
            if frame < from {
                return None;
            }
        }
    }
    if let Ok(value) = crate::ds_draw_env::var("TOUCHHLE_DS_PIXEL_TRACE_TO_FRAME") {
        if let Ok(to) = value.parse::<u64>() {
            if frame > to {
                return None;
            }
        }
    }
    let (host_x, host_y) = pixel_trace_host_point(land_x, land_y)?;
    let mut pixel = [0u8; 4];
    ds_observed_read_pixels(
        host_x,
        host_y,
        1,
        1,
        gl21::RGBA,
        gl21::UNSIGNED_BYTE,
        pixel.as_mut_ptr() as *mut GLvoid,
    );
    let mut depth = 0.0f32;
    ds_observed_read_pixels(
        host_x,
        host_y,
        1,
        1,
        gl21::DEPTH_COMPONENT,
        gl21::FLOAT,
        &mut depth as *mut f32 as *mut GLvoid,
    );
    Some(PixelTraceBackup {
        land: (land_x, land_y),
        pixel,
        depth,
    })
}

unsafe fn pixel_trace_end(
    backup: Option<PixelTraceBackup>,
    kind: &str,
    mode: GLenum,
    count: GLsizei,
) {
    let Some(backup) = backup else {
        return;
    };
    let Some((host_x, host_y)) = pixel_trace_host_point(backup.land.0, backup.land.1) else {
        return;
    };
    let mut after = [0u8; 4];
    ds_observed_read_pixels(
        host_x,
        host_y,
        1,
        1,
        gl21::RGBA,
        gl21::UNSIGNED_BYTE,
        after.as_mut_ptr() as *mut GLvoid,
    );
    let mut after_depth = 0.0f32;
    ds_observed_read_pixels(
        host_x,
        host_y,
        1,
        1,
        gl21::DEPTH_COMPONENT,
        gl21::FLOAT,
        &mut after_depth as *mut f32 as *mut GLvoid,
    );
    if after == backup.pixel {
        return;
    }
    let key = bound_texture_key().unwrap_or(0);
    let mut tex: GLint = 0;
    let mut depth: GLint = 0;
    let mut depth_mask: GLboolean = 0;
    let mut blend: GLboolean = 0;
    let mut color_size: GLint = 0;
    let mut color_type: GLint = 0;
    let mut color_stride: GLint = 0;
    let mut current_color = [0.0f32; 4];
    let mut color_mask = [0u8; 4];
    let mut blend_src: GLint = 0;
    let mut blend_dst: GLint = 0;
    let mut alpha_func: GLint = 0;
    let mut alpha_ref: GLfloat = 0.0;
    let mut cull_mode: GLint = 0;
    let mut front_face: GLint = 0;
    let mut tex_env_mode: GLint = 0;
    let mut active_unit: GLint = 0;
    let mut client_unit: GLint = 0;
    let mut max_units: GLint = 1;
    let mut scissor_box = [0i32; 4];
    let mut stencil_func: GLint = 0;
    let mut stencil_ref: GLint = 0;
    let mut stencil_value_mask: GLint = 0;
    let mut stencil_fail: GLint = 0;
    let mut stencil_pass_depth_fail: GLint = 0;
    let mut stencil_pass_depth_pass: GLint = 0;
    let mut framebuffer: GLint = 0;
    let mut viewport = [0i32; 4];
    let mut depth_range = [0.0f32; 2];
    let texture_2d_enabled = gl21::IsEnabled(gl21::TEXTURE_2D) != 0;
    gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut tex);
    gl21::GetIntegerv(gl21::DEPTH_FUNC, &mut depth);
    gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut depth_mask);
    gl21::GetBooleanv(gl21::BLEND, &mut blend);
    gl21::GetIntegerv(gl21::COLOR_ARRAY_SIZE, &mut color_size);
    gl21::GetIntegerv(gl21::COLOR_ARRAY_TYPE, &mut color_type);
    gl21::GetIntegerv(gl21::COLOR_ARRAY_STRIDE, &mut color_stride);
    gl21::GetFloatv(gl21::CURRENT_COLOR, current_color.as_mut_ptr());
    gl21::GetBooleanv(gl21::COLOR_WRITEMASK, color_mask.as_mut_ptr());
    gl21::GetIntegerv(gl21::BLEND_SRC, &mut blend_src);
    gl21::GetIntegerv(gl21::BLEND_DST, &mut blend_dst);
    gl21::GetIntegerv(gl21::ALPHA_TEST_FUNC, &mut alpha_func);
    gl21::GetFloatv(gl21::ALPHA_TEST_REF, &mut alpha_ref);
    gl21::GetIntegerv(gl21::CULL_FACE_MODE, &mut cull_mode);
    gl21::GetIntegerv(gl21::FRONT_FACE, &mut front_face);
    gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::TEXTURE_ENV_MODE, &mut tex_env_mode);
    gl21::GetIntegerv(gl21::SCISSOR_BOX, scissor_box.as_mut_ptr());
    gl21::GetIntegerv(gl21::STENCIL_FUNC, &mut stencil_func);
    gl21::GetIntegerv(gl21::STENCIL_REF, &mut stencil_ref);
    gl21::GetIntegerv(gl21::STENCIL_VALUE_MASK, &mut stencil_value_mask);
    gl21::GetIntegerv(gl21::STENCIL_FAIL, &mut stencil_fail);
    gl21::GetIntegerv(gl21::STENCIL_PASS_DEPTH_FAIL, &mut stencil_pass_depth_fail);
    gl21::GetIntegerv(gl21::STENCIL_PASS_DEPTH_PASS, &mut stencil_pass_depth_pass);
    gl21::GetIntegerv(gl21::FRAMEBUFFER_BINDING_EXT, &mut framebuffer);
    gl21::GetIntegerv(gl21::VIEWPORT, viewport.as_mut_ptr());
    gl21::GetFloatv(gl21::DEPTH_RANGE, depth_range.as_mut_ptr());
    gl21::GetIntegerv(gl21::ACTIVE_TEXTURE, &mut active_unit);
    gl21::GetIntegerv(gl21::CLIENT_ACTIVE_TEXTURE, &mut client_unit);
    gl21::GetIntegerv(gl21::MAX_TEXTURE_UNITS, &mut max_units);
    let mut unit_states = Vec::new();
    for unit in 0..max_units.clamp(1, 8) {
        gl21::ActiveTexture(gl21::TEXTURE0 + unit as GLenum);
        let mut bound: GLint = 0;
        let mut mode: GLint = 0;
        let mut combine_rgb: GLint = 0;
        let mut combine_alpha: GLint = 0;
        let mut src0_rgb: GLint = 0;
        let mut src1_rgb: GLint = 0;
        let mut src2_rgb: GLint = 0;
        let mut op0_rgb: GLint = 0;
        let mut src0_alpha: GLint = 0;
        let mut src1_alpha: GLint = 0;
        let mut src2_alpha: GLint = 0;
        let mut op0_alpha: GLint = 0;
        let mut matrix = [0.0f32; 16];
        gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut bound);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::TEXTURE_ENV_MODE, &mut mode);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::COMBINE_RGB, &mut combine_rgb);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::COMBINE_ALPHA, &mut combine_alpha);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::SOURCE0_RGB, &mut src0_rgb);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::SOURCE1_RGB, &mut src1_rgb);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::SOURCE2_RGB, &mut src2_rgb);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::OPERAND0_RGB, &mut op0_rgb);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::SOURCE0_ALPHA, &mut src0_alpha);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::SOURCE1_ALPHA, &mut src1_alpha);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::SOURCE2_ALPHA, &mut src2_alpha);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::OPERAND0_ALPHA, &mut op0_alpha);
        gl21::GetFloatv(gl21::TEXTURE_MATRIX, matrix.as_mut_ptr());
        unit_states.push(format!(
            "{}:tex{} mode={:#x} rgb={:#x}/{:#x}/{:#x}/{:#x} a={:#x}/{:#x}/{:#x}/{:#x} op={:#x} mat={:?}",
            unit,
            bound,
            mode,
            combine_rgb,
            src0_rgb,
            src1_rgb,
            src2_rgb,
            combine_alpha,
            src0_alpha,
            src1_alpha,
            src2_alpha,
            op0_rgb,
            matrix
        ));
    }
    gl21::ActiveTexture(active_unit as GLenum);
    let unit_states = unit_states.join("; ");
    log!(
        "[DSpixel] f={} point=({:.1},{:.1}) kind={} mode=0x{:x} count={} tex={} key={:08x} before={:?} after={:?} depthFunc={} mask={} blend={} depth={:.7}->{:.7} fbo={} viewport={:?} depthRange={:?} active={}/{} units=[{}]",
        crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),
        backup.land.0,
        backup.land.1,
        kind,
        mode,
        count,
        tex,
        key,
        backup.pixel,
        after,
        depth,
        depth_mask,
        blend,
        backup.depth,
        after_depth,
        framebuffer,
        viewport,
        depth_range,
        active_unit,
        client_unit,
        unit_states
    );
    log!(
        "[DSpixelState] f={} key={:08x} texture2D={} colArr={} colorSize={} colorType=0x{:x} colorStride={} lighting={} colorMaterial={} currentColor={:?} blend={}/{}/{}/{} alphaTest={}/0x{:x}/{:.2} cull={}/0x{:x}/0x{:x} scissor={}/{:?} colorMask={:?} texenv=0x{:x} stencil={}/0x{:x}/{}/{}/0x{:x}/0x{:x}/0x{:x}",
        crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),
        key,
        texture_2d_enabled,
        gl21::IsEnabled(gl21::COLOR_ARRAY) != 0,
        color_size,
        color_type,
        color_stride,
        gl21::IsEnabled(gl21::LIGHTING) != 0,
        gl21::IsEnabled(gl21::COLOR_MATERIAL) != 0,
        current_color,
        gl21::IsEnabled(gl21::BLEND) != 0,
        blend_src,
        blend_dst,
        blend,
        gl21::IsEnabled(gl21::ALPHA_TEST) != 0,
        alpha_func,
        alpha_ref,
        gl21::IsEnabled(gl21::CULL_FACE) != 0,
        cull_mode,
        front_face,
        gl21::IsEnabled(gl21::SCISSOR_TEST) != 0,
        scissor_box,
        color_mask,
        tex_env_mode,
        gl21::IsEnabled(gl21::STENCIL_TEST) != 0,
        stencil_func,
        stencil_ref,
        stencil_value_mask,
        stencil_fail,
        stencil_pass_depth_fail,
        stencil_pass_depth_pass
    );
    if gl21::IsEnabled(gl21::COLOR_ARRAY) != 0 && color_size > 0 {
        let mut color_pointer: *mut GLvoid = std::ptr::null_mut();
        #[allow(clippy::unnecessary_mut_passed)]
        gl21::GetPointerv(gl21::COLOR_ARRAY_POINTER, &mut color_pointer);
        let mut buffer_binding: GLint = 0;
        gl21::GetIntegerv(gl21::ARRAY_BUFFER_BINDING, &mut buffer_binding);
        let mapped = if buffer_binding != 0 {
            gl21::MapBuffer(gl21::ARRAY_BUFFER, gl21::READ_ONLY)
        } else {
            std::ptr::null_mut()
        };
        let base = if !mapped.is_null() {
            mapped.add(color_pointer as usize) as *const u8
        } else {
            color_pointer as *const u8
        };
        if !base.is_null() {
            let element_size = match color_type as GLenum {
                gl21::BYTE | gl21::UNSIGNED_BYTE => 1usize,
                gl21::SHORT | gl21::UNSIGNED_SHORT => 2usize,
                _ => 4usize,
            };
            let stride = if color_stride == 0 {
                color_size as usize * element_size
            } else {
                color_stride as usize
            };
            let mut samples = Vec::with_capacity(4);
            for i in 0..4usize {
                let p = base.add(i * stride);
                let value = |component: usize| -> f32 {
                    match color_type as GLenum {
                        gl21::UNSIGNED_BYTE => *(p.add(component * element_size)) as f32 / 255.0,
                        gl21::BYTE => {
                            *(p.add(component * element_size) as *const i8) as f32 / 127.0
                        }
                        gl21::UNSIGNED_SHORT => {
                            (p.add(component * element_size) as *const u16).read_unaligned() as f32
                                / 65535.0
                        }
                        gl21::SHORT => {
                            (p.add(component * element_size) as *const i16).read_unaligned() as f32
                                / 32767.0
                        }
                        _ => (p.add(component * element_size) as *const f32).read_unaligned(),
                    }
                };
                samples.push([
                    value(0),
                    value(1),
                    value(2),
                    if color_size >= 4 { value(3) } else { 1.0 },
                ]);
            }
            log!(
                "[DSpixelColor] f={} key={:08x} samples={:?}",
                crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),
                key,
                samples
            );
        }
        if !mapped.is_null() {
            gl21::UnmapBuffer(gl21::ARRAY_BUFFER);
        }
    }
}

/// Per-texture LOD-bias diagnostic for the opening hull atlases. Positive
/// values select lower-resolution mips and can expose whether residual combing
/// is a sampling-frequency problem rather than mesh/depth overlap.
unsafe fn texlod_begin() -> Option<GLfloat> {
    let keys = match crate::ds_draw_env::var("TOUCHHLE_DS_TEXLOD_HASH") {
        Ok(value) => crate::ds_texid::parse_keys(&value),
        Err(_) if crate::ds_draw_env::var("TOUCHHLE_DS_OPENING_LOD_BIAS").as_deref() != Ok("0") => {
            vec![0x7bcf_a36c, 0x780d_00eb]
        }
        Err(_) => Vec::new(),
    };
    if keys.is_empty() || !tex_selected(None, &keys) {
        return None;
    }
    let bias = crate::ds_draw_env::var("TOUCHHLE_DS_TEXLOD_BIAS")
        .or_else(|_| crate::ds_draw_env::var("TOUCHHLE_DS_OPENING_LOD_BIAS"))
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(2.0);
    let mut old: GLfloat = 0.0;
    gl21::GetTexEnvfv(
        gl21::TEXTURE_FILTER_CONTROL_EXT,
        gl21::TEXTURE_LOD_BIAS_EXT,
        &mut old,
    );
    gl21::TexEnvf(
        gl21::TEXTURE_FILTER_CONTROL_EXT,
        gl21::TEXTURE_LOD_BIAS_EXT,
        bias,
    );
    log_once!("TOUCHHLE_DS_TEXLOD_HASH: applying per-texture LOD bias");
    Some(old)
}

unsafe fn texlod_end(old: Option<GLfloat>) {
    if let Some(old) = old {
        gl21::TexEnvf(
            gl21::TEXTURE_FILTER_CONTROL_EXT,
            gl21::TEXTURE_LOD_BIAS_EXT,
            old,
        );
    }
}

#[derive(Copy, Clone)]
struct TexFlatBackup {
    texture_2d: GLboolean,
    lighting: GLboolean,
    blend: GLboolean,
    alpha_test: GLboolean,
    color: [GLfloat; 4],
}

/// Diagnostic: render selected textures as unlit solid white while preserving
/// depth/culling. Useful for separating mesh/depth seams from texture combing.
unsafe fn texflat_begin() -> Option<TexFlatBackup> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_TEXFLAT_HASH").ok()?;
    if !tex_hash_in_spec(&spec) {
        return None;
    }
    let mut color = [1.0f32; 4];
    gl21::GetFloatv(gl21::CURRENT_COLOR, color.as_mut_ptr());
    let backup = TexFlatBackup {
        texture_2d: (gl21::IsEnabled(gl21::TEXTURE_2D) != 0) as GLboolean,
        lighting: (gl21::IsEnabled(gl21::LIGHTING) != 0) as GLboolean,
        blend: (gl21::IsEnabled(gl21::BLEND) != 0) as GLboolean,
        alpha_test: (gl21::IsEnabled(gl21::ALPHA_TEST) != 0) as GLboolean,
        color,
    };
    gl21::Disable(gl21::TEXTURE_2D);
    if backup.lighting != 0 {
        gl21::Disable(gl21::LIGHTING);
    }
    if backup.blend != 0 {
        gl21::Disable(gl21::BLEND);
    }
    if backup.alpha_test != 0 {
        gl21::Disable(gl21::ALPHA_TEST);
    }
    gl21::Color4f(1.0, 1.0, 1.0, 1.0);
    log_once!("TOUCHHLE_DS_TEXFLAT_HASH: rendering selected textures unlit");
    Some(backup)
}

unsafe fn texflat_end(backup: Option<TexFlatBackup>) {
    if let Some(backup) = backup {
        gl21::Color4f(
            backup.color[0],
            backup.color[1],
            backup.color[2],
            backup.color[3],
        );
        if backup.texture_2d != 0 {
            gl21::Enable(gl21::TEXTURE_2D);
        }
        if backup.lighting != 0 {
            gl21::Enable(gl21::LIGHTING);
        }
        if backup.blend != 0 {
            gl21::Enable(gl21::BLEND);
        }
        if backup.alpha_test != 0 {
            gl21::Enable(gl21::ALPHA_TEST);
        }
    }
}

/// Draw selected full-screen dither masks as flat black. Dead Space uses
/// one-pixel black/white masks for PowerVR coverage effects; desktop GL exposes
/// the mask as visible scan lines. Replacing only the small mask batches keeps
/// the coverage and depth behavior while removing the pattern.
unsafe fn texblack_begin(count: GLsizei) -> Option<TexFlatBackup> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_TEXBLACK_HASH_COUNT").ok()?;
    let mut selected = false;
    for item in spec.split(',') {
        let mut parts = item.splitn(2, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok());
        if let (Some(key), Some(wanted_count)) = (key, wanted_count) {
            if count == wanted_count && tex_selected(None, &[key]) {
                selected = true;
                break;
            }
        }
    }
    if !selected {
        return None;
    }
    let mut color = [1.0f32; 4];
    gl21::GetFloatv(gl21::CURRENT_COLOR, color.as_mut_ptr());
    let backup = TexFlatBackup {
        texture_2d: (gl21::IsEnabled(gl21::TEXTURE_2D) != 0) as GLboolean,
        lighting: (gl21::IsEnabled(gl21::LIGHTING) != 0) as GLboolean,
        blend: (gl21::IsEnabled(gl21::BLEND) != 0) as GLboolean,
        alpha_test: (gl21::IsEnabled(gl21::ALPHA_TEST) != 0) as GLboolean,
        color,
    };
    gl21::Disable(gl21::TEXTURE_2D);
    if backup.lighting != 0 {
        gl21::Disable(gl21::LIGHTING);
    }
    if backup.blend != 0 {
        gl21::Disable(gl21::BLEND);
    }
    if backup.alpha_test != 0 {
        gl21::Disable(gl21::ALPHA_TEST);
    }
    gl21::Color4f(0.0, 0.0, 0.0, 1.0);
    log_once!("TOUCHHLE_DS_TEXBLACK_HASH_COUNT: replacing dither mask with black");
    Some(backup)
}

/// Diagnostic A/B control for a translucent overlay whose source alpha is
/// carried in `GL_CURRENT_COLOR`: `TOUCHHLE_DS_ALPHA_HASH_COUNT=key:count:alpha`.
/// This leaves geometry, texture coordinates and depth untouched.
unsafe fn alpha_hash_count_begin(count: GLsizei) -> Option<[f32; 4]> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_ALPHA_HASH_COUNT").ok()?;
    for item in spec.split(';') {
        let mut parts = item.splitn(3, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok());
        let alpha = parts
            .next()
            .and_then(|value| value.trim().parse::<f32>().ok());
        if key.is_some_and(|key| tex_selected(None, &[key]))
            && wanted_count == Some(count)
            && alpha.is_some()
        {
            let mut color = [0.0f32; 4];
            gl21::GetFloatv(gl21::CURRENT_COLOR, color.as_mut_ptr());
            gl21::Color4f(color[0], color[1], color[2], alpha.unwrap_or(color[3]));
            log_once!("TOUCHHLE_DS_ALPHA_HASH_COUNT: overriding current alpha");
            return Some(color);
        }
    }
    None
}

unsafe fn alpha_hash_count_end(backup: Option<[f32; 4]>) {
    if let Some(color) = backup {
        gl21::Color4f(color[0], color[1], color[2], color[3]);
    }
}

/// Diagnostic A/B control:
/// `TOUCHHLE_DS_NO_BLEND_HASH_COUNT=key:count[;key:count...]` disables
/// blending for the selected draw only.
unsafe fn no_blend_hash_count_begin(count: GLsizei) -> Option<bool> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_NO_BLEND_HASH_COUNT").ok()?;
    for item in spec.split(';') {
        let mut parts = item.splitn(2, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok());
        if key.is_some_and(|key| tex_selected(None, &[key])) && wanted_count == Some(count) {
            let was_enabled = gl21::IsEnabled(gl21::BLEND) != 0;
            if was_enabled {
                gl21::Disable(gl21::BLEND);
            }
            log_once!("TOUCHHLE_DS_NO_BLEND_HASH_COUNT: disabling blend");
            return Some(was_enabled);
        }
    }
    None
}

unsafe fn no_blend_hash_count_end(backup: Option<bool>) {
    if backup == Some(true) {
        gl21::Enable(gl21::BLEND);
    }
}

/// Diagnostic/fix control for geometry whose winding does not match the host
/// interpretation:
/// `TOUCHHLE_DS_NO_CULL_HASH_COUNT=key:count[;key:count...]` disables face
/// culling for one selected texture batch only, then restores it.
unsafe fn no_cull_hash_count_begin(count: GLsizei) -> Option<bool> {
    // The PC-compatible station-hull batch uses opposite winding from the
    // original PowerVR path in the opening space scene. Keep culling enabled
    // everywhere else and relax it only for this stable content/count pair.
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_NO_CULL_HASH_COUNT")
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
        if key.is_some_and(|key| tex_selected(None, &[key])) && wanted_count == Some(count) {
            let was_enabled = gl21::IsEnabled(gl21::CULL_FACE) != 0;
            if was_enabled {
                gl21::Disable(gl21::CULL_FACE);
            }
            log_once!("TOUCHHLE_DS_NO_CULL_HASH_COUNT: disabling culling for selected batch");
            return Some(was_enabled);
        }
    }
    None
}

unsafe fn no_cull_hash_count_end(backup: Option<bool>) {
    if backup == Some(true) {
        gl21::Enable(gl21::CULL_FACE);
    }
}

/// Flip front/back winding for one selected opening-scene batch. The asteroid
/// mesh uses opposite winding from the host GL path, but disabling culling
/// makes its back faces cover the camera. Keeping culling enabled after the
/// flip restores the intended front surface without pulling the whole mesh
/// into the foreground.
unsafe fn flip_winding_hash_count_begin(count: GLsizei) -> Option<GLint> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_FLIP_WINDING_HASH_COUNT").ok()?;
    if !opening_projection_active() {
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
        if key.is_some_and(|key| tex_selected(None, &[key])) && wanted_count == Some(count) {
            let mut old_front_face: GLint = 0;
            gl21::GetIntegerv(gl21::FRONT_FACE, &mut old_front_face);
            let flipped = if old_front_face == gl21::CW as GLint {
                gl21::CCW
            } else {
                gl21::CW
            };
            gl21::FrontFace(flipped);
            log_once!("TOUCHHLE_DS_FLIP_WINDING_HASH_COUNT: flipping selected batch winding");
            return Some(old_front_face);
        }
    }
    None
}

unsafe fn flip_winding_hash_count_end(backup: Option<GLint>) {
    if let Some(old_front_face) = backup {
        gl21::FrontFace(old_front_face as GLenum);
    }
}

/// Force one selected draw batch through stale multisample depth while still
/// writing correct new depth. This is narrower than disabling depth entirely:
/// later geometry can still be occluded by the restored foreground rock.
unsafe fn depth_always_hash_count_begin(
    count: GLsizei,
) -> Option<(GLboolean, GLint, GLboolean)> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_DEPTH_ALWAYS_HASH_COUNT").ok()?;
    for item in spec.split(';') {
        let mut parts = item.splitn(2, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok());
        if key.is_some_and(|key| tex_selected(None, &[key])) && wanted_count == Some(count) {
            let was_test = (gl21::IsEnabled(gl21::DEPTH_TEST) != 0) as GLboolean;
            let mut old_func: GLint = 0;
            let mut old_mask: GLboolean = 0;
            gl21::GetIntegerv(gl21::DEPTH_FUNC, &mut old_func);
            gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut old_mask);
            gl21::Enable(gl21::DEPTH_TEST);
            gl21::DepthFunc(gl21::ALWAYS);
            gl21::DepthMask(gl21::TRUE);
            log_once!(
                "TOUCHHLE_DS_DEPTH_ALWAYS_HASH_COUNT: bypassing stale depth for selected batch"
            );
            return Some((was_test, old_func, old_mask));
        }
    }
    None
}

unsafe fn depth_always_hash_count_end(backup: Option<(GLboolean, GLint, GLboolean)>) {
    if let Some((was_test, old_func, old_mask)) = backup {
        gl21::DepthMask(old_mask);
        gl21::DepthFunc(old_func as GLenum);
        if was_test != 0 {
            gl21::Enable(gl21::DEPTH_TEST);
        } else {
            gl21::Disable(gl21::DEPTH_TEST);
        }
    }
}

/// Clear one frame of stale depth immediately before the selected opening
/// asteroid batch. The batch then uses normal GL_LESS/GL_LEQUAL, so nearer
/// station geometry still occludes the rock correctly.
unsafe fn clear_depth_hash_count_once(count: GLsizei) {
    let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_CLEAR_DEPTH_HASH_COUNT") else {
        return;
    };
    if !opening_projection_active() {
        return;
    }
    let matched = spec.split(';').any(|item| {
        let mut parts = item.splitn(2, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok());
        key.is_some_and(|key| tex_selected(None, &[key])) && wanted_count == Some(count)
    });
    if !matched {
        return;
    }
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
    let frame = crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed);
    if LAST_FRAME.swap(frame, Ordering::Relaxed) == frame {
        return;
    }
    let mut old_mask: GLboolean = 0;
    gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut old_mask);
    gl21::DepthMask(gl21::TRUE);
    gl21::Clear(gl21::DEPTH_BUFFER_BIT);
    gl21::DepthMask(old_mask);
    log_once!("TOUCHHLE_DS_CLEAR_DEPTH_HASH_COUNT: cleared stale opening depth once");
}

/// During the opening space fly-through, each guest FBO must start its first
/// 3D draw with a clean depth buffer. The title shares depth storage between
/// composition passes; stale camera depth otherwise produces rectangular holes
/// in the foreground asteroid. Clear each FBO at most once per presented frame.
unsafe fn clear_opening_depth_once_per_fbo() {
    if crate::ds_draw_env::var("TOUCHHLE_DS_CLEAR_OPENING_DEPTH").as_deref() != Ok("1")
        || !opening_projection_active()
    {
        return;
    }
    let mut framebuffer: GLint = 0;
    gl21::GetIntegerv(gl21::FRAMEBUFFER_BINDING_EXT, &mut framebuffer);
    let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    use std::sync::{Mutex, OnceLock};
    static LAST_CLEARED: OnceLock<Mutex<std::collections::HashMap<GLuint, u64>>> = OnceLock::new();
    let last_cleared = LAST_CLEARED.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let Ok(mut last_cleared) = last_cleared.lock() else {
        return;
    };
    let fbo = framebuffer as GLuint;
    if last_cleared.insert(fbo, frame) == Some(frame) {
        return;
    }
    drop(last_cleared);
    let mut old_mask: GLboolean = 0;
    gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut old_mask);
    gl21::DepthMask(gl21::TRUE);
    gl21::Clear(gl21::DEPTH_BUFFER_BIT);
    gl21::DepthMask(old_mask);
    log_once!("TOUCHHLE_DS_CLEAR_OPENING_DEPTH: cleared opening depth once per FBO/frame");
}

/// Composite a selected foreground layer only into dark destination pixels:
/// `out = src * (1 - dst) + dst`. This fills stale-depth/background holes while
/// preserving already-lit station geometry instead of forcing the entire layer
/// to the front.
unsafe fn dst_fill_hash_count_begin(
    count: GLsizei,
) -> Option<(GLboolean, GLint, GLint, GLboolean, GLint, GLboolean, GLboolean)> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_DST_FILL_HASH_COUNT").ok()?;
    for item in spec.split(';') {
        let mut parts = item.splitn(2, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok());
        if key.is_some_and(|key| tex_selected(None, &[key])) && wanted_count == Some(count) {
            let was_blend = (gl21::IsEnabled(gl21::BLEND) != 0) as GLboolean;
            let was_alpha = (gl21::IsEnabled(gl21::ALPHA_TEST) != 0) as GLboolean;
            let was_test = (gl21::IsEnabled(gl21::DEPTH_TEST) != 0) as GLboolean;
            let mut old_src: GLint = 0;
            let mut old_dst: GLint = 0;
            let mut old_func: GLint = 0;
            let mut old_mask: GLboolean = 0;
            gl21::GetIntegerv(gl21::BLEND_SRC_RGB, &mut old_src);
            gl21::GetIntegerv(gl21::BLEND_DST_RGB, &mut old_dst);
            gl21::GetIntegerv(gl21::DEPTH_FUNC, &mut old_func);
            gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut old_mask);
            if was_alpha != 0 {
                gl21::Disable(gl21::ALPHA_TEST);
            }
            gl21::Enable(gl21::BLEND);
            gl21::BlendFunc(gl21::ONE_MINUS_DST_COLOR, gl21::ONE);
            gl21::Enable(gl21::DEPTH_TEST);
            gl21::DepthFunc(gl21::ALWAYS);
            gl21::DepthMask(gl21::FALSE);
            log_once!("TOUCHHLE_DS_DST_FILL_HASH_COUNT: filling selected layer into dark pixels");
            return Some((
                was_blend, old_src, old_dst, was_test, old_func, was_alpha, old_mask,
            ));
        }
    }
    None
}

unsafe fn dst_fill_hash_count_end(
    backup: Option<(GLboolean, GLint, GLint, GLboolean, GLint, GLboolean, GLboolean)>,
) {
    if let Some((was_blend, old_src, old_dst, was_test, old_func, was_alpha, old_mask)) = backup {
        gl21::DepthMask(old_mask);
        gl21::DepthFunc(old_func as GLenum);
        if was_test != 0 {
            gl21::Enable(gl21::DEPTH_TEST);
        } else {
            gl21::Disable(gl21::DEPTH_TEST);
        }
        if was_blend != 0 {
            gl21::Enable(gl21::BLEND);
        } else {
            gl21::Disable(gl21::BLEND);
        }
        gl21::BlendFunc(old_src as GLenum, old_dst as GLenum);
        if was_alpha != 0 {
            gl21::Enable(gl21::ALPHA_TEST);
        }
    }
}

/// Dead Space debug aid: `TOUCHHLE_DS_TEXWRAP=<gl texture id>[:mode]` overrides
/// the wrap mode of that one texture for each draw. `mode` is `edge` (default),
/// `repeat`, `mirror` or `clamp`. The game re-sets wrap itself every frame, so
/// the override has to be applied immediately before the draw call and undone
/// afterwards; the `[DSTEXt]` dump therefore still reports the game's own value.
unsafe fn texwrap_begin() -> Option<(GLint, GLint)> {
    // Legacy form selects by GL name (`<id>[:mode]`); the `_HASH` form selects
    // by upload content (`<key[,key...]>[:mode]`), which stays valid for the
    // whole run because it does not depend on the recycled GL name.
    let legacy = crate::ds_draw_env::var("TOUCHHLE_DS_TEXWRAP").ok();
    let hashed = crate::ds_draw_env::var("TOUCHHLE_DS_TEXWRAP_HASH").ok();
    if legacy.is_none() && hashed.is_none() {
        return None;
    }
    let mut id: Option<GLint> = None;
    let mut keys: Vec<u32> = Vec::new();
    let mut mode = String::from("edge");
    if let Some(spec) = &legacy {
        let mut parts = spec.splitn(2, ':');
        id = parts.next().and_then(|v| v.trim().parse::<GLint>().ok());
        if let Some(m) = parts.next() {
            mode = m.trim().to_ascii_lowercase();
        }
    }
    if let Some(spec) = &hashed {
        let mut parts = spec.splitn(2, ':');
        keys = crate::ds_texid::parse_keys(parts.next().unwrap_or(""));
        if let Some(m) = parts.next() {
            mode = m.trim().to_ascii_lowercase();
        }
    }
    if id.is_none() && keys.is_empty() {
        return None;
    }
    let m = match mode.as_str() {
        "repeat" => gl21::REPEAT as GLint,
        "mirror" => gl21::MIRRORED_REPEAT as GLint,
        "clamp" => 0x2900u32 as GLint, // desktop GL_CLAMP, for A/B comparison
        _ => gl21::CLAMP_TO_EDGE as GLint,
    };
    if !tex_selected(id, &keys) {
        return None;
    }
    let mut old_s: GLint = 0;
    let mut old_t: GLint = 0;
    gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_WRAP_S, &mut old_s);
    gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_WRAP_T, &mut old_t);
    gl21::TexParameteri(gl21::TEXTURE_2D, gl21::TEXTURE_WRAP_S, m);
    gl21::TexParameteri(gl21::TEXTURE_2D, gl21::TEXTURE_WRAP_T, m);
    let _ = mode;
    log_once!("TOUCHHLE_DS_TEXWRAP: overriding wrap mode of the selected texture");
    Some((old_s, old_t))
}

unsafe fn texwrap_end(old: Option<(GLint, GLint)>) {
    if let Some((s, t)) = old {
        gl21::TexParameteri(gl21::TEXTURE_2D, gl21::TEXTURE_WRAP_S, s);
        gl21::TexParameteri(gl21::TEXTURE_2D, gl21::TEXTURE_WRAP_T, t);
    }
}

#[derive(Copy, Clone)]
struct DepthLequalBackup {
    was_test: GLboolean,
    was_mask: GLboolean,
    old_func: GLint,
    old_polygon_offset: GLboolean,
    old_offset_factor: GLfloat,
    old_offset_units: GLfloat,
}

/// Restrict the coplanar tie-break to the far-plane background band where the
/// original space stack lives. `TOUCHHLE_DS_DEPTH_LEQUAL_Z="min,max"` overrides
/// the default NDC-z range for diagnostics.
unsafe fn depth_lequal_z_allowed(count: GLsizei) -> bool {
    let (default_min, default_max) = (0.9f32, 1.1f32);
    let (min_z, max_z) = match crate::ds_draw_env::var("TOUCHHLE_DS_DEPTH_LEQUAL_Z") {
        Ok(spec) => {
            let values: Vec<f32> = spec
                .split(',')
                .filter_map(|part| part.trim().parse::<f32>().ok())
                .collect();
            if values.len() == 2 && values[0] <= values[1] {
                (values[0], values[1])
            } else {
                (default_min, default_max)
            }
        }
        Err(_) => (default_min, default_max),
    };
    match ds_draw_z_ndc_range(count) {
        Some((zmin, zmax)) if zmin.is_finite() && zmax.is_finite() => {
            zmin >= min_z && zmax <= max_z
        }
        _ => false,
    }
}

const DEFAULT_DEPTH_LEQUAL_KEYS: &[u32] = &[
    0x25a0_8a82,
    0xcee4_3546,
    0x3649_7762,
    0x1d5c_ccb8,
    0x8c12_a704,
    0x8712_7d0f,
    0xe70e_fc97,
];

/// Dead Space debug aid: `TOUCHHLE_DS_TEXNODEPTH=<gl texture id>` /
/// `TOUCHHLE_DS_TEXNODEPTH_HASH=<key[,key...]>` draws that one texture with
/// depth testing and depth writing turned off, restoring both
/// afterwards. Use it to tell whether a layer flickers because it is fighting
/// other geometry at almost the same depth (z-fighting) rather than because of
/// filtering or wrap.
unsafe fn texnodepth_begin(count: GLsizei) -> Option<DepthLequalBackup> {
    // The space background needs `GL_LEQUAL` so its later coplanar layers can
    // replace earlier black ones. Applying that to the whole scene also lets
    // docking-bay surfaces overwrite correct geometry, producing moving
    // black/bright bands. By default, scope the tie-break to the far-plane
    // background band only. `TOUCHHLE_DS_DEPTH_LEQUAL=1` restores the legacy
    // whole-scene experiment and `=0` disables the repair entirely.
    //
    // Why: every layer of the space background stack is a
    // coplanar quad whose clip-space z is *exactly* 1.003 (measured 2026-10-09
    // from the terse trace: all of tex 45/48/54/58/59/60/61/62 report
    // z=[1.003,1.003]). After the z-row fix they all land on the same depth, so
    // with `GL_LESS` the *first* drawn layer owns the pixel and the rest are
    // rejected - which is what painted the giant black disc over the lit planet
    // (the disc has exactly the planet's silhouette). With `GL_LEQUAL` the
    // *last* submitted layer wins, which reproduces the reference image.
    //
    let lequal_mode = crate::ds_draw_env::var("TOUCHHLE_DS_DEPTH_LEQUAL").ok();
    let global_lequal = lequal_mode.as_deref() == Some("1");
    let hash_spec = crate::ds_draw_env::var("TOUCHHLE_DS_DEPTH_LEQUAL_HASH").ok();
    let z_spec = crate::ds_draw_env::contains("TOUCHHLE_DS_DEPTH_LEQUAL_Z");
    let legacy_requested = lequal_mode.is_some() || hash_spec.is_some() || z_spec;
    let depth_repair_enabled = lequal_mode.as_deref() != Some("0") && legacy_requested;
    let min_stack_span = crate::ds_draw_env::var("TOUCHHLE_DS_DEPTH_LEQUAL_MIN_SPAN")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| *value > 0.0)
        .unwrap_or(1.5);
    let screen_sized_draw = ds_draw_ndc_span(count)
        .map(|(width, height)| width >= min_stack_span && height >= min_stack_span)
        .unwrap_or(false);
    if crate::ds_draw_env::contains("TOUCHHLE_DS_DEPTH_TRACE")
        && tex_selected(None, DEFAULT_DEPTH_LEQUAL_KEYS)
    {
        let key = bound_texture_key().unwrap_or(0);
        let span = ds_draw_ndc_span(count);
        let zrange = ds_draw_z_ndc_range(count);
        log!(
            "[DSdepth] f={} key={:08x} count={} span={:?} z={:?}",
            crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),
            key,
            count,
            span,
            zrange
        );
    }
    // The asteroid is a depth-tested mesh, not a coplanar background layer.
    // Disabling its depth writes makes later triangles overwrite its front
    // surface (rectangular holes and apparently displaced foreground rocks).
    // Keep old launcher hash lists from reintroducing that regression.
    let scene_rock_batch = count == 600 && tex_selected(None, &[0xd1fe_b687]);
    let scoped_repair = !scene_rock_batch && match hash_spec.as_deref() {
        Some(spec) => screen_sized_draw && tex_hash_in_spec(spec),
        None if z_spec => depth_lequal_z_allowed(count),
        None => screen_sized_draw && tex_selected(None, DEFAULT_DEPTH_LEQUAL_KEYS),
    };
    if depth_repair_enabled
        && (global_lequal || scoped_repair)
        && gl21::IsEnabled(gl21::DEPTH_TEST) != 0
    {
        let mut func: GLint = 0;
        gl21::GetIntegerv(gl21::DEPTH_FUNC, &mut func);
        if func == gl21::LESS as GLint {
            let mut was_mask: GLboolean = 0;
            gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut was_mask);
            let old_polygon_offset = (gl21::IsEnabled(gl21::POLYGON_OFFSET_FILL) != 0) as GLboolean;
            let mut old_offset_factor: GLfloat = 0.0;
            let mut old_offset_units: GLfloat = 0.0;
            gl21::GetFloatv(gl21::POLYGON_OFFSET_FACTOR, &mut old_offset_factor);
            gl21::GetFloatv(gl21::POLYGON_OFFSET_UNITS, &mut old_offset_units);
            gl21::DepthFunc(gl21::LEQUAL);
            if !global_lequal {
                if crate::ds_draw_env::contains("TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE") {
                    gl21::DepthMask(0);
                } else {
                    let factor = crate::ds_draw_env::var("TOUCHHLE_DS_DEPTH_LEQUAL_OFFSET_FACTOR")
                        .ok()
                        .and_then(|value| value.parse::<GLfloat>().ok())
                        .unwrap_or(1.0);
                    let units = crate::ds_draw_env::var("TOUCHHLE_DS_DEPTH_LEQUAL_OFFSET_UNITS")
                        .ok()
                        .and_then(|value| value.parse::<GLfloat>().ok())
                        .unwrap_or(1.0);
                    gl21::Enable(gl21::POLYGON_OFFSET_FILL);
                    gl21::PolygonOffset(factor, units);
                }
            }
            log_once!("TOUCHHLE_DS_DEPTH_LEQUAL: GL_LESS -> GL_LEQUAL for tied depths");
            return Some(DepthLequalBackup {
                was_test: 1,
                was_mask,
                old_func: func,
                old_polygon_offset,
                old_offset_factor,
                old_offset_units,
            });
        }
    }
    // Global variant, for attributing whole-frame occlusion problems (B2):
    // `TOUCHHLE_DS_NODEPTH_ALL=1` draws *everything* with depth test and depth
    // write off. If a wrongly-black region disappears with this on, the cause
    // is depth ordering/occlusion rather than a missing or mis-sampled texture;
    // if it survives, the black is drawn last regardless of depth (or is the
    // clear colour showing through).
    if crate::ds_draw_env::var("TOUCHHLE_DS_NODEPTH_ALL").map_or(false, |v| v == "1") {
        let was_test = gl21::IsEnabled(gl21::DEPTH_TEST) != 0;
        let mut was_mask: GLboolean = 0;
        gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut was_mask);
        gl21::Disable(gl21::DEPTH_TEST);
        gl21::DepthMask(0);
        log_once!("TOUCHHLE_DS_NODEPTH_ALL: drawing everything without depth test/write");
        return Some(DepthLequalBackup {
            was_test: was_test as GLboolean,
            was_mask,
            old_func: -1,
            old_polygon_offset: 0,
            old_offset_factor: 0.0,
            old_offset_units: 0.0,
        });
    }
    let count_spec = crate::ds_draw_env::var("TOUCHHLE_DS_TEXNODEPTH_HASH_COUNT").ok();
    if let Some(spec) = count_spec {
        let opening_projection = opening_projection_active();
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
                // The six-index opening cover quads are opaque layers; skipping
                // their depth test exposes them as a black sheet. The 36-index
                // wall batches do not have that regression and remain eligible.
                if opening_projection && wanted_count == 6 {
                    continue;
                }
                if wanted_count == count && tex_selected(None, &[key]) {
                    let blend_enabled = gl21::IsEnabled(gl21::BLEND) != 0;
                    let mut blend_src: GLint = 0;
                    let mut blend_dst: GLint = 0;
                    gl21::GetIntegerv(gl21::BLEND_SRC, &mut blend_src);
                    gl21::GetIntegerv(gl21::BLEND_DST, &mut blend_dst);
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
        if !matched {
            return None;
        }
        let was_test = gl21::IsEnabled(gl21::DEPTH_TEST) != 0;
        let mut was_mask: GLboolean = 0;
        gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut was_mask);
        gl21::Disable(gl21::DEPTH_TEST);
        gl21::DepthMask(0);
        log_once!(
            "TOUCHHLE_DS_TEXNODEPTH_HASH_COUNT: drawing selected hash/count batches without depth"
        );
        return Some(DepthLequalBackup {
            was_test: was_test as GLboolean,
            was_mask,
            old_func: -1,
            old_polygon_offset: 0,
            old_offset_factor: 0.0,
            old_offset_units: 0.0,
        });
    }
    let id: Option<GLint> = crate::ds_draw_env::var("TOUCHHLE_DS_TEXNODEPTH")
        .ok()
        .and_then(|v| v.parse().ok());
    let keys = crate::ds_draw_env::var("TOUCHHLE_DS_TEXNODEPTH_HASH")
        .map(|v| crate::ds_texid::parse_keys(&v))
        .unwrap_or_default();
    if id.is_none() && keys.is_empty() {
        return None;
    }
    if !tex_selected(id, &keys) {
        return None;
    }
    let was_test = gl21::IsEnabled(gl21::DEPTH_TEST) != 0;
    let mut was_mask: GLboolean = 0;
    gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut was_mask);
    gl21::Disable(gl21::DEPTH_TEST);
    gl21::DepthMask(0);
    log_once!("TOUCHHLE_DS_TEXNODEPTH: drawing the selected texture without depth test/write");
    Some(DepthLequalBackup {
        was_test: was_test as GLboolean,
        was_mask,
        old_func: -1,
        old_polygon_offset: 0,
        old_offset_factor: 0.0,
        old_offset_units: 0.0,
    })
}

unsafe fn texnodepth_end(old: Option<DepthLequalBackup>) {
    if let Some(state) = old {
        if state.old_func >= 0 {
            // Only the LEQUAL experiment changes the func; put it back.
            gl21::DepthFunc(state.old_func as GLenum);
            gl21::DepthMask(state.was_mask);
            gl21::PolygonOffset(state.old_offset_factor, state.old_offset_units);
            if state.old_polygon_offset != 0 {
                gl21::Enable(gl21::POLYGON_OFFSET_FILL);
            } else {
                gl21::Disable(gl21::POLYGON_OFFSET_FILL);
            }
        } else {
            if state.was_test != 0 {
                gl21::Enable(gl21::DEPTH_TEST);
            } else {
                gl21::Disable(gl21::DEPTH_TEST);
            }
            gl21::DepthMask(state.was_mask);
        }
    }
}

unsafe fn blood_depth_begin(mode: GLenum, count: GLsizei) -> bool {
    if mode != gl21::TRIANGLES
        || count <= 0
        || count % 6 != 0
        || crate::ds_draw_env::var("TOUCHHLE_DS_BLOOD_DEPTH_FIX").as_deref() == Ok("0")
        || !tex_selected(None, &[0x00b9_b876, 0xfd93_7e94, 0xb3ca_1a51])
        || gl21::IsEnabled(gl21::TEXTURE_2D) == 0
        || gl21::IsEnabled(gl21::BLEND) == 0
        || gl21::IsEnabled(gl21::DEPTH_TEST) != 0
    {
        return false;
    }
    let mut projection = [0.0f32; 16];
    gl21::GetFloatv(gl21::PROJECTION_MATRIX, projection.as_mut_ptr());
    if (projection[11] + 1.0).abs() > 0.001 || projection[15].abs() > 0.001 {
        return false;
    }
    let mut source: GLint = 0;
    let mut destination: GLint = 0;
    let mut depth_function: GLint = 0;
    let mut depth_write: GLboolean = 0;
    gl21::GetIntegerv(gl21::BLEND_SRC, &mut source);
    gl21::GetIntegerv(gl21::BLEND_DST, &mut destination);
    gl21::GetIntegerv(gl21::DEPTH_FUNC, &mut depth_function);
    gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut depth_write);
    if source != gl21::SRC_ALPHA as GLint
        || destination != gl21::ONE_MINUS_SRC_ALPHA as GLint
        || depth_function != gl21::LESS as GLint
        || depth_write != 0
    {
        return false;
    }
    gl21::Enable(gl21::DEPTH_TEST);
    log_once!("[DSbloodDepth] enabled scene-depth occlusion for world-space blood particles; size unchanged");
    true
}

unsafe fn blood_depth_end(active: bool) {
    if active {
        gl21::Disable(gl21::DEPTH_TEST);
    }
}

/// Split an indexed triangle batch when it mixes a full-atlas space layer with
/// sub-rectangle scene geometry. Full-atlas triangles keep `GL_LEQUAL` so the
/// background stack can replace an earlier black layer; sub-rectangle triangles
/// keep the guest's `GL_LESS`, preventing the same background atlas from
/// overwriting the docking-bay surfaces.
unsafe fn split_triangle_depth_draw_elements(
    mode: GLenum,
    count: GLsizei,
    type_: GLenum,
    indices: *const GLvoid,
) -> bool {
    let trace = crate::ds_draw_env::contains("TOUCHHLE_DS_TRI_DEPTH_TRACE");
    let trace_skip = |bit: u32, reason: &str| {
        if trace {
            use std::sync::atomic::{AtomicU32, Ordering};
            static SEEN: AtomicU32 = AtomicU32::new(0);
            let flag = 1u32 << bit;
            if SEEN.fetch_or(flag, Ordering::Relaxed) & flag == 0 {
                log!(
                    "[DStri] skip {}: mode=0x{:x} count={} type=0x{:x}",
                    reason,
                    mode,
                    count,
                    type_
                );
            }
        }
    };
    if crate::ds_draw_env::var("TOUCHHLE_DS_TRI_DEPTH_SPLIT").as_deref() != Ok("1")
        || mode != gl21::TRIANGLES
        || count < 3
    {
        trace_skip(0, "disabled/mode/count");
        return false;
    }
    if !tex_selected(None, DEFAULT_DEPTH_LEQUAL_KEYS) {
        trace_skip(1, "texture");
        return false;
    }
    let elem = match type_ {
        gl21::UNSIGNED_BYTE => 1usize,
        gl21::UNSIGNED_SHORT => 2,
        _ => {
            trace_skip(2, "index type");
            return false;
        }
    };

    let mut tc_size: GLint = 0;
    let mut tc_type: GLint = 0;
    let mut tc_stride: GLint = 0;
    let mut tc_buffer: GLint = 0;
    gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_SIZE, &mut tc_size);
    gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_TYPE, &mut tc_type);
    gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_STRIDE, &mut tc_stride);
    gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_BUFFER_BINDING, &mut tc_buffer);
    let mut tc_ptr: *mut GLvoid = std::ptr::null_mut();
    #[allow(clippy::unnecessary_mut_passed)]
    gl21::GetPointerv(gl21::TEXTURE_COORD_ARRAY_POINTER, &mut tc_ptr);
    if tc_size < 2 || tc_buffer != 0 || tc_ptr.is_null() {
        let reason = format!(
            "texture coords size={} type=0x{:x} stride={} buffer={} pointer={:?}",
            tc_size, tc_type, tc_stride, tc_buffer, tc_ptr
        );
        trace_skip(3, &reason);
        return false;
    }

    let mut idx_raw = Vec::<u8>::new();
    let mut idx_off = 0usize;
    let mut eab: GLint = 0;
    gl21::GetIntegerv(gl21::ELEMENT_ARRAY_BUFFER_BINDING, &mut eab);
    if eab != 0 {
        idx_off = indices as usize;
        let want = idx_off + count as usize * elem;
        if want <= idx_off || want > (16 << 20) {
            return false;
        }
        idx_raw.resize(want, 0);
        gl21::GetBufferSubData(
            gl21::ELEMENT_ARRAY_BUFFER,
            0,
            want as GLsizeiptr,
            idx_raw.as_mut_ptr() as *mut GLvoid,
        );
    } else if indices.is_null() {
        return false;
    }

    let tc_elem = match tc_type as GLenum {
        gl21::BYTE | gl21::UNSIGNED_BYTE => 1usize,
        gl21::SHORT | gl21::UNSIGNED_SHORT => 2,
        _ => 4,
    };
    let tc_n = tc_size.clamp(2, 4) as usize;
    let tc_stride = if tc_stride == 0 {
        tc_n * tc_elem
    } else {
        tc_stride as usize
    };
    let read_tc = |index: usize| -> Option<(f32, f32)> {
        let base = (tc_ptr as *const u8).add(index * tc_stride);
        let read = |component: usize| -> f32 {
            let ptr = base.add(component * tc_elem);
            match tc_type as GLenum {
                gl21::FLOAT => (ptr as *const f32).read_unaligned(),
                gles11::FIXED => (ptr as *const i32).read_unaligned() as f32 / 65536.0,
                gl21::UNSIGNED_BYTE => (ptr as *const u8).read_unaligned() as f32,
                gl21::BYTE => (ptr as *const i8).read_unaligned() as f32,
                gl21::UNSIGNED_SHORT => (ptr as *const u16).read_unaligned() as f32,
                gl21::SHORT => (ptr as *const i16).read_unaligned() as f32,
                _ => return 0.0,
            }
        };
        Some((read(0), read(1)))
    };

    let mut old_func: GLint = 0;
    gl21::GetIntegerv(gl21::DEPTH_FUNC, &mut old_func);
    let mut old_mask: GLboolean = 0;
    gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut old_mask);
    let old_polygon_offset = (gl21::IsEnabled(gl21::POLYGON_OFFSET_FILL) != 0) as GLboolean;
    let mut old_offset_factor: GLfloat = 0.0;
    let mut old_offset_units: GLfloat = 0.0;
    gl21::GetFloatv(gl21::POLYGON_OFFSET_FACTOR, &mut old_offset_factor);
    gl21::GetFloatv(gl21::POLYGON_OFFSET_UNITS, &mut old_offset_units);
    let forward_units = crate::ds_draw_env::var("TOUCHHLE_DS_TRI_DEPTH_SPLIT_FORWARD_UNITS")
        .ok()
        .and_then(|value| value.parse::<GLfloat>().ok())
        .unwrap_or(-1.0);
    let min_uv_span = crate::ds_draw_env::var("TOUCHHLE_DS_TRI_DEPTH_SPLIT_UV_SPAN")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(1.5);
    let scene_original_depth =
        crate::ds_draw_env::var("TOUCHHLE_DS_TRI_DEPTH_SPLIT_SCENE_FUNC").as_deref() == Ok("original");
    let triangles = count as usize / 3;
    let mut full_atlas_count = 0usize;
    for tri in 0..triangles {
        let mut idx = [0usize; 3];
        for corner in 0..3 {
            let offset = tri * 3 + corner;
            idx[corner] = match type_ {
                gl21::UNSIGNED_BYTE => {
                    if idx_raw.is_empty() {
                        *(indices as *const u8).add(offset) as usize
                    } else {
                        idx_raw[idx_off + offset] as usize
                    }
                }
                gl21::UNSIGNED_SHORT => {
                    if idx_raw.is_empty() {
                        *(indices as *const u16).add(offset) as usize
                    } else {
                        u16::from_le_bytes([
                            idx_raw[idx_off + offset * 2],
                            idx_raw[idx_off + offset * 2 + 1],
                        ]) as usize
                    }
                }
                _ => return false,
            };
        }
        let Some((u0, v0)) = read_tc(idx[0]) else {
            return false;
        };
        let Some((u1, v1)) = read_tc(idx[1]) else {
            return false;
        };
        let Some((u2, v2)) = read_tc(idx[2]) else {
            return false;
        };
        let u_span = u0.max(u1).max(u2) - u0.min(u1).min(u2);
        let v_span = v0.max(v1).max(v2) - v0.min(v1).min(v2);
        let full_atlas = u_span >= min_uv_span || v_span >= min_uv_span;
        if full_atlas {
            full_atlas_count += 1;
            gl21::DepthFunc(gl21::LEQUAL);
            gl21::DepthMask(0);
            gl21::Disable(gl21::POLYGON_OFFSET_FILL);
        } else if scene_original_depth {
            // The background triangles do not write depth, so scene geometry
            // still covers them. Preserving the guest function here prevents
            // two coplanar scene triangles from becoming last-writer-wins.
            gl21::DepthFunc(old_func as GLenum);
            gl21::DepthMask(old_mask);
            gl21::PolygonOffset(old_offset_factor, old_offset_units);
            if old_polygon_offset != 0 {
                gl21::Enable(gl21::POLYGON_OFFSET_FILL);
            } else {
                gl21::Disable(gl21::POLYGON_OFFSET_FILL);
            }
        } else {
            gl21::DepthFunc(gl21::LEQUAL);
            gl21::DepthMask(old_mask);
            gl21::Enable(gl21::POLYGON_OFFSET_FILL);
            gl21::PolygonOffset(0.0, forward_units);
        }
        let draw_indices = (indices as *const u8).add(tri * 3 * elem) as *const GLvoid;
        gl21::DrawElements(gl21::TRIANGLES, 3, type_, draw_indices);
    }
    gl21::DepthFunc(old_func as GLenum);
    gl21::DepthMask(old_mask);
    gl21::PolygonOffset(old_offset_factor, old_offset_units);
    if old_polygon_offset != 0 {
        gl21::Enable(gl21::POLYGON_OFFSET_FILL);
    } else {
        gl21::Disable(gl21::POLYGON_OFFSET_FILL);
    }
    if crate::ds_draw_env::contains("TOUCHHLE_DS_TRI_DEPTH_TRACE") {
        static TRACE_SUCCESS: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !TRACE_SUCCESS.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log!(
                "[DStri] split active: count={} triangles={} full_atlas={} scene={} uv_span={} scene_func={} forward_units={}",
                count,
                triangles,
                full_atlas_count,
                triangles.saturating_sub(full_atlas_count),
                min_uv_span,
                if scene_original_depth {
                    "original"
                } else {
                    "lequal"
                },
                forward_units
            );
        }
    }
    true
}

/// Dead Space debug aid: `TOUCHHLE_DS_REDTEX=<gl texture id>` draws that one
/// texture's draws as opaque red instead of texturing them. If a suspected
/// layer is actually reaching the framebuffer, its area lights up red.
const REDTEX_PALETTE: [(f32, f32, f32); 8] = [
    (1.0, 0.0, 0.0),
    (0.0, 1.0, 0.0),
    (0.0, 0.0, 1.0),
    (1.0, 1.0, 0.0),
    (1.0, 0.0, 1.0),
    (0.0, 1.0, 1.0),
    (1.0, 0.5, 0.0),
    (0.5, 0.5, 0.5),
];

unsafe fn redtex_begin(count: GLsizei) -> Option<[f32; 4]> {
    let mut cur: GLint = 0;
    gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut cur);
    let mut col: Option<(f32, f32, f32)> = None;
    // Preferred form: a list of upload *content keys* (hex). GL names are
    // recycled - one name can hold unrelated pictures at different times - so
    // the name is only trustworthy for the instant it holds the wanted
    // picture, while a content key stays valid for the whole run.
    if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_REDTEX_HASH") {
        let keys = crate::ds_texid::parse_keys(&spec);
        if let Some(ident) = crate::ds_texid::lookup(cur as u32) {
            if let Some(pos) = keys.iter().position(|key| *key == ident.key) {
                col = Some(REDTEX_PALETTE[pos % REDTEX_PALETTE.len()]);
                use std::sync::atomic::{AtomicUsize, Ordering};
                static LOGGED: AtomicUsize = AtomicUsize::new(0);
                if LOGGED.fetch_add(1, Ordering::Relaxed) < 20 {
                    log!(
                        "TOUCHHLE_DS_REDTEX_HASH: painting content key {:08x} (tex {})",
                        ident.key,
                        cur
                    );
                }
            }
        }
    }
    if col.is_none() {
        if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_REDTEX_HASH_COUNT") {
            for (pos, item) in spec.split(',').enumerate() {
                let mut parts = item.split(':');
                let key = parts
                    .next()
                    .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
                let wanted_count = parts
                    .next()
                    .and_then(|value| value.trim().parse::<GLsizei>().ok());
                if let (Some(key), Some(wanted_count)) = (key, wanted_count) {
                    if wanted_count == count && tex_selected(None, &[key]) {
                        col = Some(REDTEX_PALETTE[pos % REDTEX_PALETTE.len()]);
                        log_once!(
                            "TOUCHHLE_DS_REDTEX_HASH_COUNT: painting listed hash/count batches"
                        );
                        break;
                    }
                }
            }
        }
    }
    // Legacy form: comma separated list of GL texture names; the n-th entry
    // gets the n-th palette colour, so a single capture tells several
    // candidate layers apart.
    if col.is_none() {
        if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_REDTEX") {
            let mut idx: usize = 0;
            for part in spec.split(',') {
                if let Ok(id) = part.trim().parse::<GLint>() {
                    if id == cur {
                        col = Some(REDTEX_PALETTE[idx % REDTEX_PALETTE.len()]);
                        break;
                    }
                }
                idx += 1;
            }
        }
    }
    let col = match col {
        Some(c) => c,
        None => return None,
    };
    let mut old = [1.0f32; 4];
    gl21::GetFloatv(gl21::CURRENT_COLOR, old.as_mut_ptr());
    gl21::Disable(gl21::TEXTURE_2D);
    gl21::Color4f(col.0, col.1, col.2, 1.0);
    log_once!("TOUCHHLE_DS_REDTEX: painting listed texture draws with palette colours");
    Some(old)
}

unsafe fn redtex_end(old: Option<[f32; 4]>) {
    if let Some(c) = old {
        gl21::Enable(gl21::TEXTURE_2D);
        gl21::Color4f(c[0], c[1], c[2], c[3]);
    }
}

/// Dead Space debug aid: `TOUCHHLE_DS_TEXID_COLOR=1` paints every textured draw
/// with a colour derived from the bound texture id, so a single capture shows
/// which texture is responsible for each screen region. Red channel encodes the
/// id: red = (id * 97) mod 251, so the id can be recovered from a screenshot.
unsafe fn texid_color_begin() -> Option<[f32; 4]> {
    if !crate::ds_draw_env::contains("TOUCHHLE_DS_TEXID_COLOR") {
        return None;
    }
    let mut cur: GLint = 0;
    gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut cur);
    if cur == 0 {
        return None;
    }
    let mut old = [1.0f32; 4];
    gl21::GetFloatv(gl21::CURRENT_COLOR, old.as_mut_ptr());
    let id = cur as u32;
    let r = (id.wrapping_mul(97) % 251) as f32 / 251.0;
    let g = ((id.wrapping_mul(53) + 37) % 251) as f32 / 251.0;
    let b = ((id.wrapping_mul(29) + 101) % 251) as f32 / 251.0;
    // Two variants: "flat" replaces the texture with a solid id colour, while
    // "tint" keeps the texture and only hue-shifts it. Both preserve the
    // original alpha so blending behaviour is unchanged.
    if !crate::ds_draw_env::contains("TOUCHHLE_DS_TEXID_TINT") {
        gl21::Disable(gl21::TEXTURE_2D);
    }
    gl21::Color4f(r, g, b, old[3]);
    log_once!("TOUCHHLE_DS_TEXID_COLOR: painting texture ids as colours");
    Some(old)
}

unsafe fn texid_color_end(old: Option<[f32; 4]>) {
    if let Some(c) = old {
        gl21::Enable(gl21::TEXTURE_2D);
        gl21::Color4f(c[0], c[1], c[2], c[3]);
    }
}

/// Dead Space debug aid: `TOUCHHLE_DS_FOG_RED=1` replaces the fog colour with
/// bright red around each draw, so a black region can be attributed to the fog
/// colour itself rather than to something else that fog-disable happens to fix.
unsafe fn fog_red_begin() -> Option<[f32; 4]> {
    if !crate::ds_draw_env::contains("TOUCHHLE_DS_FOG_RED") {
        return None;
    }
    let mut en: GLboolean = 0;
    gl21::GetBooleanv(gl21::FOG, &mut en);
    if en == 0 {
        return None;
    }
    let mut old = [0.0f32; 4];
    gl21::GetFloatv(gl21::FOG_COLOR, old.as_mut_ptr());
    gl21::Fogfv(gl21::FOG_COLOR, [1.0, 0.0, 0.0, 1.0].as_ptr());
    log_once!("TOUCHHLE_DS_FOG_RED: painting the fog colour red");
    Some(old)
}

unsafe fn fog_red_end(old: Option<[f32; 4]>) {
    if let Some(c) = old {
        gl21::Fogfv(gl21::FOG_COLOR, c.as_ptr());
    }
}

/// True when the current projection matrix is orthographic (bottom row
/// 0,0,0,1 rather than the perspective 0,0,-1,0).
unsafe fn projection_is_ortho() -> bool {
    let mut m = [0.0f32; 16];
    gl21::GetFloatv(gl21::PROJECTION_MATRIX, m.as_mut_ptr());
    m[3].abs() < 1e-6 && m[7].abs() < 1e-6 && m[11].abs() < 1e-6 && (m[15] - 1.0).abs() < 1e-6
}

/// Dead Space fix candidate: `TOUCHHLE_DS_NO_FOG_ORTHO=1` disables fog only for
/// orthographic draws. The far cutscene geometry that our port renders black is
/// drawn with an orthographic projection, where the fog coordinate (eye-space
/// distance) is meaningless for a 2D-style pass, so the app's black GL_LINEAR
/// fog saturates it. Keeping fog everywhere else preserves the atmosphere.
unsafe fn nofog_ortho_begin() -> bool {
    if !crate::ds_draw_env::var("TOUCHHLE_DS_NO_FOG_ORTHO")
        .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
    {
        return false;
    }
    if !projection_is_ortho() {
        return false;
    }
    let on = gl21::IsEnabled(gl21::FOG) != 0;
    if on {
        gl21::Disable(gl21::FOG);
        log_once!("TOUCHHLE_DS_NO_FOG_ORTHO: disabling fog for orthographic draws");
    }
    on
}

/// Dead Space debug aid: `TOUCHHLE_DS_FOG_END_SCALE=<k>` multiplies GL_FOG_END
/// around each draw, so a scene whose distant geometry is over-fogged can keep
/// its atmosphere while still revealing that geometry.
unsafe fn fog_scale_begin() -> Option<f32> {
    let k: f32 = match crate::ds_draw_env::var("TOUCHHLE_DS_FOG_END_SCALE")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        Some(v) => v,
        None => return None,
    };
    let mut en: GLboolean = 0;
    gl21::GetBooleanv(gl21::FOG, &mut en);
    if en == 0 {
        return None;
    }
    let mut end = 0.0f32;
    gl21::GetFloatv(gl21::FOG_END, &mut end);
    gl21::Fogf(gl21::FOG_END, end * k);
    log_once!("TOUCHHLE_DS_FOG_END_SCALE: extending the fog far distance");
    Some(end)
}

unsafe fn fog_scale_end(old: Option<f32>) {
    if let Some(end) = old {
        gl21::Fogf(gl21::FOG_END, end);
    }
}

/// Dead Space debug aid: `TOUCHHLE_DS_LOG_FOG=1` logs the fog state at draw time
/// whenever it changes, so the parameters the app actually uses are visible.
unsafe fn log_fog_state() {
    if !crate::ds_draw_env::contains("TOUCHHLE_DS_LOG_FOG") {
        return;
    }
    use std::sync::{Mutex, OnceLock};
    static LAST: OnceLock<Mutex<String>> = OnceLock::new();
    let slot = LAST.get_or_init(|| Mutex::new(String::from("<none>")));
    let mut key = String::from("off");
    let mut en: GLboolean = 0;
    gl21::GetBooleanv(gl21::FOG, &mut en);
    if en != 0 {
        let mut mode: GLint = 0;
        gl21::GetIntegerv(gl21::FOG_MODE, &mut mode);
        let mut col = [0.0f32; 4];
        gl21::GetFloatv(gl21::FOG_COLOR, col.as_mut_ptr());
        let mut st = 0.0f32;
        gl21::GetFloatv(gl21::FOG_START, &mut st);
        let mut end = 0.0f32;
        gl21::GetFloatv(gl21::FOG_END, &mut end);
        let mut den = 0.0f32;
        gl21::GetFloatv(gl21::FOG_DENSITY, &mut den);
        key = format!(
            "on mode=0x{:x} col={:?} start={} end={} density={}",
            mode, col, st, end, den
        );
    }
    let mut last = slot.lock().unwrap();
    if *last != key {
        *last = key.clone();
        log!("[DSfog] {}", key);
    }
}

/// Dead Space debug aid: `TOUCHHLE_DS_NO_FOG=1` disables GL fog around each
/// draw. If a black region lights up, its blackness was fog (distant geometry
/// blended towards the fog colour).
///
/// `TOUCHHLE_DS_NO_FOG_BLEND=1` scopes that exclusion to blended draws. The
/// opening station's lit porch/window is a transparent decal over the shell;
/// eye-space fog darkens the decal to nearly black while opaque scene geometry
/// still needs the original distance fade.
// The opening camera abruptly shrinks 10..25 fog to near zero, then ramps
// back. Keep the established range only during this observed transition.
unsafe fn opening_fog_begin(state: &mut ds_opening_fog::OpeningFog) -> Option<(f32, f32)> {
    if crate::ds_draw_env::var("TOUCHHLE_DS_OPENING_FOG_FIX").as_deref() == Ok("0") {
        return None;
    }
    let enabled = gl21::IsEnabled(gl21::FOG) != 0;
    let mut mode = 0;
    let mut color = [0.0; 4];
    let mut projection = [0.0; 16];
    gl21::GetIntegerv(gl21::FOG_MODE, &mut mode);
    gl21::GetFloatv(gl21::FOG_COLOR, color.as_mut_ptr());
    gl21::GetFloatv(gl21::PROJECTION_MATRIX, projection.as_mut_ptr());
    let mut start = 0.0;
    let mut end = 0.0;
    gl21::GetFloatv(gl21::FOG_START, &mut start);
    gl21::GetFloatv(gl21::FOG_END, &mut end);
    if end > 0.0 && end < 0.25 && (start - end * 0.4).abs() < 0.002
        && (projection[11] + 1.0).abs() < 0.001 {
        static CHECKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !CHECKED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log!("[DSopeningFogCheck] enabled={} mode={} color={:?} near_coefficient={} range={}..{} state={:?}",
                enabled, mode, color, projection[14], start, end, state);
        }
    }
    if mode != gl21::LINEAR as GLint
        || color[..3].iter().any(|c| c.abs() > 0.001)
        || (projection[11] + 1.0).abs() > 0.001
        || projection[15].abs() > 0.001
        || !(-0.105..=-0.095).contains(&projection[14])
    { return None; }
    let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    let (fixed_start, fixed_end) = state.range(frame, start, end)?;
    if !enabled { return None; }
    gl21::Fogf(gl21::FOG_START, fixed_start);
    gl21::Fogf(gl21::FOG_END, fixed_end);
    log_once!("[DSopeningFog] preserving 10..25 during collapsed opening fog ramp");
    Some((start, end))
}

unsafe fn opening_fog_end(old: Option<(f32, f32)>) {
    if let Some((start, end)) = old {
        gl21::Fogf(gl21::FOG_START, start);
        gl21::Fogf(gl21::FOG_END, end);
    }
}

unsafe fn nofog_begin(count: GLsizei) -> bool {
    let global = crate::ds_draw_env::var("TOUCHHLE_DS_NO_FOG")
        .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
        .unwrap_or(false);
    let texture_match = crate::ds_draw_env::var("TOUCHHLE_DS_NO_FOG_HASH")
        .ok()
        .map(|spec| tex_selected(None, &crate::ds_texid::parse_keys(&spec)))
        .unwrap_or(false);
    let hash_count_match = crate::ds_draw_env::var("TOUCHHLE_DS_NO_FOG_HASH_COUNT")
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
                key.is_some_and(|key| tex_selected(None, &[key])) && wanted_count == Some(count)
            })
        });
    let screen_layer = crate::ds_draw_env::var("TOUCHHLE_DS_NO_FOG_SCREEN")
        .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
        && ds_draw_ndc_span(count)
            .map(|(width, height)| width >= 1.5 && height >= 1.5)
            .unwrap_or(false);
    let blend_layer = crate::ds_draw_env::var("TOUCHHLE_DS_NO_FOG_BLEND")
        .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
        && gl21::IsEnabled(gl21::BLEND) != 0;
    // Dead Space uses a degenerate linear fog interval for interior
    // cutscenes. Desktop GL clamps every visible distance to the fog colour,
    // while the original PowerVR path leaves the lit scene visible. Preserve
    // valid atmospheric ranges and only suppress this impossible interval.
    let degenerate_linear_fog = gl21::IsEnabled(gl21::FOG) != 0 && {
        let mut mode: GLint = 0;
        let mut start: GLfloat = 0.0;
        let mut end: GLfloat = 0.0;
        gl21::GetIntegerv(gl21::FOG_MODE, &mut mode);
        gl21::GetFloatv(gl21::FOG_START, &mut start);
        gl21::GetFloatv(gl21::FOG_END, &mut end);
        mode == gl21::LINEAR as GLint && (end <= 0.0 || end <= start)
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
    let on = gl21::IsEnabled(gl21::FOG) != 0;
    if on {
        gl21::Disable(gl21::FOG);
        log_once!("TOUCHHLE_DS_NO_FOG: disabling fog for draws");
    }
    on
}

unsafe fn nofog_end(on: bool) {
    if on {
        gl21::Enable(gl21::FOG);
    }
}

/// Dead Space debug aid: `TOUCHHLE_DS_SKIP_TEX=<id>[,<id>...]` /
/// `TOUCHHLE_DS_SKIP_TEX_HASH=<key>[,<key>...]` drops every draw whose bound
/// texture is one of those, so a layer responsible for a symptom can be found by
/// removing it. Logs once per run when it first suppresses a draw.
///
/// Both forms are independent: the content-key one must work on its own, which
/// is easy to get wrong with an early `return` on the legacy variable.
unsafe fn skip_tex_active(count: GLsizei) -> bool {
    if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_SKIP_TEX_HASH_COUNT") {
        for item in spec.split(',') {
            let mut parts = item.split(':');
            let key = parts
                .next()
                .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
            let wanted_count = parts
                .next()
                .and_then(|value| value.trim().parse::<GLsizei>().ok());
            if let (Some(key), Some(wanted_count)) = (key, wanted_count) {
                if wanted_count == count && tex_selected(None, &[key]) {
                    log_once!(
                        "TOUCHHLE_DS_SKIP_TEX_HASH_COUNT: suppressing listed hash/count batches"
                    );
                    return true;
                }
            }
        }
    }
    if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_SKIP_TEX_HASH") {
        if tex_hash_matches_spec(&spec) {
            return true;
        }
    }
    let spec = match crate::ds_draw_env::var("TOUCHHLE_DS_SKIP_TEX") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut cur: GLint = 0;
    gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut cur);
    for part in spec.split(',') {
        if let Ok(id) = part.trim().parse::<GLint>() {
            if id == cur {
                log_once!("TOUCHHLE_DS_SKIP_TEX: suppressing draws of listed textures");
                return true;
            }
        }
    }
    false
}

/// Debug aid for a batch that mixes valid scene geometry with one bad
/// triangle: `TOUCHHLE_DS_SKIP_TRI_HASH_COUNT="key:count:tri[,tri...]"`.
unsafe fn skip_triangles(count: GLsizei) -> Option<std::collections::HashSet<usize>> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_SKIP_TRI_HASH_COUNT").ok()?;
    for item in spec.split(';') {
        let mut parts = item.splitn(3, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok())?;
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok())?;
        if wanted_count != count || !tex_selected(None, &[key]) {
            continue;
        }
        let skipped: std::collections::HashSet<usize> = parts
            .next()?
            .split(',')
            .filter_map(|value| value.trim().parse::<usize>().ok())
            .collect();
        if !skipped.is_empty() {
            log_once!("TOUCHHLE_DS_SKIP_TRI_HASH_COUNT: suppressing listed triangles");
            return Some(skipped);
        }
    }
    None
}

/// Draw one indexed batch in contiguous runs, allowing only selected triangle
/// IDs to bypass depth ordering. This repairs triangles hidden by stale MSAA
/// depth without forcing the complete foreground rock pass in front of station
/// geometry.
unsafe fn draw_elements_with_depth_always_triangles(
    mode: GLenum,
    count: GLsizei,
    type_: GLenum,
    indices: *const GLvoid,
) -> bool {
    let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_DEPTH_ALWAYS_TRI_HASH_COUNT") else {
        return false;
    };
    if mode != gl21::TRIANGLES || count < 3 {
        return false;
    }
    let mut selected = None;
    for item in spec.split(';') {
        let mut parts = item.splitn(3, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok());
        if key.is_some_and(|key| tex_selected(None, &[key])) && wanted_count == Some(count) {
            selected = Some(
                parts
                    .next()
                    .unwrap_or("")
                    .split(',')
                    .filter_map(|value| value.trim().parse::<usize>().ok())
                    .collect::<std::collections::HashSet<_>>(),
            );
            break;
        }
    }
    let Some(selected) = selected.filter(|set| !set.is_empty()) else {
        return false;
    };
    let index_size = match type_ {
        gl21::UNSIGNED_BYTE => 1usize,
        gl21::UNSIGNED_SHORT => 2usize,
        _ => return false,
    };
    let triangles = count as usize / 3;
    if !selected.iter().any(|triangle| *triangle < triangles) {
        return false;
    }
    let mut old_func: GLint = 0;
    gl21::GetIntegerv(gl21::DEPTH_FUNC, &mut old_func);
    let mut triangle = 0usize;
    while triangle < triangles {
        let force_visible = selected.contains(&triangle);
        let run_start = triangle;
        while triangle < triangles && selected.contains(&triangle) == force_visible {
            triangle += 1;
        }
        gl21::DepthFunc(if force_visible {
            gl21::ALWAYS
        } else {
            old_func as GLenum
        });
        let run_indices = ((triangle - run_start) * 3) as GLsizei;
        let byte_offset = run_start * 3 * index_size;
        gl21::DrawElements(
            mode,
            run_indices,
            type_,
            indices.cast::<u8>().add(byte_offset).cast(),
        );
    }
    gl21::DepthFunc(old_func as GLenum);
    log_once!("TOUCHHLE_DS_DEPTH_ALWAYS_TRI_HASH_COUNT: repaired selected asteroid triangles");
    true
}

/// Disable face culling only for selected triangles in one indexed batch.
/// This is used when a few asteroid faces have reversed winding, while the
/// rest of the mesh still needs normal back-face rejection.
unsafe fn draw_elements_with_no_cull_triangles(
    mode: GLenum,
    count: GLsizei,
    type_: GLenum,
    indices: *const GLvoid,
) -> bool {
    let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_NO_CULL_TRI_HASH_COUNT") else {
        return false;
    };
    if mode != gl21::TRIANGLES || count < 3 {
        return false;
    }
    let mut selected = None;
    for item in spec.split(';') {
        let mut parts = item.splitn(3, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok());
        if key.is_some_and(|key| tex_selected(None, &[key])) && wanted_count == Some(count) {
            selected = Some(
                parts
                    .next()
                    .unwrap_or("")
                    .split(',')
                    .filter_map(|value| value.trim().parse::<usize>().ok())
                    .collect::<std::collections::HashSet<_>>(),
            );
            break;
        }
    }
    let Some(selected) = selected.filter(|set| !set.is_empty()) else {
        return false;
    };
    if gl21::IsEnabled(gl21::CULL_FACE) == 0 {
        return false;
    }
    let index_size = match type_ {
        gl21::UNSIGNED_BYTE => 1usize,
        gl21::UNSIGNED_SHORT => 2usize,
        _ => return false,
    };
    let triangles = count as usize / 3;
    if !selected.iter().any(|triangle| *triangle < triangles) {
        return false;
    }
    let mut triangle = 0usize;
    while triangle < triangles {
        let no_cull = selected.contains(&triangle);
        let run_start = triangle;
        while triangle < triangles && selected.contains(&triangle) == no_cull {
            triangle += 1;
        }
        if no_cull {
            gl21::Disable(gl21::CULL_FACE);
        }
        let run_indices = ((triangle - run_start) * 3) as GLsizei;
        let byte_offset = run_start * 3 * index_size;
        gl21::DrawElements(
            mode,
            run_indices,
            type_,
            indices.cast::<u8>().add(byte_offset).cast(),
        );
        if no_cull {
            gl21::Enable(gl21::CULL_FACE);
        }
    }
    log_once!("TOUCHHLE_DS_NO_CULL_TRI_HASH_COUNT: disabling culling for selected triangles");
    true
}

/// Draw a selected indexed mesh in two passes: normal front faces first, then
/// unlit back faces. Back faces are farther than existing front geometry, so
/// they only fill holes while avoiding the black result of lighting inverted
/// normals.
unsafe fn draw_elements_with_unlit_backfaces(
    mode: GLenum,
    count: GLsizei,
    type_: GLenum,
    indices: *const GLvoid,
) -> bool {
    let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_UNLIT_BACKFACE_HASH_COUNT") else {
        return false;
    };
    if mode != gl21::TRIANGLES || count < 3 {
        return false;
    }
    let matched = spec.split(';').any(|item| {
        let mut parts = item.splitn(2, ':');
        let key = parts
            .next()
            .and_then(|value| u32::from_str_radix(value.trim(), 16).ok());
        let wanted_count = parts
            .next()
            .and_then(|value| value.trim().parse::<GLsizei>().ok());
        key.is_some_and(|key| tex_selected(None, &[key])) && wanted_count == Some(count)
    });
    if !matched {
        return false;
    }
    let was_cull = (gl21::IsEnabled(gl21::CULL_FACE) != 0) as GLboolean;
    let was_lighting = (gl21::IsEnabled(gl21::LIGHTING) != 0) as GLboolean;
    let mut old_cull_mode: GLint = 0;
    gl21::GetIntegerv(gl21::CULL_FACE_MODE, &mut old_cull_mode);

    gl21::DrawElements(mode, count, type_, indices);

    gl21::Enable(gl21::CULL_FACE);
    gl21::CullFace(gl21::FRONT);
    if was_lighting != 0 {
        gl21::Disable(gl21::LIGHTING);
    }
    gl21::DrawElements(mode, count, type_, indices);

    if was_lighting != 0 {
        gl21::Enable(gl21::LIGHTING);
    }
    gl21::CullFace(old_cull_mode as GLenum);
    if was_cull == 0 {
        gl21::Disable(gl21::CULL_FACE);
    }
    log_once!("TOUCHHLE_DS_UNLIT_BACKFACE_HASH_COUNT: filled selected back faces unlit");
    true
}

unsafe fn draw_elements_skipping_triangles(
    mode: GLenum,
    count: GLsizei,
    type_: GLenum,
    indices: *const GLvoid,
) -> bool {
    if mode != gl21::TRIANGLES {
        return false;
    }
    let Some(skipped) = skip_triangles(count) else {
        return false;
    };
    let triangle_count = count.max(0) as usize / 3;
    if !skipped.iter().any(|triangle| *triangle < triangle_count) {
        return false;
    }
    let index_size = match type_ {
        gl21::UNSIGNED_BYTE => 1usize,
        gl21::UNSIGNED_SHORT => 2usize,
        _ => return false,
    };
    let mut triangle = 0usize;
    while triangle < triangle_count {
        while triangle < triangle_count && skipped.contains(&triangle) {
            triangle += 1;
        }
        let run_start = triangle;
        while triangle < triangle_count && !skipped.contains(&triangle) {
            triangle += 1;
        }
        if run_start < triangle {
            let run_indices = ((triangle - run_start) * 3) as GLsizei;
            let byte_offset = run_start * 3 * index_size;
            gl21::DrawElements(
                mode,
                run_indices,
                type_,
                indices.cast::<u8>().add(byte_offset).cast(),
            );
        }
    }
    true
}

/// Dead Space debug aid: `TOUCHHLE_DS_IGNORE_VERTEX_COLOR=1` disables the client
/// colour array for each draw, so a layer whose blackness comes from the vertex
/// colours (not from its texture) renders with the current colour instead. If a
/// suspected region lights up, its blackness came from vertex colours.
unsafe fn vcolor_off_begin() -> bool {
    if !crate::ds_draw_env::contains("TOUCHHLE_DS_IGNORE_VERTEX_COLOR") {
        return false;
    }
    let on = gl21::IsEnabled(gl21::COLOR_ARRAY) != 0;
    if on {
        gl21::DisableClientState(gl21::COLOR_ARRAY);
        log_once!("TOUCHHLE_DS_IGNORE_VERTEX_COLOR: dropping client colour arrays");
    }
    on
}

unsafe fn vcolor_off_end(on: bool) {
    if on {
        gl21::EnableClientState(gl21::COLOR_ARRAY);
    }
}

fn ds_texmat_trace_log<'a>(message: impl Into<trace_file::Message<'a>>) {
    let path = crate::ds_draw_env::var_os("TOUCHHLE_DS_TEXMAT_TRACE_FILE");
    let trigger = path.as_ref().and_then(|_| crate::ds_draw_env::var_os("TOUCHHLE_DS_TEXMAT_TRACE_TRIGGER"));
    trace_file::append_if_configured(path.as_deref().map(std::path::Path::new),
        trigger.as_deref().map(std::path::Path::new), message);
}

/// Debug aid: when `TOUCHHLE_DS_TEXMAT_IDENTITY` is set, force the texture
/// matrix to identity around a draw so we can A/B test whether the guest's
/// texture matrix is collapsing the sampled texel.
unsafe fn force_texmat_identity_begin() -> Option<GLint> {
    if !crate::ds_draw_env::contains("TOUCHHLE_DS_TEXMAT_IDENTITY") {
        return None;
    }
    let mut old_mode: GLint = 0;
    gl21::GetIntegerv(gl21::MATRIX_MODE, &mut old_mode);
    gl21::MatrixMode(gl21::TEXTURE);
    gl21::PushMatrix();
    gl21::LoadIdentity();
    gl21::MatrixMode(old_mode as GLenum);
    Some(old_mode)
}
unsafe fn force_texmat_identity_end(backup: Option<GLint>) {
    if let Some(old_mode) = backup {
        gl21::MatrixMode(gl21::TEXTURE);
        gl21::PopMatrix();
        gl21::MatrixMode(old_mode as GLenum);
    }
}

/// Apply identity texture coordinates only to a selected content/count batch.
/// The opening asteroid atlas contains valid 0..1 UVs, but an inherited guest
/// texture matrix can collapse every sample to the atlas center.
unsafe fn force_texmat_identity_hash_count_begin(count: GLsizei) -> Option<GLint> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_TEXMAT_IDENTITY_HASH_COUNT").ok()?;
    if !opening_projection_active() {
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
        if key.is_some_and(|key| tex_selected(None, &[key])) && wanted_count == Some(count) {
            let mut old_mode: GLint = 0;
            gl21::GetIntegerv(gl21::MATRIX_MODE, &mut old_mode);
            gl21::MatrixMode(gl21::TEXTURE);
            gl21::PushMatrix();
            gl21::LoadIdentity();
            gl21::MatrixMode(old_mode as GLenum);
            log_once!("TOUCHHLE_DS_TEXMAT_IDENTITY_HASH_COUNT: using atlas UVs for selected batch");
            return Some(old_mode);
        }
    }
    None
}

unsafe fn force_texmat_identity_hash_count_end(backup: Option<GLint>) {
    if let Some(old_mode) = backup {
        gl21::MatrixMode(gl21::TEXTURE);
        gl21::PopMatrix();
        gl21::MatrixMode(old_mode as GLenum);
    }
}

struct TexCoordTransformBackup {
    tc_size: GLint,
    tc_type: GLenum,
    tc_stride: GLsizei,
    tc_buffer: GLuint,
    tc_pointer: *const GLvoid,
    active_texture: GLenum,
    client_active_texture: GLenum,
    matrix_mode: GLenum,
    _transformed: Vec<f32>,
}

/// Apply a fixed-function texture matrix on the CPU and submit float texture
/// coordinates with an identity matrix. This preserves the guest's mapping
/// while avoiding driver-dependent interpolation error from tiny matrix scales
/// multiplied by large SHORT texture coordinates.
unsafe fn pretransform_texcoords_begin(
    first: GLint,
    count: GLsizei,
    draw_count: GLsizei,
) -> Option<TexCoordTransformBackup> {
    let spec = crate::ds_draw_env::var("TOUCHHLE_DS_PRETRANSFORM_TEX_HASH").ok()?;
    if count <= 0 || first < 0 || !tex_hash_in_spec(&spec) {
        return None;
    }
    let mut tc_size: GLint = 0;
    let mut tc_type: GLint = 0;
    let mut tc_stride: GLint = 0;
    let mut tc_buffer: GLint = 0;
    gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_SIZE, &mut tc_size);
    gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_TYPE, &mut tc_type);
    gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_STRIDE, &mut tc_stride);
    gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_BUFFER_BINDING, &mut tc_buffer);
    if tc_size != 2 || gl21::IsEnabled(gl21::TEXTURE_COORD_ARRAY) == 0 {
        return None;
    }
    let mut tc_pointer: *mut GLvoid = std::ptr::null_mut();
    #[allow(clippy::unnecessary_mut_passed)]
    gl21::GetPointerv(gl21::TEXTURE_COORD_ARRAY_POINTER, &mut tc_pointer);
    if tc_buffer == 0 && tc_pointer.is_null() {
        return None;
    }
    let mut active_texture: GLint = 0;
    let mut client_active_texture: GLint = 0;
    let mut matrix_mode: GLint = 0;
    gl21::GetIntegerv(gl21::ACTIVE_TEXTURE, &mut active_texture);
    gl21::GetIntegerv(gl21::CLIENT_ACTIVE_TEXTURE, &mut client_active_texture);
    gl21::GetIntegerv(gl21::MATRIX_MODE, &mut matrix_mode);
    gl21::ActiveTexture(active_texture as GLenum);
    let mut matrix = [0.0f32; 16];
    gl21::GetFloatv(gl21::TEXTURE_MATRIX, matrix.as_mut_ptr());

    let vertex_count = first as usize + count as usize;
    let mut raw = vec![0.0f32; vertex_count * 4];
    GLES1OnGL2::ds_read_attrib(
        tc_buffer,
        tc_pointer,
        vertex_count,
        tc_size as usize,
        tc_type,
        tc_stride,
        &mut raw,
        false,
    );
    let clamp_output = crate::ds_draw_env::contains("TOUCHHLE_DS_PRETRANSFORM_TEX_CLAMP");
    let mut transformed = Vec::with_capacity(vertex_count * 2);
    for i in 0..vertex_count {
        let u = raw[i * 4];
        let v = raw[i * 4 + 1];
        let mut tu = matrix[0] * u + matrix[4] * v + matrix[12];
        let mut tv = matrix[1] * u + matrix[5] * v + matrix[13];
        if clamp_output {
            tu = tu.clamp(0.0, 1.0);
            tv = tv.clamp(0.0, 1.0);
        }
        transformed.push(tu);
        transformed.push(tv);
    }
    if let Ok(range) = crate::ds_draw_env::var("TOUCHHLE_DS_PRETRANSFORM_TRACE_RANGE") {
        let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
        let mut parts = range.split(',');
        let from = parts
            .next()
            .and_then(|value| value.trim().parse::<u64>().ok());
        let to = parts
            .next()
            .and_then(|value| value.trim().parse::<u64>().ok());
        if from.is_some_and(|value| frame >= value) && to.is_some_and(|value| frame <= value) {
            let mut raw_u = (f32::MAX, f32::MIN);
            let mut raw_v = (f32::MAX, f32::MIN);
            let mut out_u = (f32::MAX, f32::MIN);
            let mut out_v = (f32::MAX, f32::MIN);
            for i in 0..vertex_count {
                raw_u.0 = raw_u.0.min(raw[i * 4]);
                raw_u.1 = raw_u.1.max(raw[i * 4]);
                raw_v.0 = raw_v.0.min(raw[i * 4 + 1]);
                raw_v.1 = raw_v.1.max(raw[i * 4 + 1]);
                out_u.0 = out_u.0.min(transformed[i * 2]);
                out_u.1 = out_u.1.max(transformed[i * 2]);
                out_v.0 = out_v.0.min(transformed[i * 2 + 1]);
                out_v.1 = out_v.1.max(transformed[i * 2 + 1]);
            }
            let mut depth_func: GLint = 0;
            let mut depth_write: GLboolean = 0;
            let mut blend_src: GLint = 0;
            let mut blend_dst: GLint = 0;
            let mut alpha_func: GLint = 0;
            let mut alpha_ref: GLfloat = 0.0;
            let mut texenv_mode: GLint = 0;
            let mut wrap_s: GLint = 0;
            let mut wrap_t: GLint = 0;
            let mut min_filter: GLint = 0;
            let mut mag_filter: GLint = 0;
            gl21::GetIntegerv(gl21::DEPTH_FUNC, &mut depth_func);
            gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut depth_write);
            gl21::GetIntegerv(gl21::BLEND_SRC, &mut blend_src);
            gl21::GetIntegerv(gl21::BLEND_DST, &mut blend_dst);
            gl21::GetIntegerv(gl21::ALPHA_TEST_FUNC, &mut alpha_func);
            gl21::GetFloatv(gl21::ALPHA_TEST_REF, &mut alpha_ref);
            gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::TEXTURE_ENV_MODE, &mut texenv_mode);
            gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_WRAP_S, &mut wrap_s);
            gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_WRAP_T, &mut wrap_t);
            gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_MIN_FILTER, &mut min_filter);
            gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_MAG_FILTER, &mut mag_filter);
            log!(
                "[DSpretex] f={} key={:08x} draw_count={} first={} count={} tc_type=0x{:x} stride={} buffer={} depth={}/0x{:x}/{} blend={}/{}/{} alpha={}/0x{:x}/{:.3} texenv=0x{:x} wrap={}/{} filter={}/{} matrix={:?} raw_u=[{:.3},{:.3}] raw_v=[{:.3},{:.3}] out_u=[{:.9},{:.9}] out_v=[{:.9},{:.9}]",
                frame,
                bound_texture_key().unwrap_or(0),
                draw_count,
                first,
                count,
                tc_type,
                tc_stride,
                tc_buffer,
                gl21::IsEnabled(gl21::DEPTH_TEST) != 0,
                depth_func,
                depth_write,
                gl21::IsEnabled(gl21::BLEND) != 0,
                blend_src,
                blend_dst,
                gl21::IsEnabled(gl21::ALPHA_TEST) != 0,
                alpha_func,
                alpha_ref,
                texenv_mode,
                wrap_s,
                wrap_t,
                min_filter,
                mag_filter,
                matrix,
                raw_u.0,
                raw_u.1,
                raw_v.0,
                raw_v.1,
                out_u.0,
                out_u.1,
                out_v.0,
                out_v.1
            );
        }
    }

    gl21::MatrixMode(gl21::TEXTURE);
    gl21::PushMatrix();
    gl21::LoadIdentity();
    gl21::ClientActiveTexture(active_texture as GLenum);
    gl21::BindBuffer(gl21::ARRAY_BUFFER, 0);
    gl21::TexCoordPointer(2, gl21::FLOAT, 0, transformed.as_ptr().cast());
    log_once!("TOUCHHLE_DS_PRETRANSFORM_TEX_HASH: pretransforming texture coordinates");
    Some(TexCoordTransformBackup {
        tc_size,
        tc_type: tc_type as GLenum,
        tc_stride,
        tc_buffer: tc_buffer as GLuint,
        tc_pointer: tc_pointer.cast_const(),
        active_texture: active_texture as GLenum,
        client_active_texture: client_active_texture as GLenum,
        matrix_mode: matrix_mode as GLenum,
        _transformed: transformed,
    })
}

unsafe fn pretransform_texcoords_end(backup: Option<TexCoordTransformBackup>) {
    let Some(backup) = backup else {
        return;
    };
    gl21::MatrixMode(gl21::TEXTURE);
    gl21::PopMatrix();
    gl21::MatrixMode(backup.matrix_mode);
    gl21::ActiveTexture(backup.active_texture);
    gl21::ClientActiveTexture(backup.client_active_texture);
    gl21::BindBuffer(gl21::ARRAY_BUFFER, backup.tc_buffer);
    gl21::TexCoordPointer(
        backup.tc_size,
        backup.tc_type,
        backup.tc_stride,
        backup.tc_pointer,
    );
}

fn ds_mv_trace_log<'a>(message: impl Into<trace_file::Message<'a>>) {
    let path = crate::ds_draw_env::var_os("TOUCHHLE_DS_MV_TRACE_FILE");
    let trigger = path.as_ref().and_then(|_| crate::ds_draw_env::var_os("TOUCHHLE_DS_MV_TRACE_TRIGGER"));
    trace_file::append_if_configured(path.as_deref().map(std::path::Path::new),
        trigger.as_deref().map(std::path::Path::new), message);
}

pub struct GLES1OnGL2Context {
    gl_ctx: GLContext,
    state: GLES1OnGL2State,
    is_loaded: bool,
}

/// Build the initial backend state. Centralised so both context constructors
/// stay in sync as fields are added.
fn new_gles1_on_gl2_state() -> GLES1OnGL2State {
    GLES1OnGL2State {
        gpu_profile: gpu_profile::State::default(),
        state_cache: ds_gl_state_cache::Cache::new(),
        scene:crate::ds_scene_diagnostics::RenderState::default(),
        input_prompt_atlas: None,
        opening_fog: ds_opening_fog::OpeningFog::default(),
        pointer_is_fixed_point: [false; ARRAYS.len()],
        fixed_point_texture_units: HashSet::new(),
        fixed_point_translation_buffers: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        matrix_mode: MatrixModeState::ModelView,
        matrix_palette_enabled: false,
        current_palette_matrix: 0,
        // OES_matrix_palette: "all matrices are set to the identity" initially.
        palette_matrices: vec![MATRIX_IDENTITY; MATRIX_PALETTE_MIN_MATRICES],
        palette_weight_state: MatrixArrayPointerState::default(),
        palette_index_state: MatrixArrayPointerState::default(),
        texture_crop_rects: HashMap::new(),
    }
}

impl GLESContext for GLES1OnGL2Context {
    fn description() -> &'static str {
        "OpenGL ES 1.1 via touchHLE GLES1-on-GL2 layer"
    }

    fn new(window: &mut Window) -> Result<Self, String> {
        Ok(Self {
            gl_ctx: window.create_gl_context(GLVersion::GL21Compat)?,
            state: new_gles1_on_gl2_state(),
            is_loaded: false,
        })
    }

    fn make_current<'gl_ctx, 'win: 'gl_ctx>(
        &'gl_ctx mut self,
        window: &'win mut Window,
    ) -> Box<dyn GLES + 'gl_ctx> {
        if self.gl_ctx.is_current() && self.is_loaded {
            return Box::new(GLES1OnGL2 {
                state: &mut self.state,
            });
        }

        unsafe {
            window.make_gl_context_current(&self.gl_ctx);
        }
        gl21::load_with(|s| window.gl_get_proc_address(s));
        self.is_loaded = true;

        Box::new(GLES1OnGL2 {
            state: &mut self.state,
        })
    }

    fn make_current_handle<'gl_ctx, 'win: 'gl_ctx>(
        &'gl_ctx mut self,
        window: &'win mut Window,
    ) -> super::GLESHandle<'gl_ctx> {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let enabled = *ENABLED.get_or_init(|| {
            let on = crate::ds_draw_env::var("TOUCHHLE_DS_GL_STACK_DRIVER").as_deref() == Ok("1");
            eprintln!("[DSgl-stack-driver] effective={}", if on {"stack"} else {"boxed"});
            on
        });
        if !enabled {
            return super::GLESHandle::Boxed(self.make_current(window));
        }
        if !self.gl_ctx.is_current() || !self.is_loaded {
            unsafe { window.make_gl_context_current(&self.gl_ctx); }
            gl21::load_with(|s| window.gl_get_proc_address(s));
            self.is_loaded = true;
        }
        super::GLESHandle::DesktopEs1(GLES1OnGL2 { state: &mut self.state })
    }

    unsafe fn make_current_unchecked_for_window<'gl_ctx>(
        &'gl_ctx mut self,
        make_current_fn: &mut dyn FnMut(&GLContext),
        loader_fn: &mut dyn FnMut(&'static str) -> *const std::ffi::c_void,
    ) -> Box<dyn GLES + 'gl_ctx> {
        if self.gl_ctx.is_current() && self.is_loaded {
            return Box::new(GLES1OnGL2 {
                state: &mut self.state,
            });
        }

        make_current_fn(&self.gl_ctx);
        gl21::load_with(loader_fn);
        self.is_loaded = true;

        Box::new(GLES1OnGL2 {
            state: &mut self.state,
        })
    }
}

pub struct GLES1OnGL2<'a> {
    state: &'a mut GLES1OnGL2State,
}

impl GLES1OnGL2<'_> {
    /// Record a `GL_TEXTURE_CROP_RECT_OES` value for the texture currently
    /// bound to `GL_TEXTURE_2D` on the active unit. The extension scopes this
    /// parameter per texture object, and OpenGL 2.1 has no equivalent, so the
    /// value is kept in `GLES1OnGL2State` instead of being forwarded.
    unsafe fn set_texture_crop_rect(&mut self, values: [GLint; 4]) {
        let mut texture: GLint = 0;
        gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut texture);
        if texture == 0 {
            return;
        }
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static SEEN: AtomicBool = AtomicBool::new(false);
            if !SEEN.swap(true, Ordering::Relaxed) {
                log!(
                    "GL_OES_draw_texture: first crop rect = [{}, {}, {}, {}] on texture {}",
                    values[0],
                    values[1],
                    values[2],
                    values[3],
                    texture
                );
            }
        }
        self.state
            .texture_crop_rects
            .insert(texture as GLuint, values);
    }
    /// One-shot diagnostic: log the first N draw calls together with the
    /// state that decides which texture sub-rect they sample.
    unsafe fn log_draw_probe(&mut self, mode: GLenum, count: GLsizei) {
        // This probe used to run on every process by default, even when all
        // diagnostics were disabled. The first 48 draws perform several
        // synchronous glGet* queries; later draws still paid an atomic bump.
        // Keep the probe available for explicit investigation without adding
        // steady-state GL or logging work to normal gameplay.
        use std::sync::OnceLock;
        static ENABLED: OnceLock<bool> = OnceLock::new();
        if !*ENABLED.get_or_init(|| {
            crate::ds_draw_env::contains("TOUCHHLE_DS_DRAW_PROBE")
        }) {
            return;
        }
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        if n >= 48 {
            return;
        }
        let mut texture: GLint = 0;
        gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut texture);
        let mut active_unit: GLint = 0;
        gl21::GetIntegerv(gl21::ACTIVE_TEXTURE, &mut active_unit);
        let mut tex_matrix = [0.0f32; 16];
        gl21::GetFloatv(gl21::TEXTURE_MATRIX, tex_matrix.as_mut_ptr());
        // also read unit 0's texture matrix for comparison
        let mut unit0_matrix = [0.0f32; 16];
        let saved: GLint = active_unit;
        if saved as GLenum != gl21::TEXTURE0 {
            gl21::ActiveTexture(gl21::TEXTURE0);
            gl21::GetFloatv(gl21::TEXTURE_MATRIX, unit0_matrix.as_mut_ptr());
            gl21::ActiveTexture(saved as GLenum);
        } else {
            unit0_matrix = tex_matrix;
        }
        let mut color = [0.0f32; 4];
        gl21::GetFloatv(gl21::CURRENT_COLOR, color.as_mut_ptr());
        log!(
            "DRAWDBG #{} mode={:#x} count={} tex={} actUnit={:#x} tex2d={} tcArr={} colArr={} texmat_scale=[{:.4},{:.4}] unit0_scale=[{:.4},{:.4}] color=[{:.2},{:.2},{:.2},{:.2}]",
            n,
            mode,
            count,
            texture,
            active_unit,
            gl21::IsEnabled(gl21::TEXTURE_2D) != 0,
            gl21::IsEnabled(gl21::TEXTURE_COORD_ARRAY) != 0,
            gl21::IsEnabled(gl21::COLOR_ARRAY) != 0,
            tex_matrix[0],
            tex_matrix[5],
            unit0_matrix[0],
            unit0_matrix[5],
            color[0],
            color[1],
            color[2],
            color[3]
        );
    }
    /// Read `count` attribute vectors of `size` components into `out`, from either
    /// a client-side array or a bound VBO.
    ///
    /// `ptr` is exactly what the guest handed to `*Pointer`: a real pointer for a
    /// client array, or a **byte offset** when a VBO is bound. The original code
    /// skipped the VBO case entirely, which left `vertices`/`uv` at zero for every
    /// VBO-backed draw (see the M2 note about `vertex_buffer=1`) - that is why
    /// `TOUCHHLE_DS_UV_WATCH` produced no output at all on the HUD, whose quads
    /// live in a buffer object.
    ///
    /// `normalize` retains the legacy diagnostic UV convention. Integer
    /// glTexCoordPointer inputs themselves are NOT normalized by GL; undo this
    /// conversion before applying the actual texture matrix.
    ///
    /// `TOUCHHLE_DS_VBO_READ=0` disables the VBO path for A/B.
    unsafe fn ds_read_attrib(
        buffer: GLint,
        ptr: *mut GLvoid,
        count: usize,
        size: usize,
        type_: GLint,
        stride_i: GLint,
        out: &mut [f32],
        normalize: bool,
    ) {
        if size == 0 || count == 0 {
            return;
        }
        let elem_size = match type_ as GLenum {
            gl21::BYTE | gl21::UNSIGNED_BYTE => 1usize,
            gl21::SHORT | gl21::UNSIGNED_SHORT => 2,
            _ => 4,
        };
        let stride = if stride_i == 0 {
            size * elem_size
        } else {
            stride_i as usize
        };
        let base = ptr as usize;
        let decode = |p: *const u8| -> f32 {
            let raw = match type_ as GLenum {
                gl21::FLOAT => (p as *const f32).read_unaligned(),
                gles11::FIXED => (p as *const i32).read_unaligned() as f32 / 65536.0,
                gl21::UNSIGNED_BYTE => (*(p as *const u8)) as f32,
                gl21::BYTE => (*(p as *const i8)) as f32,
                gl21::UNSIGNED_SHORT => (p as *const u16).read_unaligned() as f32,
                gl21::SHORT => (p as *const i16).read_unaligned() as f32,
                _ => 0.0,
            };
            if !normalize {
                return raw;
            }
            match type_ as GLenum {
                gl21::UNSIGNED_BYTE => raw / 255.0,
                gl21::BYTE => raw / 128.0,
                gl21::UNSIGNED_SHORT => raw / 32768.0,
                gl21::SHORT => raw / 32768.0,
                _ => raw,
            }
        };
        if buffer == 0 {
            if ptr.is_null() {
                return;
            }
            for i in 0..count {
                for c in 0..size {
                    out[i * 4 + c] = decode((ptr as *const u8).add(i * stride + c * elem_size));
                }
            }
            return;
        }
        if crate::ds_draw_env::var("TOUCHHLE_DS_VBO_READ").map_or(false, |v| v == "0") {
            return;
        }
        let need = base + count * stride + elem_size;
        let mut raw = vec![0u8; need];
        let mut prev: GLint = 0;
        gl21::GetIntegerv(gl21::ARRAY_BUFFER_BINDING, &mut prev);
        gl21::BindBuffer(gl21::ARRAY_BUFFER, buffer as GLuint);
        gl21::GetBufferSubData(
            gl21::ARRAY_BUFFER,
            0,
            need as GLsizeiptr,
            raw.as_mut_ptr() as *mut GLvoid,
        );
        gl21::BindBuffer(gl21::ARRAY_BUFFER, prev as GLuint);
        for i in 0..count {
            for c in 0..size {
                let off = base + i * stride + c * elem_size;
                if off + elem_size > raw.len() {
                    continue;
                }
                out[i * 4 + c] = decode(raw.as_ptr().add(off));
            }
        }
    }

    /// One-shot diagnostic for the Dead Space texture/UV investigation.
    /// Enabled only by `TOUCHHLE_DS_TEXTURE_TRACE=1`. Logs each distinct
    /// (texture, dimensions, UVs, vertex positions) draw signature so a
    /// one-off pickup/equipment draw is not buried in repeated scene draws.

    unsafe fn log_ds_texture_draw(
        &mut self,
        kind: &str,
        mode: GLenum,
        first: GLint,
        count: GLsizei,
        indices: Option<(*const GLvoid, GLenum)>,
    ) {
        // The probe recorders live in this function, so it must run whenever
        // any of them is enabled - not only under the (very chatty and slow)
        // full texture trace. Measured 2026-10-11: a door run with
        // TOUCHHLE_DS_PROMPT_AT set but TEXTURE_TRACE unset produced ZERO
        // [DSpromptAT] lines, so the E key fell back to the colour scan and the
        // data-coordinate path was never exercised at all. PROMPT_UV/PROMPT_AT/
        // UV_PROBE/UV_WATCH/DRAW_LOG must each work on their own.
        use std::sync::OnceLock;
        static PROBE_WANTED: OnceLock<bool> = OnceLock::new();
        let probe_wanted = *PROBE_WANTED.get_or_init(|| {
            crate::ds_draw_env::contains("TOUCHHLE_DS_TEXTURE_TRACE")
                || crate::ds_draw_env::contains("TOUCHHLE_DS_UV_PROBE")
                || crate::ds_draw_env::contains("TOUCHHLE_DS_UV_WATCH")
                || crate::ds_draw_env::contains("TOUCHHLE_DS_PROMPT_UV")
                || crate::ds_draw_env::contains("TOUCHHLE_DS_PROMPT_AT")
                || crate::ds_draw_env::contains("TOUCHHLE_DS_DRAW_LOG")
        });
        if !probe_wanted {
            return;
        }
        // Optional trigger file: only trace draws while the file exists.
        // Lets a long run log just the interesting few seconds.
        if let Some(trigger) = crate::ds_draw_env::var_os("TOUCHHLE_DS_DRAW_TRACE_TRIGGER") {
            if !std::path::Path::new(&trigger).exists() {
                return;
            }
        }
        // Optional frame gate, preferred over the trigger file because a file
        // existence check per draw costs a filesystem stat (~1.8x slowdown on
        // an untraced run), while this is a plain integer compare. Use it to
        // trace only the second half of a long capture.
        if let Ok(from) = crate::ds_draw_env::var("TOUCHHLE_DS_TRACE_FROM_FRAME") {
            if let Ok(from) = from.parse::<u64>() {
                if crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed) < from
                {
                    return;
                }
            }
        }

        use std::collections::HashSet;
        use std::hash::{Hash, Hasher};
        use std::sync::Mutex;

        static KEYS: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

        let mut texture: GLint = 0;
        gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut texture);
        let mut tex_w: GLint = 0;
        let mut tex_h: GLint = 0;
        gl21::GetTexLevelParameteriv(gl21::TEXTURE_2D, 0, gl21::TEXTURE_WIDTH, &mut tex_w);
        gl21::GetTexLevelParameteriv(gl21::TEXTURE_2D, 0, gl21::TEXTURE_HEIGHT, &mut tex_h);
        let content_key = crate::ds_texid::lookup(texture as u32).map(|ident| ident.key);
        if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_UV_PROBE_KEY") {
            let keys = crate::ds_texid::parse_keys(&spec);
            if !content_key.is_some_and(|key| keys.contains(&key)) {
                return;
            }
        }
        if let Ok(value) = crate::ds_draw_env::var("TOUCHHLE_DS_UV_PROBE_COUNT") {
            if value
                .trim()
                .parse::<GLsizei>()
                .ok()
                .is_some_and(|wanted| wanted != count)
            {
                return;
            }
        }

        let mut active_unit: GLint = 0;
        let mut client_unit: GLint = 0;
        gl21::GetIntegerv(gl21::ACTIVE_TEXTURE, &mut active_unit);
        gl21::GetIntegerv(gl21::CLIENT_ACTIVE_TEXTURE, &mut client_unit);

        // Texture/model-view matrices are part of the actual sampling and
        // placement math even when the raw UV/vertex arrays look correct.
        // Record them so atlas/page-offset bugs can be distinguished from
        // bad client-side coordinates.
        let mut tex_matrix = [0.0f32; 16];
        gl21::GetFloatv(gl21::TEXTURE_MATRIX, tex_matrix.as_mut_ptr());
        let mut modelview_matrix = [0.0f32; 16];
        gl21::GetFloatv(gl21::MODELVIEW_MATRIX, modelview_matrix.as_mut_ptr());
        let mut projection_matrix = [0.0f32; 16];
        gl21::GetFloatv(gl21::PROJECTION_MATRIX, projection_matrix.as_mut_ptr());
        let tex_matrix_debug = format!("{:?}", tex_matrix);
        let modelview_matrix_debug = format!("{:?}", modelview_matrix);
        let projection_matrix_debug = format!("{:?}", projection_matrix);

        let texture_2d_enabled = gl21::IsEnabled(gl21::TEXTURE_2D) != 0;
        let cull_enabled = gl21::IsEnabled(gl21::CULL_FACE) != 0;
        let blend_enabled = gl21::IsEnabled(gl21::BLEND) != 0;
        let alpha_test_enabled = gl21::IsEnabled(gl21::ALPHA_TEST) != 0;
        let depth_test_enabled = gl21::IsEnabled(gl21::DEPTH_TEST) != 0;
        let mut blend_src: GLint = 0;
        let mut blend_dst: GLint = 0;
        let mut alpha_func: GLint = 0;
        let mut alpha_ref: GLfloat = 0.0;
        let mut depth_func: GLint = 0;
        let mut depth_mask: GLboolean = 0;
        let mut tex_env_mode: GLint = 0;
        let mut current_color = [0.0f32; 4];
        gl21::GetFloatv(gl21::CURRENT_COLOR, current_color.as_mut_ptr());
        let scissor_enabled = gl21::IsEnabled(gl21::SCISSOR_TEST) != 0;
        let mut scissor_box = [0i32; 4];
        let mut viewport = [0i32; 4];
        let mut color_mask = [0u8; 4];
        gl21::GetIntegerv(gl21::SCISSOR_BOX, scissor_box.as_mut_ptr());
        gl21::GetIntegerv(gl21::VIEWPORT, viewport.as_mut_ptr());
        gl21::GetBooleanv(gl21::COLOR_WRITEMASK, color_mask.as_mut_ptr());
        gl21::GetIntegerv(gl21::BLEND_SRC, &mut blend_src);
        gl21::GetIntegerv(gl21::BLEND_DST, &mut blend_dst);
        gl21::GetIntegerv(gl21::ALPHA_TEST_FUNC, &mut alpha_func);
        gl21::GetFloatv(gl21::ALPHA_TEST_REF, &mut alpha_ref);
        gl21::GetIntegerv(gl21::DEPTH_FUNC, &mut depth_func);
        gl21::GetBooleanv(gl21::DEPTH_WRITEMASK, &mut depth_mask);
        gl21::GetTexEnviv(gl21::TEXTURE_ENV, gl21::TEXTURE_ENV_MODE, &mut tex_env_mode);

        let mut tc_size: GLint = 0;
        let mut tc_type: GLint = 0;
        let mut tc_stride: GLint = 0;
        let mut tc_buffer: GLint = 0;
        gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_SIZE, &mut tc_size);
        gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_TYPE, &mut tc_type);
        gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_STRIDE, &mut tc_stride);
        gl21::GetIntegerv(gl21::TEXTURE_COORD_ARRAY_BUFFER_BINDING, &mut tc_buffer);
        let mut tc_ptr: *mut GLvoid = std::ptr::null_mut();
        #[allow(clippy::unnecessary_mut_passed)]
        gl21::GetPointerv(gl21::TEXTURE_COORD_ARRAY_POINTER, &mut tc_ptr);

        let mut vertex_size: GLint = 0;
        let mut vertex_type: GLint = 0;
        let mut vertex_stride: GLint = 0;
        let mut vertex_buffer: GLint = 0;
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_SIZE, &mut vertex_size);
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_TYPE, &mut vertex_type);
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_STRIDE, &mut vertex_stride);
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_BUFFER_BINDING, &mut vertex_buffer);
        let mut vertex_ptr: *mut GLvoid = std::ptr::null_mut();
        #[allow(clippy::unnecessary_mut_passed)]
        gl21::GetPointerv(gl21::VERTEX_ARRAY_POINTER, &mut vertex_ptr);

        let trace_frame =
            crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
        let uv_from = crate::ds_draw_env::var("TOUCHHLE_DS_UV_PROBE_FROM_FRAME")
            .ok()
            .and_then(|value| value.parse::<u64>().ok());
        let uv_to = crate::ds_draw_env::var("TOUCHHLE_DS_UV_PROBE_TO_FRAME")
            .ok()
            .and_then(|value| value.parse::<u64>().ok());
        let uv_probe_frame = crate::ds_draw_env::contains("TOUCHHLE_DS_UV_PROBE")
            && uv_from.map_or(true, |from| trace_frame >= from)
            && uv_to.map_or(true, |to| trace_frame <= to);
        // Size the vertex window from the largest index the draw references.
        // For an indexed draw `count` is the INDEX count and the mesh can have
        // more vertices than a fixed window holds; when `idx < nv` then fails,
        // `order.clear()` hands us sequential vertices - triangles that were
        // never rasterised - and the UV probe matches nothing (measured
        // 2026-10-10: 0 hits for every chapter-12 scene batch).
        // `TOUCHHLE_DS_VERTEX_WINDOW` caps the window (default 8192).
        // Allocating a large vertex window on every draw would otherwise slow
        // unrelated frames down even when the probe is only armed for one frame.
        let window_cap: usize = if uv_probe_frame {
            crate::ds_draw_env::var("TOUCHHLE_DS_VERTEX_WINDOW")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(65536)
        } else {
            1024
        };
        let mut vertex_count = count.clamp(0, window_cap as GLsizei) as usize;
        if let Some((ptr, ity)) = indices {
            // Pass 1: how many vertices do we actually need?
            let elem = match ity {
                0x1401 => 1usize,
                0x1403 => 2,
                _ => 0,
            };
            if elem > 0 {
                let n = count.min(4096) as usize;
                let mut need = 0usize;
                let mut need_min = usize::MAX;
                let mut eab: GLint = 0;
                gl21::GetIntegerv(gl21::ELEMENT_ARRAY_BUFFER_BINDING, &mut eab);
                let mut raw: Vec<u8> = Vec::new();
                let off = ptr as usize;
                if eab != 0 {
                    let want = off + n * elem;
                    if want > off && want <= (16 << 20) {
                        raw.resize(want, 0);
                        gl21::GetBufferSubData(
                            gl21::ELEMENT_ARRAY_BUFFER,
                            0,
                            want as GLsizeiptr,
                            raw.as_mut_ptr() as *mut GLvoid,
                        );
                    }
                }
                for i in 0..n {
                    let idx = if !raw.is_empty() {
                        if elem == 1 {
                            raw[off + i] as usize
                        } else {
                            u16::from_le_bytes([raw[off + i * 2], raw[off + i * 2 + 1]]) as usize
                        }
                    } else if !ptr.is_null() {
                        if elem == 1 {
                            *(ptr as *const u8).add(i) as usize
                        } else {
                            *(ptr as *const u16).add(i) as usize
                        }
                    } else {
                        0
                    };
                    if idx > need {
                        need = idx;
                    }
                    if idx < need_min {
                        need_min = idx;
                    }
                }
                if need > 0 {
                    vertex_count = (need + 1).min(window_cap);
                    if content_key == Some(0x35e0_bc22) {
                        log!(
                            "[DSTEXvwin] f={} tex={} count={} index_min={} index_max={} window={}",
                            crate::window::DS_FRAME_COUNTER
                                .load(std::sync::atomic::Ordering::Relaxed),
                            texture,
                            count,
                            need_min,
                            need,
                            vertex_count
                        );
                    }
                }
            }
        }
        let mut vertices = vec![0.0f32; window_cap * 4];
        let mut uv = vec![0.0f32; window_cap * 4];
        let vertex_size = vertex_size.clamp(0, 4) as usize;
        let tc_size = tc_size.clamp(0, 4) as usize;

        Self::ds_read_attrib(
            vertex_buffer,
            vertex_ptr,
            vertex_count,
            vertex_size,
            vertex_type,
            vertex_stride,
            &mut vertices,
            false,
        );
        Self::ds_read_attrib(
            tc_buffer,
            tc_ptr,
            vertex_count,
            tc_size,
            tc_type,
            tc_stride,
            &mut uv,
            true,
        );

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        kind.hash(&mut hasher);
        texture.hash(&mut hasher);
        cull_enabled.hash(&mut hasher);
        scissor_enabled.hash(&mut hasher);
        scissor_box.hash(&mut hasher);
        viewport.hash(&mut hasher);
        color_mask.hash(&mut hasher);
        tex_w.hash(&mut hasher);
        tex_h.hash(&mut hasher);
        active_unit.hash(&mut hasher);
        client_unit.hash(&mut hasher);
        tc_size.hash(&mut hasher);
        tc_type.hash(&mut hasher);
        tc_stride.hash(&mut hasher);
        tc_buffer.hash(&mut hasher);
        vertex_size.hash(&mut hasher);
        vertex_type.hash(&mut hasher);
        vertex_stride.hash(&mut hasher);
        vertex_buffer.hash(&mut hasher);
        for v in tex_matrix {
            ((v * 100000.0) as i64).hash(&mut hasher);
        }
        for v in modelview_matrix {
            ((v * 100000.0) as i64).hash(&mut hasher);
        }
        for v in &vertices {
            ((v * 10000.0) as i64).hash(&mut hasher);
        }
        for v in &uv {
            ((v * 100000.0) as i64).hash(&mut hasher);
        }
        let key = hasher.finish();
        let keys = KEYS.get_or_init(|| Mutex::new(HashSet::new()));
        // `TOUCHHLE_DS_UV_PROBE` / `TOUCHHLE_DS_UV_WATCH` must see *every* draw,
        // not just the first occurrence of each signature: this dedup set persists
        // across frames, so by the time a probe frame arrives the common HUD and
        // scene draws have all been registered already and the early return would
        // stop the probe from ever running. Measured 2026-10-10: 23 883 traced
        // draws but 0 probe hits - even the full-screen UI quad, which certainly
        // covers the probe point, produced nothing.
        let probe_active = crate::ds_draw_env::contains("TOUCHHLE_DS_UV_PROBE")
        || crate::ds_draw_env::contains("TOUCHHLE_DS_UV_WATCH")
        // The prompt recorders need the same treatment: with the dedup gate
        // active they would only ever see the *first* occurrence of each draw
        // signature, and the `keys.len() > limit` early return stops the whole
        // block once the budget is spent. Both happen long before the door
        // prompt is drawn, so `PROMPT_AT` recorded nothing at all in the first
        // run (2026-10-10 06:0x: E fell back to the colour scan with 0 hits).
        || crate::ds_draw_env::contains("TOUCHHLE_DS_PROMPT_UV")
        || crate::ds_draw_env::contains("TOUCHHLE_DS_PROMPT_AT")
        // `TOUCHHLE_DS_DRAW_LOG` needs every draw too: with the dedup gate on it
        // would only see the first occurrence of each signature and then stop
        // entirely once DRAW_LIMIT signatures are registered (measured
        // 2026-10-10: a whole chapter-12 run produced 0 `[DSdraw]` lines).
        || crate::ds_draw_env::contains("TOUCHHLE_DS_DRAW_LOG");
        // `dedup_ok` still gates the one-shot `[DSTEX draw]` dump and the terse
        // trace, so those keep their "first time we see this signature" meaning.
        let dedup_ok = {
            // The dedup cap keeps the log bounded, but it is easy to hit
            // before the draw of interest is reached. Allow raising it for
            // targeted captures (e.g. tracking down one magnified sprite).
            let limit: usize = crate::ds_draw_env::var("TOUCHHLE_DS_DRAW_LIMIT")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(4000);
            let mut keys = keys.lock().unwrap();
            let first = keys.insert(key);
            if !probe_active && (!first || keys.len() > limit) {
                return;
            }
            first && keys.len() <= limit
        };

        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // `TOUCHHLE_DS_UV_PROBE="x,y[;x,y...]"` answers "which part of which
        // texture paints this landscape point?". The in-game HUD is drawn as
        // big batched draws whose per-draw UV range unions to most of the
        // atlas, so neither the draw call nor a per-draw UV box can single out
        // a widget - but the triangle that actually covers the point can.
        // Points use the same 480x320 landscape space as the script `tap`s.
        if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_UV_PROBE").or_else(|_| {
            crate::ds_draw_env::var("TOUCHHLE_DS_PROMPT_UV")
                .or_else(|_| crate::ds_draw_env::var("TOUCHHLE_DS_PROMPT_AT").map(|_| String::new()))
                .map(|_| String::new())
        }) {
            let from = crate::ds_draw_env::var("TOUCHHLE_DS_UV_PROBE_FROM_FRAME")
                .ok()
                .and_then(|v| v.parse::<u64>().ok());
            let to = crate::ds_draw_env::var("TOUCHHLE_DS_UV_PROBE_TO_FRAME")
                .ok()
                .and_then(|v| v.parse::<u64>().ok());
            let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
            let wait_for_dark = (crate::ds_draw_env::contains("TOUCHHLE_DS_UV_PROBE_ON_DARK")
                || crate::ds_draw_env::contains("TOUCHHLE_DS_UV_PROBE_ON_BRIGHT"))
                && !PIXEL_DARK_TRACE_ARMED.load(std::sync::atomic::Ordering::Relaxed);
            if !wait_for_dark
                && from.map_or(true, |f| frame >= f)
                && to.map_or(true, |f| frame <= f)
            {
                let mut targets: Vec<(f32, f32)> = Vec::new();
                for pair in spec.split(';') {
                    let mut it = pair.split(',');
                    if let (Some(a), Some(b)) = (
                        it.next().and_then(|v| v.trim().parse::<f32>().ok()),
                        it.next().and_then(|v| v.trim().parse::<f32>().ok()),
                    ) {
                        targets.push((a, b));
                    }
                }
                // The colour buffer is a portrait target that the compositor
                // rotates by 90 degrees (see the frame dumper), so landscape
                // X = GL-y * 480/vh and landscape Y = GL-x * 320/vw. Both
                // factors cancel against the NDC->window scaling, so the
                // conversion below is just "NDC to 480x320 points" and stays
                // correct under --scale-hack too.
                let mut px = vec![0.0f32; window_cap];
                let mut py = vec![0.0f32; window_cap];
                let mut pz = vec![0.0f32; window_cap];
                let mut pw = vec![0.0f32; window_cap];
                let mut puv = vec![0.0f32; window_cap * 2];
                let uv_raw_scale = match tc_type as GLenum {
                    gl21::UNSIGNED_BYTE => 255.0,
                    gl21::BYTE => 128.0,
                    gl21::SHORT | gl21::UNSIGNED_SHORT => 32768.0,
                    _ => 1.0,
                };
                let mut nv = 0usize;
                let mut usable = true;
                for i in 0..vertex_count {
                    let ch = &vertices[i * 4..i * 4 + 4];
                    let (x, y, z) = (ch[0], ch[1], ch[2]);
                    let w = if vertex_size >= 4 { ch[3] } else { 1.0 };
                    let mv = modelview_matrix;
                    let pj = projection_matrix;
                    let ex = mv[0] * x + mv[4] * y + mv[8] * z + mv[12] * w;
                    let ey = mv[1] * x + mv[5] * y + mv[9] * z + mv[13] * w;
                    let ez = mv[2] * x + mv[6] * y + mv[10] * z + mv[14] * w;
                    let ew = mv[3] * x + mv[7] * y + mv[11] * z + mv[15] * w;
                    let cx = pj[0] * ex + pj[4] * ey + pj[8] * ez + pj[12] * ew;
                    let cy = pj[1] * ex + pj[5] * ey + pj[9] * ez + pj[13] * ew;
                    let cz = pj[2] * ex + pj[6] * ey + pj[10] * ez + pj[14] * ew;
                    let cw = pj[3] * ex + pj[7] * ey + pj[11] * ez + pj[15] * ew;
                    if cw == 0.0 {
                        usable = false;
                        break;
                    }
                    let (nx, ny) = (cx / cw, cy / cw);
                    px[nv] = (ny + 1.0) * 0.5 * 480.0;
                    py[nv] = (nx + 1.0) * 0.5 * 320.0;
                    pz[nv] = cz / cw;
                    pw[nv] = cw;
                    puv[nv * 2] = uv[i * 4];
                    puv[nv * 2 + 1] = uv[i * 4 + 1];
                    nv += 1;
                    if nv >= window_cap {
                        break;
                    }
                }
                if usable && nv >= 3 {
                    // `TOUCHHLE_DS_DRAW_LOG=<max lines>` - compact per-draw log:
                    // one line per distinct (texture, screen box) shape that is being
                    // drawn, with the presented frame number. Answers "what is the
                    // program drawing on this frame?" without guessing a probe point -
                    // used 2026-10-10 to hunt the interaction ring by comparing the
                    // draws while the ring is on screen with the draws when it is not.
                    if let Ok(limit) = crate::ds_draw_env::var("TOUCHHLE_DS_DRAW_LOG") {
                        // Entry probe: tells "the block never ran" apart from "it ran but
                        // the filters dropped everything" (measured 2026-10-10: a full
                        // chapter-12 run logged zero [DSdraw] lines).
                        log_once!("[DSdraw] armed");
                        let limit: usize = limit.trim().parse().unwrap_or(400);
                        // Optional trigger file: only log while it exists, so a run can
                        // capture just the seconds of interest (the log cap is otherwise
                        // eaten by the first frames - measured: 400 lines gone by f=65).
                        let trigger_ok = crate::ds_draw_env::var("TOUCHHLE_DS_DRAW_LOG_TRIGGER")
                            .map(|p| std::path::Path::new(&p).exists())
                            .unwrap_or(true);
                        use std::sync::atomic::{AtomicUsize as DLogCtr, Ordering as DLogOrd};
                        static DLOG_SEEN: std::sync::OnceLock<std::sync::Mutex<Vec<u64>>> =
                            std::sync::OnceLock::new();
                        let (mut x0, mut y0, mut x1, mut y1) =
                            (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                        for i in 0..nv {
                            x0 = x0.min(px[i]);
                            x1 = x1.max(px[i]);
                            y0 = y0.min(py[i]);
                            y1 = y1.max(py[i]);
                        }
                        // Several draw paths (VBO reads, near-degenerate matrices)
                        // project to junk like +-1e6; those boxes are useless and eat
                        // the cap, so drop anything outside a generous sane window.
                        let sane = x0.is_finite()
                            && y0.is_finite()
                            && x1.is_finite()
                            && y1.is_finite()
                            && x0 > -1000.0
                            && y0 > -1000.0
                            && x1 < 2000.0
                            && y1 < 2000.0;
                        // Optional filters, so the small HUD sprites (e.g. the 15x15
                        // single-character quad of the in-world prompt) can be found
                        // without the scene draws eating the line budget, and without
                        // having to time a trigger window:
                        //   TOUCHHLE_DS_DRAW_LOG_MAX=<span>      only boxes no bigger
                        //   TOUCHHLE_DS_DRAW_LOG_BOX="x0,y0,x1,y1" only boxes touching
                        let max_span: f32 = crate::ds_draw_env::var("TOUCHHLE_DS_DRAW_LOG_MAX")
                            .ok()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(f32::MAX);
                        let want_box =
                            crate::ds_draw_env::var("TOUCHHLE_DS_DRAW_LOG_BOX")
                                .ok()
                                .and_then(|s| {
                                    let v: Vec<f32> = s
                                        .split(',')
                                        .filter_map(|t| t.trim().parse::<f32>().ok())
                                        .collect();
                                    if v.len() == 4 {
                                        Some((v[0], v[1], v[2], v[3]))
                                    } else {
                                        None
                                    }
                                });
                        let span_ok = (x1 - x0).max(y1 - y0) <= max_span;
                        let box_ok = want_box.map_or(true, |(bx0, by0, bx1, by1)| {
                            x1 >= bx0 && x0 <= bx1 && y1 >= by0 && y0 <= by1
                        });
                        let sig = ((texture as i64 as u64 & 0xffff) << 40)
                            | (((x0 * 4.0) as i64 as u64 & 0xfffff) << 20)
                            | ((y0 * 4.0) as i64 as u64 & 0xfffff);
                        static DLOG_LINES: DLogCtr = DLogCtr::new(0);
                        let seen = DLOG_SEEN.get_or_init(|| std::sync::Mutex::new(Vec::new()));
                        let mut seen = seen.lock().unwrap();
                        if trigger_ok && sane && span_ok && box_ok && !seen.contains(&sig) {
                            let n = DLOG_LINES.fetch_add(1, DLogOrd::Relaxed);
                            if n < limit {
                                log!(
                            "[DSdraw] f={} tex={} {}x{} nv={} box=[{:.0},{:.0}..{:.0},{:.0}]",
                            crate::window::DS_FRAME_COUNTER
                                .load(std::sync::atomic::Ordering::Relaxed),
                            texture, tex_w, tex_h, nv,
                            x0, y0, x1, y1
                        );
                            }
                            seen.push(sig);
                        }
                    }
                    let strip = mode == 0x5; // GL_TRIANGLE_STRIP
                                             // For an indexed draw the captured array is in *vertex*
                                             // order, not in the order the triangles are built, so the
                                             // index buffer has to be walked; grouping consecutive
                                             // entries would test triangles that are never rasterised.
                                             // Indices may live in a *buffer object*: `glDrawElements` then receives a
                                             // byte offset (usually 0) instead of a pointer, so the `!ptr.is_null()`
                                             // test below fails and `order` falls back to sequential vertices - which
                                             // are not the triangles that were rasterised. Measured 2026-10-10: every
                                             // scene batch in the chapter-12 corridor produced 0 probe hits for this
                                             // reason, while the non-indexed main-menu quad matched fine.
                    let mut idx_raw: Vec<u8> = Vec::new();
                    let mut idx_off = 0usize;
                    if let Some((ptr, ity)) = indices {
                        let esz = match ity {
                            0x1401 => 1usize,
                            0x1403 => 2,
                            _ => 0,
                        };
                        let mut eab: GLint = 0;
                        gl21::GetIntegerv(gl21::ELEMENT_ARRAY_BUFFER_BINDING, &mut eab);
                        if esz > 0 && eab != 0 {
                            idx_off = ptr as usize;
                            let want = idx_off + count.min(4096) as usize * esz;
                            if want > idx_off && want <= (16 << 20) {
                                idx_raw.resize(want, 0);
                                gl21::GetBufferSubData(
                                    gl21::ELEMENT_ARRAY_BUFFER,
                                    0,
                                    want as GLsizeiptr,
                                    idx_raw.as_mut_ptr() as *mut GLvoid,
                                );
                            }
                        }
                    }
                    let mut order: Vec<usize> = Vec::with_capacity(nv);
                    if let Some((ptr, ity)) = indices {
                        if !idx_raw.is_empty() || !ptr.is_null() {
                            for i in 0..count.min(4096) as usize {
                                let idx = match ity {
                                    0x1401 => {
                                        if idx_raw.is_empty() {
                                            *(ptr as *const u8).add(i) as usize
                                        } else {
                                            idx_raw[idx_off + i] as usize
                                        }
                                    }
                                    0x1403 => {
                                        if idx_raw.is_empty() {
                                            *(ptr as *const u16).add(i) as usize
                                        } else {
                                            u16::from_le_bytes([
                                                idx_raw[idx_off + i * 2],
                                                idx_raw[idx_off + i * 2 + 1],
                                            ]) as usize
                                        }
                                    }
                                    _ => {
                                        order.clear();
                                        break;
                                    }
                                };
                                if idx < nv {
                                    order.push(idx);
                                } else {
                                    order.clear();
                                    break;
                                }
                            }
                        }
                    }
                    if order.is_empty() {
                        order = (0..nv).collect();
                    }
                    let m = order.len();
                    let tris: Vec<[usize; 3]> = if strip {
                        (0..m.saturating_sub(2))
                            .map(|i| [order[i], order[i + 1], order[i + 2]])
                            .collect()
                    } else {
                        (0..(m / 3))
                            .map(|i| [order[i * 3], order[i * 3 + 1], order[i * 3 + 2]])
                            .collect()
                    };
                    if let (Ok(dir), Ok(keys)) = (
                        crate::ds_draw_env::var("TOUCHHLE_DS_MESH_DUMP_DIR"),
                        crate::ds_draw_env::var("TOUCHHLE_DS_MESH_DUMP_KEYS"),
                    ) {
                        if content_key.is_some_and(|key| crate::ds_texid::parse_keys(&keys).contains(&key)) {
                            use std::fmt::Write;
                            let mut dump = format!(
                                "# projection={:?}\n# modelview={:?}\n# texmatrix={:?}\n# cull={} depth={}:{}:{}\ntri,vertex,x,y,z,w,u,v\n",
                                projection_matrix, modelview_matrix, tex_matrix,
                                cull_enabled, depth_test_enabled, depth_func, depth_mask,
                            );
                            for (ti, tri) in tris.iter().enumerate() {
                                for &vi in tri {
                                    let u = puv[vi * 2] * uv_raw_scale;
                                    let v = puv[vi * 2 + 1] * uv_raw_scale;
                                    let tu = tex_matrix[0] * u + tex_matrix[4] * v + tex_matrix[12];
                                    let tv = tex_matrix[1] * u + tex_matrix[5] * v + tex_matrix[13];
                                    let tq = tex_matrix[3] * u + tex_matrix[7] * v + tex_matrix[15];
                                    let _ = writeln!(dump, "{ti},{vi},{},{},{},{},{},{}", px[vi], py[vi], pz[vi], pw[vi], tu / tq, tv / tq);
                                }
                            }
                            let path = std::path::Path::new(&dir).join(format!("mesh-f{frame}-{n}-{:08x}-{count}.csv", content_key.unwrap_or(0)));
                            if let Err(error) = std::fs::write(&path, dump) {
                                log!("[DSmesh] failed to write {:?}: {}", path, error);
                            }
                        }
                    }
                    for (ti, tri) in tris.iter().enumerate() {
                        // TOUCHHLE_DS_PROMPT_UV="tex,u0,u1,v0,v1" - record where the prompt sprite is
                        // drawn so the E key can tap its data coordinates instead of scanning the
                        // Field 0 may be a GL texture name ("18") or, preferably, a texture size
                        // ("512x256"). GL names are per-run and therefore useless as a spec; the
                        // size plus the UV rectangle is stable across runs.
                        // TOUCHHLE_DS_PROMPT_AT="x,y[,max_span]" - position-based prompt match. Far
                        // more robust than a UV spec: it keys on *where* a small (HUD-sized)
                        // triangle covers the requested screen point, so it needs no per-run
                        // texture/UV calibration. max_span defaults to 160 pt.
                        if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_PROMPT_AT") {
                            let mut it = spec.split(',');
                            let txo = it.next().and_then(|s| s.trim().parse::<f32>().ok());
                            let tyo = it.next().and_then(|s| s.trim().parse::<f32>().ok());
                            let max_span = it
                                .next()
                                .and_then(|s| s.trim().parse::<f32>().ok())
                                .unwrap_or(160.0);
                            // Optional 4th field = sprite filter slack, in points. When > 0 a
                            // candidate is only accepted if its UV rectangle maps to its screen
                            // rectangle at ~1:1 texel-per-point (the signature of a HUD sprite
                            // blitted at native size) within this many points on each axis. That
                            // rejects stretched scene polygons - in particular the corridor floor
                            // quads that also cover the probe point. 0 / missing = off.
                            let ratio_tol = it
                                .next()
                                .and_then(|s| s.trim().parse::<f32>().ok())
                                .unwrap_or(0.0);
                            // Optional area of interest: "x0,y0,x1,y1" in the same
                            // 480x320 points. Only candidates whose screen box
                            // intersects it are matched *and* logged, so one run
                            // lists what the door area draws instead of the first
                            // 60 shapes seen anywhere - which the main-menu frames
                            // eat before the chapter has even loaded (measured
                            // 2026-10-10: every census line came from the menu).
                            let want_box = crate::ds_draw_env::var("TOUCHHLE_DS_PROMPT_AT_BOX")
                                .ok()
                                .and_then(|s| {
                                    let v: Vec<f32> = s
                                        .split(',')
                                        .filter_map(|t| t.trim().parse::<f32>().ok())
                                        .collect();
                                    if v.len() == 4 {
                                        Some((v[0], v[1], v[2], v[3]))
                                    } else {
                                        None
                                    }
                                });
                            if let (Some(tx), Some(ty)) = (txo, tyo) {
                                let (ax, ay) = (px[tri[0]], py[tri[0]]);
                                let (bx, by) = (px[tri[1]], py[tri[1]]);
                                let (cx2, cy2) = (px[tri[2]], py[tri[2]]);
                                let span = (ax.max(bx).max(cx2) - ax.min(bx).min(cx2))
                                    .max(ay.max(by).max(cy2) - ay.min(by).min(cy2));
                                let in_box = want_box.map_or(true, |(x0, y0, x1, y1)| {
                                    ax.max(bx).max(cx2) >= x0
                                        && ax.min(bx).min(cx2) <= x1
                                        && ay.max(by).max(cy2) >= y0
                                        && ay.min(by).min(cy2) <= y1
                                });
                                if span > 0.0 && span <= max_span && in_box {
                                    let s1 = (bx - ax) * (ty - ay) - (by - ay) * (tx - ax);
                                    let s2 = (cx2 - bx) * (ty - by) - (cy2 - by) * (tx - bx);
                                    let s3 = (ax - cx2) * (ty - cy2) - (ay - cy2) * (tx - cx2);
                                    let inside = (s1 >= 0.0 && s2 >= 0.0 && s3 >= 0.0)
                                        || (s1 <= 0.0 && s2 <= 0.0 && s3 <= 0.0);
                                    if inside {
                                        let xs = [ax, bx, cx2];
                                        let ys = [ay, by, cy2];
                                        let us =
                                            [puv[tri[0] * 2], puv[tri[1] * 2], puv[tri[2] * 2]];
                                        let vs = [
                                            puv[tri[0] * 2 + 1],
                                            puv[tri[1] * 2 + 1],
                                            puv[tri[2] * 2 + 1],
                                        ];
                                        let w_pt = xs.iter().cloned().fold(f32::MIN, f32::max)
                                            - xs.iter().cloned().fold(f32::MAX, f32::min);
                                        let h_pt = ys.iter().cloned().fold(f32::MIN, f32::max)
                                            - ys.iter().cloned().fold(f32::MAX, f32::min);
                                        let u_px = (us.iter().cloned().fold(f32::MIN, f32::max)
                                            - us.iter().cloned().fold(f32::MAX, f32::min))
                                            * tex_w as f32;
                                        let v_px = (vs.iter().cloned().fold(f32::MIN, f32::max)
                                            - vs.iter().cloned().fold(f32::MAX, f32::min))
                                            * tex_h as f32;
                                        let du = (u_px - w_pt).abs();
                                        let dv = (v_px - h_pt).abs();
                                        let sprite_like = du <= ratio_tol && dv <= ratio_tol;
                                        // Capped one-shot diagnostic: proves the position recorder
                                        // actually fires and shows which triangle won, so a failed
                                        // run can be told apart from "the block never ran".
                                        {
                                            // Census, deduped by shape: one line per *kind* of
                                            // triangle that covers the probe point, so a single run
                                            // enumerates the candidates (scene floor quads, HUD
                                            // sprites, ...) instead of repeating the same draw.
                                            static ATSIGS: std::sync::OnceLock<
                                                std::sync::Mutex<Vec<u64>>,
                                            > = std::sync::OnceLock::new();
                                            let sig = ((tex_w as i64 as u64 & 0xffff) << 48)
                                                | ((tex_h as i64 as u64 & 0xffff) << 32)
                                                | (((u_px * 2.0) as i64 as u64 & 0xffff) << 16)
                                                | ((v_px * 2.0) as i64 as u64 & 0xffff);
                                            let sigs = ATSIGS
                                                .get_or_init(|| std::sync::Mutex::new(Vec::new()));
                                            let mut seen = sigs.lock().unwrap();
                                            if !seen.contains(&sig) {
                                                if seen.len() < 60 {
                                                    log!(
                                                             "[DSpromptAT] tex={} {}x{} sbox=[{:.0},{:.0}..{:.0},{:.0}] uvpx=[{:.1}x{:.1}] d=[{:.1},{:.1}] span={:.1} sprite_like={} texid={}",
                                                             texture, tex_w, tex_h,
                                                             xs.iter().cloned().fold(f32::MAX, f32::min),
                                                             ys.iter().cloned().fold(f32::MAX, f32::min),
                                                             xs.iter().cloned().fold(f32::MIN, f32::max),
                                                             ys.iter().cloned().fold(f32::MIN, f32::max),
                                                             u_px, v_px, du, dv, span, sprite_like,
                                                             texture
                                                         );
                                                }
                                                seen.push(sig);
                                            }
                                            drop(seen);
                                        }
                                        if ratio_tol <= 0.0 || sprite_like {
                                            crate::window::ds_set_prompt_xy(
                                                (ax + bx + cx2) / 3.0,
                                                (ay + by + cy2) / 3.0,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_PROMPT_UV") {
                            let mut it = spec.split(',');
                            let key = it.next().unwrap_or("").trim().to_string();
                            let rest: Vec<f32> =
                                it.filter_map(|s| s.trim().parse::<f32>().ok()).collect();
                            let key_ok = if let Some((w, h)) = key.split_once('x') {
                                w.trim().parse::<i32>().ok() == Some(tex_w)
                                    && h.trim().parse::<i32>().ok() == Some(tex_h)
                            } else {
                                key.parse::<i32>().ok() == Some(texture)
                            };
                            // Diagnostic: prove the recorder runs and show what the door-area draws
                            // actually carry. Capped so a long run cannot flood the log.
                            {
                                use std::sync::atomic::{AtomicUsize, Ordering as DSord};
                                static DPSEEN: AtomicUsize = AtomicUsize::new(0);
                                let dk = DPSEEN.fetch_add(1, DSord::Relaxed);
                                if dk < 2000 {
                                    let us = [puv[tri[0] * 2], puv[tri[1] * 2], puv[tri[2] * 2]];
                                    let vs = [
                                        puv[tri[0] * 2 + 1],
                                        puv[tri[1] * 2 + 1],
                                        puv[tri[2] * 2 + 1],
                                    ];
                                    let xs = [px[tri[0]], px[tri[1]], px[tri[2]]];
                                    let ys = [py[tri[0]], py[tri[1]], py[tri[2]]];
                                    log!(
                                        "[DSprompt] tex={} {}x{} sbox=[{:.0},{:.0}..{:.0},{:.0}] uv=[{:.4},{:.4} | {:.4},{:.4} | {:.4},{:.4}] bbox=[{:.4},{:.4}..{:.4},{:.4}] key_ok={}",
                                        texture, tex_w, tex_h,
                                        xs.iter().cloned().fold(f32::MAX, f32::min),
                                        ys.iter().cloned().fold(f32::MAX, f32::min),
                                        xs.iter().cloned().fold(f32::MIN, f32::max),
                                        ys.iter().cloned().fold(f32::MIN, f32::max),
                                        us[0], vs[0], us[1], vs[1], us[2], vs[2],
                                        us.iter().cloned().fold(f32::MAX, f32::min),
                                        vs.iter().cloned().fold(f32::MAX, f32::min),
                                        us.iter().cloned().fold(f32::MIN, f32::max),
                                        vs.iter().cloned().fold(f32::MIN, f32::max),
                                        key_ok
                                    );
                                }
                            }
                            if key_ok && rest.len() == 4 {
                                let v = &rest;
                                let (pu0, pu1) = (v[0].min(v[1]), v[0].max(v[1]));
                                let (pv0, pv1) = (v[2].min(v[3]), v[2].max(v[3]));
                                let us = [puv[tri[0] * 2], puv[tri[1] * 2], puv[tri[2] * 2]];
                                let vs = [
                                    puv[tri[0] * 2 + 1],
                                    puv[tri[1] * 2 + 1],
                                    puv[tri[2] * 2 + 1],
                                ];
                                let xs = [px[tri[0]], px[tri[1]], px[tri[2]]];
                                let ys = [py[tri[0]], py[tri[1]], py[tri[2]]];
                                let (umin, umax) = (
                                    us.iter().cloned().fold(f32::MAX, f32::min),
                                    us.iter().cloned().fold(f32::MIN, f32::max),
                                );
                                let (vmin, vmax) = (
                                    vs.iter().cloned().fold(f32::MAX, f32::min),
                                    vs.iter().cloned().fold(f32::MIN, f32::max),
                                );
                                if umin <= pu1 && umax >= pu0 && vmin <= pv1 && vmax >= pv0 {
                                    let cx = (px[tri[0]] + px[tri[1]] + px[tri[2]]) / 3.0;
                                    let cy = (py[tri[0]] + py[tri[1]] + py[tri[2]]) / 3.0;
                                    crate::window::ds_set_prompt_xy(cx, cy);
                                }
                            }
                        }
                        for &(tx, ty) in &targets {
                            let (ax, ay) = (px[tri[0]], py[tri[0]]);
                            let (bx, by) = (px[tri[1]], py[tri[1]]);
                            let (cxx, cyy) = (px[tri[2]], py[tri[2]]);
                            let s = (bx - ax) * (ty - ay) - (by - ay) * (tx - ax);
                            let s2 = (cxx - bx) * (ty - by) - (cyy - by) * (tx - bx);
                            // Skip degenerate triangles. Vertex arrays are capped at 64
                            // entries while indexed batches are far larger, so indices
                            // past that read as zeros and collapse to a single point; a
                            // zero-area triangle then passes the sign test below for any
                            // probe point (all three cross products are 0). Measured
                            // 2026-10-10: every one of the 200k+ probe hits at the
                            // chapter-12 door was such a collapsed triangle.
                            let tri_area2 = (bx - ax) * (cyy - ay) - (by - ay) * (cxx - ax);
                            if tri_area2.abs() < 1e-6 {
                                continue;
                            }
                            let s3 = (ax - cxx) * (ty - cyy) - (ay - cyy) * (tx - cxx);
                            let inside = (s >= 0.0 && s2 >= 0.0 && s3 >= 0.0)
                                || (s <= 0.0 && s2 <= 0.0 && s3 <= 0.0);
                            if inside {
                                let mut transformed_uv = [(0.0f32, 0.0f32); 3];
                                for (dst, vi) in transformed_uv.iter_mut().zip(tri.iter()) {
                                    let u = puv[vi * 2] * uv_raw_scale;
                                    let v = puv[vi * 2 + 1] * uv_raw_scale;
                                    let tu = tex_matrix[0] * u + tex_matrix[4] * v + tex_matrix[12];
                                    let tv = tex_matrix[1] * u + tex_matrix[5] * v + tex_matrix[13];
                                    let tw = tex_matrix[3] * u + tex_matrix[7] * v + tex_matrix[15];
                                    *dst = if tw != 0.0 {
                                        (tu / tw, tv / tw)
                                    } else {
                                        (tu, tv)
                                    };
                                }
                                log!(
                                    "[DSTEXuv] f={} #{} probe=({:.0},{:.0}) draw={} count={} tri={} tex={} key={:08x} {}x{} uv=[{:.4},{:.4} | {:.4},{:.4} | {:.4},{:.4}] tuv=[{:.4},{:.4} | {:.4},{:.4} | {:.4},{:.4}] texmat={:?} pt=[{:.1},{:.1} | {:.1},{:.1} | {:.1},{:.1}] z=[{:.6},{:.6},{:.6}] blend={} alpha={} depth={}:{}:{} cull={}",
                                    frame, n, tx, ty, kind, count, ti, texture, content_key.unwrap_or(0), tex_w, tex_h,
                                    puv[tri[0] * 2], puv[tri[0] * 2 + 1],
                                    puv[tri[1] * 2], puv[tri[1] * 2 + 1],
                                    puv[tri[2] * 2], puv[tri[2] * 2 + 1],
                                    transformed_uv[0].0, transformed_uv[0].1,
                                    transformed_uv[1].0, transformed_uv[1].1,
                                    transformed_uv[2].0, transformed_uv[2].1,
                                    tex_matrix,
                                    px[tri[0]], py[tri[0]],
                                    px[tri[1]], py[tri[1]],
                                    px[tri[2]], py[tri[2]],
                                    pz[tri[0]], pz[tri[1]], pz[tri[2]],
                                    blend_enabled,
                                    alpha_test_enabled,
                                    depth_test_enabled,
                                    depth_func,
                                    depth_mask,
                                    cull_enabled
                                );
                            }
                        }
                    }
                }
            }
        }
        // `TOUCHHLE_DS_UV_WATCH="u0,u1,v0,v1[;...]"` is the mirror image of the
        // point probe: log every triangle whose texture-coordinate box lands
        // on a watched atlas rectangle, together with where it was drawn. Use
        // it to answer "is this atlas widget rasterised at all, and if so
        // where?" - which the per-draw dumps cannot, since a draw batches
        // several quads and the index buffer is not logged.
        if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_UV_WATCH") {
            let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
            let from = crate::ds_draw_env::var("TOUCHHLE_DS_UV_WATCH_FROM_FRAME")
                .ok()
                .and_then(|v| v.parse::<u64>().ok());
            if from.map_or(true, |f| frame >= f) {
                let mut rects = [0.0f32; 16];
                let mut nrects = 0usize;
                for quad in spec.split(';') {
                    let vals: Vec<f32> = quad
                        .split(',')
                        .filter_map(|v| v.trim().parse::<f32>().ok())
                        .collect();
                    if vals.len() == 4 && nrects < 4 {
                        for k in 0..4 {
                            rects[nrects * 4 + k] = vals[k];
                        }
                        nrects += 1;
                    }
                }
                let mut px = [0.0f32; 1024];
                let mut py = [0.0f32; 1024];
                let mut puv = [0.0f32; 2048];
                let mut nv = 0usize;
                let mut usable = true;
                for i in 0..vertex_count {
                    let ch = &vertices[i * 4..i * 4 + 4];
                    let (x, y, z) = (ch[0], ch[1], ch[2]);
                    let w = if vertex_size >= 4 { ch[3] } else { 1.0 };
                    let mv = modelview_matrix;
                    let pj = projection_matrix;
                    let ex = mv[0] * x + mv[4] * y + mv[8] * z + mv[12] * w;
                    let ey = mv[1] * x + mv[5] * y + mv[9] * z + mv[13] * w;
                    let ez = mv[2] * x + mv[6] * y + mv[10] * z + mv[14] * w;
                    let ew = mv[3] * x + mv[7] * y + mv[11] * z + mv[15] * w;
                    let cx = pj[0] * ex + pj[4] * ey + pj[8] * ez + pj[12] * ew;
                    let cy = pj[1] * ex + pj[5] * ey + pj[9] * ez + pj[13] * ew;
                    let cw = pj[3] * ex + pj[7] * ey + pj[11] * ez + pj[15] * ew;
                    if cw == 0.0 {
                        usable = false;
                        break;
                    }
                    px[nv] = ((cy / cw) + 1.0) * 0.5 * 480.0;
                    py[nv] = ((cx / cw) + 1.0) * 0.5 * 320.0;
                    puv[nv * 2] = uv[i * 4];
                    puv[nv * 2 + 1] = uv[i * 4 + 1];
                    nv += 1;
                    if nv >= 1024 {
                        break;
                    }
                }
                if usable && nv >= 3 && nrects > 0 {
                    let strip = mode == 0x5;
                    let mut order: Vec<usize> = Vec::with_capacity(nv);
                    if let Some((ptr, ity)) = indices {
                        if !ptr.is_null() {
                            for i in 0..count.min(4096) as usize {
                                let idx = match ity {
                                    0x1401 => *(ptr as *const u8).add(i) as usize,
                                    0x1403 => *(ptr as *const u16).add(i) as usize,
                                    _ => {
                                        order.clear();
                                        break;
                                    }
                                };
                                if idx < nv {
                                    order.push(idx);
                                } else {
                                    order.clear();
                                    break;
                                }
                            }
                        }
                    }
                    if order.is_empty() {
                        order = (0..nv).collect();
                    }
                    let m = order.len();
                    let tris: Vec<[usize; 3]> = if strip {
                        (0..m.saturating_sub(2))
                            .map(|i| [order[i], order[i + 1], order[i + 2]])
                            .collect()
                    } else {
                        (0..(m / 3))
                            .map(|i| [order[i * 3], order[i * 3 + 1], order[i * 3 + 2]])
                            .collect()
                    };
                    for (ti, tri) in tris.iter().enumerate() {
                        let us = [puv[tri[0] * 2], puv[tri[1] * 2], puv[tri[2] * 2]];
                        let vs = [
                            puv[tri[0] * 2 + 1],
                            puv[tri[1] * 2 + 1],
                            puv[tri[2] * 2 + 1],
                        ];
                        let (umin, umax) = (
                            us.iter().cloned().fold(f32::MAX, f32::min),
                            us.iter().cloned().fold(f32::MIN, f32::max),
                        );
                        let (vmin, vmax) = (
                            vs.iter().cloned().fold(f32::MAX, f32::min),
                            vs.iter().cloned().fold(f32::MIN, f32::max),
                        );
                        for r in 0..nrects {
                            let (ru0, ru1, rv0, rv1) = (
                                rects[r * 4],
                                rects[r * 4 + 1],
                                rects[r * 4 + 2],
                                rects[r * 4 + 3],
                            );
                            if umin <= ru1 && umax >= ru0 && vmin <= rv1 && vmax >= rv0 {
                                log!(
                                    "[DSTEXuw] f={} #{} rect=({:.3},{:.3},{:.3},{:.3}) draw={} tri={} tex={} {}x{} uv=[{:.4},{:.4} | {:.4},{:.4} | {:.4},{:.4}] pt=[{:.1},{:.1} | {:.1},{:.1} | {:.1},{:.1}]",
                                    frame, n, ru0, ru1, rv0, rv1, kind, ti, texture, tex_w, tex_h,
                                    us[0], vs[0], us[1], vs[1], us[2], vs[2],
                                    px[tri[0]], py[tri[0]],
                                    px[tri[1]], py[tri[1]],
                                    px[tri[2]], py[tri[2]]
                                );
                            }
                        }
                    }
                }
            }
        }
        // Terse mode (TOUCHHLE_DS_TRACE_TERSE): one small line per draw with
        // the draw state plus the NDC bounding box of the submitted vertices,
        // so a long capture stays small and each draw maps to a screen region.
        if dedup_ok && crate::ds_draw_env::contains("TOUCHHLE_DS_TRACE_TERSE") {
            let mut nmin = [f32::MAX; 3];
            let mut nmax = [f32::MIN; 3];
            let mv = modelview_matrix;
            let pj = projection_matrix;
            for ch in vertices.chunks_exact(4) {
                // A position array with fewer than 4 components leaves the
                // unused slots at 0, so reading `ch[3]` as w gives w = 0 and
                // the homogeneous divide degenerates: under an orthographic
                // projection every clip w comes out 0 and all vertices are
                // skipped, leaving nmin/nmax at f32::MAX/MIN. Measured on
                // `[DSTEX draw]` captures: with w = 0 only 64/3959 draws got a
                // usable box, with w = 1 it is 3881/3959. Default to 1.
                let (x, y, z) = (ch[0], ch[1], ch[2]);
                let w = if vertex_size >= 4 { ch[3] } else { 1.0 };
                let ex = mv[0] * x + mv[4] * y + mv[8] * z + mv[12] * w;
                let ey = mv[1] * x + mv[5] * y + mv[9] * z + mv[13] * w;
                let ez = mv[2] * x + mv[6] * y + mv[10] * z + mv[14] * w;
                let ew = mv[3] * x + mv[7] * y + mv[11] * z + mv[15] * w;
                let cx = pj[0] * ex + pj[4] * ey + pj[8] * ez + pj[12] * ew;
                let cy = pj[1] * ex + pj[5] * ey + pj[9] * ez + pj[13] * ew;
                let cz = pj[2] * ex + pj[6] * ey + pj[10] * ez + pj[14] * ew;
                let cw = pj[3] * ex + pj[7] * ey + pj[11] * ez + pj[15] * ew;
                if cw == 0.0 {
                    continue;
                }
                let v3 = [cx / cw, cy / cw, cz / cw];
                for k in 0..3 {
                    if v3[k] < nmin[k] {
                        nmin[k] = v3[k];
                    }
                    if v3[k] > nmax[k] {
                        nmax[k] = v3[k];
                    }
                }
            }
            // Texture-coordinate range of the submitted vertices. The in-game
            // HUD is drawn from one 1024x1024 atlas (engine key a0e3c9b5) in
            // small quads, so a narrow UV rectangle plus the NDC box is what
            // pins a HUD element down; without it a terse capture cannot tell
            // the locator ring from the rest of the strip.
            let mut uvmin = [f32::MAX; 2];
            let mut uvmax = [f32::MIN; 2];
            if tc_size >= 2 {
                for i in 0..vertex_count {
                    for k in 0..2 {
                        let v = uv[i * 4 + k];
                        if v < uvmin[k] {
                            uvmin[k] = v;
                        }
                        if v > uvmax[k] {
                            uvmax[k] = v;
                        }
                    }
                }
            }
            // Wrap/filter are not part of the usual state dump; query them here so
            // a capture records what the guest actually asked for.
            let mut wrap_s: GLint = 0;
            let mut wrap_t: GLint = 0;
            let mut min_filter: GLint = 0;
            let mut mag_filter: GLint = 0;
            gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_WRAP_S, &mut wrap_s);
            gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_WRAP_T, &mut wrap_t);
            gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_MIN_FILTER, &mut min_filter);
            gl21::GetTexParameteriv(gl21::TEXTURE_2D, gl21::TEXTURE_MAG_FILTER, &mut mag_filter);
            // Whether a mip level 1 exists; a texture whose min filter is a mipmap
            // mode but which only has level 0 is *incomplete* and samples as
            // opaque black per the GL spec. Level 0 widths are also reported so
            // a 0/0 pair means "no texture bound" rather than "no mip chain".
            let mut lv0_w: GLint = 0;
            let mut lv1_w: GLint = 0;
            gl21::GetTexLevelParameteriv(gl21::TEXTURE_2D, 0, gl21::TEXTURE_WIDTH, &mut lv0_w);
            gl21::GetTexLevelParameteriv(gl21::TEXTURE_2D, 1, gl21::TEXTURE_WIDTH, &mut lv1_w);
            log!(
                // `f=` is the presented-frame counter, the same clock the
                // frame dumper names its files with, so a terse trace and a
                // frame dump from the same run can be lined up directly.
                "[DSTEXt] #{} f={} {} mode=0x{:x} cnt={} tex={} {}x{} t2d={} cull={} blend={}/{}/{} at={}/0x{:x}/{} depth={}/{}/{} col={:?} nmin=[{:.3},{:.3},{:.3}] nmax=[{:.3},{:.3},{:.3}] uv=[{:.3},{:.3}..{:.3},{:.3}] wrap=0x{:x}/0x{:x} filt=0x{:x}/0x{:x} lv0={} lv1={}",
                n,
                crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed),
                kind, mode, count, texture, tex_w, tex_h,
                texture_2d_enabled, cull_enabled,
                blend_enabled, blend_src, blend_dst,
                alpha_test_enabled, alpha_func, alpha_ref,
                depth_test_enabled, depth_func, depth_mask,
                &current_color,
                nmin[0], nmin[1], nmin[2], nmax[0], nmax[1], nmax[2],
                uvmin[0], uvmin[1], uvmax[0], uvmax[1],
                wrap_s, wrap_t, min_filter, mag_filter, lv0_w, lv1_w
            );
            return;
        }
        if !dedup_ok {
            return;
        }

        log!(
            "[DSTEX draw] #{} kind={} mode=0x{:x} first={} count={} tex={} size={}x{} active=0x{:x} client=0x{:x} tex2d={} cull={} blend={}/{}/{} alpha={}/0x{:x}/{:.2} depth={}/{}/{} texenv=0x{:x} color={:?} scissor={}/{:?} viewport={:?} colmask={:?} texmat={} modelview={} projection={} tc_enabled={} tc_size={} tc_type=0x{:x} tc_stride={} tc_buffer={} vertex_size={} vertex_type=0x{:x} vertex_stride={} vertex_buffer={} vertices={:?} uv={:?}",
            n,
            kind,
            mode,
            first,
            count,
            texture,
            tex_w,
            tex_h,
            active_unit,
            client_unit,
            texture_2d_enabled,
            cull_enabled,
            blend_enabled,
            blend_src,
            blend_dst,
            alpha_test_enabled,
            alpha_func,
            alpha_ref,
            depth_test_enabled,
            depth_func,
            depth_mask,
            tex_env_mode,
            &current_color,
            scissor_enabled,
            &scissor_box,
            &viewport,
            &color_mask,
            tex_matrix_debug,
            modelview_matrix_debug,
            projection_matrix_debug,
            gl21::IsEnabled(gl21::TEXTURE_COORD_ARRAY) != 0,
            tc_size,
            tc_type,
            tc_stride,
            tc_buffer,
            vertex_size,
            vertex_type,
            vertex_stride,
            vertex_buffer,
            &vertices[..vertex_count * 4],
            &uv[..vertex_count * 4]
        );
        ds_mv_trace_log(format_args!(
            "DRAW #{} tex={} size={}x{} count={}",
            n, texture, tex_w, tex_h, count
        ));
    }

    /// If any arrays with fixed-point data are in use at the time of a draw
    /// call, this function will convert the data to floating-point and
    /// replace the pointers. [Self::restore_fixed_point_arrays] can be called
    /// after to restore the original state.
    unsafe fn translate_fixed_point_arrays(
        &mut self,
        first: GLint,
        count: GLsizei,
    ) -> [Option<ArrayStateBackup>; ARRAYS.len()] {
        let mut backups: [Option<ArrayStateBackup>; ARRAYS.len()] = Default::default();
        for (i, array_info) in ARRAYS.iter().enumerate() {
            // Decide whether we need to do anything for this array

            if !self.state.pointer_is_fixed_point[i] {
                continue;
            }

            // There is one texture co-ordinates pointer per texture unit.
            let old_client_active_texture = if array_info.name == gl21::TEXTURE_COORD_ARRAY {
                // Is the texture unit involved in this draw call fixed-point?
                // If not, we don't need to do anything.
                let mut active_texture: GLenum = 0;
                gl21::GetIntegerv(
                    gl21::ACTIVE_TEXTURE,
                    &mut active_texture as *mut _ as *mut _,
                );
                if !self
                    .state
                    .fixed_point_texture_units
                    .contains(&active_texture)
                {
                    continue;
                }

                // Make sure our glTexCoordPointer call will affect that unit.
                let mut old_client_active_texture: GLenum = 0;
                gl21::GetIntegerv(
                    gl21::CLIENT_ACTIVE_TEXTURE,
                    &mut old_client_active_texture as *mut _ as *mut _,
                );
                gl21::ClientActiveTexture(active_texture);
                Some(old_client_active_texture)
            } else {
                None
            };

            let mut is_active = gl21::FALSE;
            gl21::GetBooleanv(array_info.name, &mut is_active);
            if is_active != gl21::TRUE {
                continue;
            }

            let mut buffer_binding = 0;
            gl21::GetIntegerv(array_info.buffer_binding, &mut buffer_binding);

            // Get and back up data

            let size = array_info.size.map(|size_enum| {
                let mut size: GLint = 0;
                gl21::GetIntegerv(size_enum, &mut size);
                size
            });
            let mut stride: GLsizei = 0;
            gl21::GetIntegerv(array_info.stride, &mut stride);
            let old_pointer = {
                let mut pointer: *mut GLvoid = std::ptr::null_mut();
                // The second argument to glGetPointerv must be a mutable
                // pointer, but gl_generator generates the wrong signature
                // by mistake, see https://github.com/brendanzab/gl-rs/issues/541
                #[allow(clippy::unnecessary_mut_passed)]
                gl21::GetPointerv(array_info.pointer, &mut pointer);
                pointer.cast_const()
            };

            backups[i] = Some(ArrayStateBackup {
                size,
                stride,
                pointer: old_pointer,
                buffer_binding: buffer_binding.try_into().unwrap(),
            });

            let pointer = if buffer_binding != 0 {
                let mapped_buffer = gl21::MapBuffer(gl21::ARRAY_BUFFER, gl21::READ_ONLY);
                assert!(!mapped_buffer.is_null());
                // in this case the old_pointer is actually an offest!
                mapped_buffer.add(old_pointer as usize)
            } else {
                old_pointer
            };

            // Create translated array and substitute pointer

            let size = size.unwrap_or_else(|| {
                assert!(array_info.name == gl21::NORMAL_ARRAY);
                3
            });
            let stride = if stride == 0 {
                // tightly packed mode
                size * 4 // sizeof(gl::FLOAT)
            } else {
                stride
            };

            let buffer = &mut self.state.fixed_point_translation_buffers[i];
            buffer.clear();
            buffer.resize(((first + count) * size).try_into().unwrap(), 0.0);

            {
                assert!(first >= 0 && count >= 0 && size >= 0 && stride >= 0);
                let first = first as usize;
                let count = count as usize;
                let size = size as usize;
                let stride = stride as usize;
                for j in first..(first + count) {
                    let vector_ptr: *const GLvoid = pointer.add(j * stride);
                    let vector_ptr: *const GLfixed = vector_ptr.cast();
                    for k in 0..size {
                        buffer[j * size + k] = fixed_to_float(vector_ptr.add(k).read_unaligned());
                    }
                }
            }

            if buffer_binding != 0 {
                gl21::UnmapBuffer(gl21::ARRAY_BUFFER);
                gl21::BindBuffer(gl21::ARRAY_BUFFER, 0);
            }

            let buffer_ptr: *const GLfloat = buffer.as_ptr();
            let buffer_ptr: *const GLvoid = buffer_ptr.cast();
            match array_info.name {
                gl21::COLOR_ARRAY => gl21::ColorPointer(size, gl21::FLOAT, 0, buffer_ptr),
                gl21::NORMAL_ARRAY => {
                    assert!(size == 3);
                    gl21::NormalPointer(gl21::FLOAT, 0, buffer_ptr)
                }
                gl21::TEXTURE_COORD_ARRAY => {
                    gl21::TexCoordPointer(size, gl21::FLOAT, 0, buffer_ptr)
                }
                gl21::VERTEX_ARRAY => gl21::VertexPointer(size, gl21::FLOAT, 0, buffer_ptr),
                _ => unreachable!(),
            }

            if let Some(old_client_active_texture) = old_client_active_texture {
                gl21::ClientActiveTexture(old_client_active_texture);
            }
        }
        backups
    }
    unsafe fn restore_fixed_point_arrays(
        &mut self,
        from_backup: [Option<ArrayStateBackup>; ARRAYS.len()],
    ) {
        for (i, backup) in from_backup.into_iter().enumerate() {
            let array_info = &ARRAYS[i];
            let Some(ArrayStateBackup {
                size,
                stride,
                pointer,
                buffer_binding,
            }) = backup
            else {
                continue;
            };

            if buffer_binding != 0 {
                gl21::BindBuffer(gl21::ARRAY_BUFFER, buffer_binding);
            }

            match array_info.name {
                gl21::COLOR_ARRAY => {
                    gl21::ColorPointer(size.unwrap(), gl21::FLOAT, stride, pointer)
                }
                gl21::NORMAL_ARRAY => {
                    assert!(size.is_none());
                    gl21::NormalPointer(gl21::FLOAT, stride, pointer)
                }
                gl21::TEXTURE_COORD_ARRAY => {
                    let mut active_texture: GLenum = 0;
                    gl21::GetIntegerv(
                        gl21::ACTIVE_TEXTURE,
                        &mut active_texture as *mut _ as *mut _,
                    );
                    assert!(self
                        .state
                        .fixed_point_texture_units
                        .contains(&active_texture));
                    let mut old_client_active_texture: GLenum = 0;
                    gl21::GetIntegerv(
                        gl21::CLIENT_ACTIVE_TEXTURE,
                        &mut old_client_active_texture as *mut _ as *mut _,
                    );
                    gl21::ClientActiveTexture(active_texture);
                    gl21::TexCoordPointer(size.unwrap(), gl21::FLOAT, stride, pointer);
                    gl21::ClientActiveTexture(old_client_active_texture)
                }
                gl21::VERTEX_ARRAY => {
                    gl21::VertexPointer(size.unwrap(), gl21::FLOAT, stride, pointer)
                }
                _ => unreachable!(),
            }
        }
    }

    /// Returns `true` if `GL_OES_matrix_palette` skinning is active for the
    /// current draw: the palette is enabled and both the weight and matrix
    /// index client arrays are enabled. When this is false the regular
    /// fixed-function path handles the draw.
    unsafe fn matrix_palette_active(&self) -> bool {
        self.state.matrix_palette_enabled
            && self.state.palette_weight_state.enabled
            && self.state.palette_index_state.enabled
    }

    /// Read one element of a client/array-buffer-backed pointer array as up to
    /// 4 `f32`s, decoding the source type. Used to gather vertex positions,
    /// blend weights and matrix indices for CPU skinning.
    ///
    /// `base` already points at element 0 in host memory (offset/buffer
    /// translation resolved by the caller). `stride` of 0 means tightly
    /// packed (`size * element_size`).
    unsafe fn read_array_element_f32(
        base: *const u8,
        index: usize,
        size: usize,
        type_: GLenum,
        stride: usize,
        out: &mut [GLfloat; 4],
    ) {
        let element_size = match type_ {
            gl21::BYTE | gl21::UNSIGNED_BYTE => 1usize,
            gl21::SHORT | gl21::UNSIGNED_SHORT => 2,
            gl21::FLOAT | gles11::FIXED => 4,
            _ => 4,
        };
        let effective_stride = if stride == 0 {
            size * element_size
        } else {
            stride
        };
        let elem_ptr = base.add(index * effective_stride);
        for c in 0..size {
            let comp_ptr = elem_ptr.add(c * element_size);
            out[c] = match type_ {
                gl21::FLOAT => (comp_ptr as *const GLfloat).read_unaligned(),
                gles11::FIXED => fixed_to_float((comp_ptr as *const GLfixed).read_unaligned()),
                gl21::UNSIGNED_BYTE => (comp_ptr as *const u8).read_unaligned() as GLfloat,
                gl21::BYTE => (comp_ptr as *const i8).read_unaligned() as GLfloat,
                gl21::UNSIGNED_SHORT => (comp_ptr as *const u16).read_unaligned() as GLfloat,
                gl21::SHORT => (comp_ptr as *const i16).read_unaligned() as GLfloat,
                _ => 0.0,
            };
        }
    }

    /// Resolve a client array's element-0 host pointer, mapping the bound
    /// `GL_ARRAY_BUFFER` if `buffer_binding` is non-zero. Returns the raw host
    /// pointer and, when a buffer was mapped, `true` so the caller can unmap.
    unsafe fn resolve_array_base(
        pointer: *const GLvoid,
        buffer_binding: GLuint,
    ) -> (*const u8, bool) {
        if buffer_binding != 0 {
            gl21::BindBuffer(gl21::ARRAY_BUFFER, buffer_binding);
            let mapped = gl21::MapBuffer(gl21::ARRAY_BUFFER, gl21::READ_ONLY) as *const u8;
            if mapped.is_null() {
                gl21::BindBuffer(gl21::ARRAY_BUFFER, 0);
                return (std::ptr::null(), false);
            }
            (mapped.add(pointer as usize), true)
        } else {
            (pointer as *const u8, false)
        }
    }

    /// Perform `GL_OES_matrix_palette` skinning on the CPU for vertex indices
    /// `[first, first + count)` and submit the transformed positions.
    ///
    /// Desktop GL 2.1 has no fixed-function palette skinning and Mesa does not
    /// expose `GL_ARB_matrix_palette`/`GL_ARB_vertex_blend`, so we compute eye
    /// coordinates ourselves following the OES_matrix_palette spec:
    ///
    /// ```text
    ///   eye = sum_i  w_i * (palette[index_i] * object)
    /// ```
    ///
    /// The blended positions are uploaded as a temporary `GL_FLOAT` vertex
    /// array and drawn with an identity MODELVIEW (the palette matrices are
    /// expected to already include the model-view transform, as set via
    /// glLoadPaletteFromModelViewMatrixOES / glLoadMatrix in palette mode).
    /// Returns `false` if skinning could not be performed (caller should fall
    /// back to a normal draw).
    unsafe fn skin_vertices(&mut self, first: GLint, count: GLsizei) -> Option<Vec<GLfloat>> {
        let mut skin_profile = crate::ds_gl_profile::begin(crate::ds_gl_profile::Kind::Skin);
        if first < 0 || count <= 0 {
            return None;
        }
        let first = first as usize;
        let count = count as usize;
        let vertex_count = first + count;

        // Gather the bound vertex (position) array via desktop GL queries.
        let mut vertex_enabled: GLboolean = 0;
        gl21::GetBooleanv(gl21::VERTEX_ARRAY, &mut vertex_enabled);
        if vertex_enabled != gl21::TRUE {
            return None;
        }
        let mut vertex_size: GLint = 0;
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_SIZE, &mut vertex_size);
        let mut vertex_type: GLint = 0;
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_TYPE, &mut vertex_type);
        let mut vertex_stride: GLint = 0;
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_STRIDE, &mut vertex_stride);
        let mut vertex_buffer_binding: GLint = 0;
        gl21::GetIntegerv(
            gl21::VERTEX_ARRAY_BUFFER_BINDING,
            &mut vertex_buffer_binding,
        );
        let mut vertex_pointer: *mut GLvoid = std::ptr::null_mut();
        #[allow(clippy::unnecessary_mut_passed)]
        gl21::GetPointerv(gl21::VERTEX_ARRAY_POINTER, &mut vertex_pointer);

        let vertex_size = vertex_size.clamp(2, 4) as usize;
        let weight = &self.state.palette_weight_state;
        let index = &self.state.palette_index_state;
        let weight_size = weight.size.clamp(1, MATRIX_PALETTE_MAX_UNITS as GLint) as usize;
        let index_size = index.size.clamp(1, MATRIX_PALETTE_MAX_UNITS as GLint) as usize;
        let units = weight_size.min(index_size);
        if units == 0 {
            return None;
        }

        if let Some(timer) = skin_profile.as_mut() {
            timer.skin_layout([
                vertex_size as u32, vertex_type as u32, vertex_stride as u32,
                weight_size as u32, weight.type_, weight.stride as u32,
                index_size as u32, index.type_, index.stride as u32,
                (vertex_buffer_binding != 0) as u32 | (((weight.buffer_binding != 0) as u32) << 1) | (((index.buffer_binding != 0) as u32) << 2),
                units as u32,
            ], count, vertex_count * vertex_size);
        }

        // Resolve all three source arrays to host pointers.
        let (vertex_base, vertex_mapped) =
            Self::resolve_array_base(vertex_pointer.cast_const(), vertex_buffer_binding as GLuint);
        if vertex_base.is_null() {
            return None;
        }
        let (weight_base, weight_mapped) =
            Self::resolve_array_base(weight.pointer, weight.buffer_binding);
        let (index_base, index_mapped) =
            Self::resolve_array_base(index.pointer, index.buffer_binding);
        if weight_base.is_null() || index_base.is_null() {
            if vertex_mapped {
                gl21::BindBuffer(gl21::ARRAY_BUFFER, vertex_buffer_binding as GLuint);
                gl21::UnmapBuffer(gl21::ARRAY_BUFFER);
                gl21::BindBuffer(gl21::ARRAY_BUFFER, 0);
            }
            return None;
        }

        let palette_count = self.state.palette_matrices.len();
        if let Some(timer) = skin_profile.as_mut() { timer.math_begin(); }
        let mut out: Vec<GLfloat> = vec![0.0; vertex_count * vertex_size];

        for v in first..vertex_count {
            let mut object = [0.0f32, 0.0, 0.0, 1.0];
            Self::read_array_element_f32(
                vertex_base,
                v,
                vertex_size,
                vertex_type as GLenum,
                weight_stride_or(vertex_stride),
                &mut object,
            );

            let mut weights = [0.0f32; 4];
            Self::read_array_element_f32(
                weight_base,
                v,
                weight_size,
                weight.type_,
                weight.stride as usize,
                &mut weights,
            );
            let mut indices = [0.0f32; 4];
            Self::read_array_element_f32(
                index_base,
                v,
                index_size,
                index.type_,
                index.stride as usize,
                &mut indices,
            );

            let mut blended = [0.0f32; 4];
            let mut weight_sum = 0.0f32;
            for u in 0..units {
                let w = weights[u];
                if w == 0.0 {
                    continue;
                }
                weight_sum += w;
                let mat_index = (indices[u] as i64).clamp(0, (palette_count as i64) - 1) as usize;
                let transformed = mat4_transform(&self.state.palette_matrices[mat_index], object);
                for c in 0..4 {
                    blended[c] += w * transformed[c];
                }
            }
            // If the guest supplied no usable weights, fall back to the
            // untransformed position so the vertex is still placed sensibly.
            if weight_sum == 0.0 {
                blended = object;
            }

            // Dead Space debug aid: `TOUCHHLE_DS_LOG_SKIN=1` reports the first
            // skinned vertex of each draw, including the blended W component,
            // which is what makes the fog coordinate degenerate if it is zero.
            if v == first && crate::ds_draw_env::contains("TOUCHHLE_DS_LOG_SKIN") {
                use std::sync::atomic::{AtomicUsize, Ordering as AOrd2};
                static SKN: AtomicUsize = AtomicUsize::new(0);
                if SKN.fetch_add(1, AOrd2::Relaxed) < 40 {
                    log!(
                        "[DSskin] obj={:?} blended={:?} units={} vsize={} wsum={}",
                        object,
                        blended,
                        units,
                        vertex_size,
                        weight_sum
                    );
                }
            }
            for c in 0..vertex_size {
                out[v * vertex_size + c] = blended[c];
            }
        }
        if let Some(timer) = skin_profile.as_mut() { timer.math_end(); }

        if vertex_mapped {
            gl21::BindBuffer(gl21::ARRAY_BUFFER, vertex_buffer_binding as GLuint);
            gl21::UnmapBuffer(gl21::ARRAY_BUFFER);
            gl21::BindBuffer(gl21::ARRAY_BUFFER, 0);
        }
        if weight_mapped {
            gl21::BindBuffer(gl21::ARRAY_BUFFER, weight.buffer_binding);
            gl21::UnmapBuffer(gl21::ARRAY_BUFFER);
            gl21::BindBuffer(gl21::ARRAY_BUFFER, 0);
        }
        if index_mapped {
            gl21::BindBuffer(gl21::ARRAY_BUFFER, index.buffer_binding);
            gl21::UnmapBuffer(gl21::ARRAY_BUFFER);
            gl21::BindBuffer(gl21::ARRAY_BUFFER, 0);
        }

        Some(out)
    }

    /// Scan an index buffer to find the `[min, max]` vertex range referenced,
    /// returning `(first, count)` so the caller can skin exactly that range.
    /// Mirrors the range scan already used for fixed-point translation.
    unsafe fn indexed_draw_vertex_range(
        &mut self,
        count: GLsizei,
        type_: GLenum,
        indices: *const GLvoid,
    ) -> Option<(GLint, GLsizei)> {
        let _gl_profile = crate::ds_gl_profile::begin(crate::ds_gl_profile::Kind::IndexRange);
        if count <= 0 {
            return None;
        }
        let mut index_buffer_binding = 0;
        gl21::GetIntegerv(
            gl21::ELEMENT_ARRAY_BUFFER_BINDING,
            &mut index_buffer_binding,
        );
        let base = if index_buffer_binding != 0 {
            let mapped = gl21::MapBuffer(gl21::ELEMENT_ARRAY_BUFFER, gl21::READ_ONLY);
            if mapped.is_null() {
                return None;
            }
            mapped.add(indices as usize)
        } else {
            indices
        };

        let mut first = usize::MAX;
        let mut last = usize::MIN;
        match type_ {
            gl21::UNSIGNED_BYTE => {
                let p: *const GLubyte = base.cast();
                for i in 0..(count as usize) {
                    let idx = p.add(i).read_unaligned() as usize;
                    first = first.min(idx);
                    last = last.max(idx);
                }
            }
            gl21::UNSIGNED_SHORT => {
                let p: *const GLushort = base.cast();
                for i in 0..(count as usize) {
                    let idx = p.add(i).read_unaligned() as usize;
                    first = first.min(idx);
                    last = last.max(idx);
                }
            }
            _ => {
                if index_buffer_binding != 0 {
                    gl21::UnmapBuffer(gl21::ELEMENT_ARRAY_BUFFER);
                }
                return None;
            }
        }
        if index_buffer_binding != 0 {
            gl21::UnmapBuffer(gl21::ELEMENT_ARRAY_BUFFER);
        }
        if first == usize::MAX {
            return None;
        }
        Some((first as GLint, (last + 1 - first) as GLsizei))
    }

    /// Temporarily install the CPU-skinned positions as the active vertex
    /// array, reset the MODELVIEW to identity (the palette matrices already
    /// carry the model-view transform per the OES spec), run `body`, then
    /// restore the previous vertex pointer and MODELVIEW matrix.
    unsafe fn with_skinned_vertex_array(
        &mut self,
        skinned: &[GLfloat],
        vertex_size: usize,
        body: impl FnOnce(&mut gpu_profile::State),
    ) {
        let _gl_profile = crate::ds_gl_profile::begin(crate::ds_gl_profile::Kind::SkinSubmit);
        // Back up the vertex array pointer/format so we can restore it.
        let mut old_size: GLint = 0;
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_SIZE, &mut old_size);
        let mut old_type: GLint = 0;
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_TYPE, &mut old_type);
        let mut old_stride: GLint = 0;
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_STRIDE, &mut old_stride);
        let mut old_buffer: GLint = 0;
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_BUFFER_BINDING, &mut old_buffer);
        let mut old_pointer: *mut GLvoid = std::ptr::null_mut();
        #[allow(clippy::unnecessary_mut_passed)]
        gl21::GetPointerv(gl21::VERTEX_ARRAY_POINTER, &mut old_pointer);

        // Skinned positions are client-side floats: unbind any array buffer.
        gl21::BindBuffer(gl21::ARRAY_BUFFER, 0);
        gl21::VertexPointer(
            vertex_size as GLint,
            gl21::FLOAT,
            0,
            skinned.as_ptr().cast(),
        );

        // Palette matrices already include the model-view transform, so draw
        // with an identity MODELVIEW. Save & restore the real one.
        let mut old_matrix_mode: GLint = 0;
        gl21::GetIntegerv(gl21::MATRIX_MODE, &mut old_matrix_mode);
        gl21::MatrixMode(gl21::MODELVIEW);
        gl21::PushMatrix();
        gl21::LoadIdentity();

        body(&mut self.state.gpu_profile);

        gl21::PopMatrix();
        gl21::MatrixMode(old_matrix_mode as GLenum);

        // Restore the previous vertex array binding/pointer.
        gl21::BindBuffer(gl21::ARRAY_BUFFER, old_buffer as GLuint);
        gl21::VertexPointer(
            old_size,
            old_type as GLenum,
            old_stride,
            old_pointer.cast_const(),
        );
        gl21::BindBuffer(gl21::ARRAY_BUFFER, 0);
    }

    unsafe fn draw_blood_scaled_elements(
        &mut self,
        count: GLsizei,
        type_: GLenum,
        indices: *const GLvoid,
    ) -> bool {
        let scale = crate::ds_draw_env::var("TOUCHHLE_DS_BLOOD_SIZE")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value > 0.0 && *value <= 1.0)
            .unwrap_or(0.5);
        if scale == 1.0 {
            return false;
        }
        let element_size = match type_ {
            gl21::UNSIGNED_BYTE => 1,
            gl21::UNSIGNED_SHORT => 2,
            _ => return false,
        };
        let mut element_buffer = 0;
        gl21::GetIntegerv(
            gl21::ELEMENT_ARRAY_BUFFER_BINDING,
            &mut element_buffer,
        );
        let mut index_data = vec![0u8; count as usize * element_size];
        if element_buffer != 0 {
            let mut buffer_size = 0;
            gl21::GetBufferParameteriv(
                gl21::ELEMENT_ARRAY_BUFFER,
                gl21::BUFFER_SIZE,
                &mut buffer_size,
            );
            if (indices as usize)
                .checked_add(index_data.len())
                .is_none_or(|end| end > buffer_size.max(0) as usize)
            {
                return false;
            }
            gl21::GetBufferSubData(
                gl21::ELEMENT_ARRAY_BUFFER,
                indices as GLintptr,
                index_data.len() as GLsizeiptr,
                index_data.as_mut_ptr().cast(),
            );
        } else if indices.is_null() {
            return false;
        } else {
            std::ptr::copy_nonoverlapping(
                indices.cast::<u8>(),
                index_data.as_mut_ptr(),
                index_data.len(),
            );
        }
        let order: Vec<usize> = index_data
            .chunks_exact(element_size)
            .map(|bytes| {
                if element_size == 1 {
                    bytes[0] as usize
                } else {
                    u16::from_ne_bytes([bytes[0], bytes[1]]) as usize
                }
            })
            .collect();
        let vertex_count = order.iter().copied().max().unwrap_or(0) + 1;
        let mut size = 0;
        let mut vertex_type = 0;
        let mut stride = 0;
        let mut vertex_buffer = 0;
        let mut array_buffer = 0;
        let mut pointer: *mut GLvoid = std::ptr::null_mut();
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_SIZE, &mut size);
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_TYPE, &mut vertex_type);
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_STRIDE, &mut stride);
        gl21::GetIntegerv(
            gl21::VERTEX_ARRAY_BUFFER_BINDING,
            &mut vertex_buffer,
        );
        gl21::GetIntegerv(gl21::ARRAY_BUFFER_BINDING, &mut array_buffer);
        gl21::GetPointerv(gl21::VERTEX_ARRAY_POINTER, &mut pointer);
        if !(3..=4).contains(&size) || vertex_type != gl21::FLOAT as GLint || stride < 0 {
            log_once!("[DSbloodSize] unsupported vertex format; keeping original size");
            return false;
        }
        let vertex_bytes = size as usize * 4;
        let effective_stride = if stride == 0 {
            vertex_bytes
        } else {
            stride as usize
        };
        let mut raw = Vec::new();
        let base = if vertex_buffer != 0 {
            gl21::BindBuffer(gl21::ARRAY_BUFFER, vertex_buffer as GLuint);
            let mut buffer_size = 0;
            gl21::GetBufferParameteriv(
                gl21::ARRAY_BUFFER,
                gl21::BUFFER_SIZE,
                &mut buffer_size,
            );
            let length = (vertex_count - 1) * effective_stride + vertex_bytes;
            if (pointer as usize)
                .checked_add(length)
                .is_none_or(|end| end > buffer_size.max(0) as usize)
            {
                gl21::BindBuffer(gl21::ARRAY_BUFFER, array_buffer as GLuint);
                return false;
            }
            raw.resize(length, 0u8);
            gl21::GetBufferSubData(
                gl21::ARRAY_BUFFER,
                pointer as GLintptr,
                length as GLsizeiptr,
                raw.as_mut_ptr().cast(),
            );
            gl21::BindBuffer(gl21::ARRAY_BUFFER, array_buffer as GLuint);
            raw.as_ptr()
        } else if pointer.is_null() {
            return false;
        } else {
            pointer.cast::<u8>()
        };
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
                positions[index][axis] =
                    (base.add(index * effective_stride + axis * 4) as *const f32)
                        .read_unaligned()
                        / weight;
            }
        }
        if !ds_blood_size::scale_quads(&mut positions, &order, scale) {
            log_once!("[DSbloodSize] non-quad topology; keeping original size");
            return false;
        }
        for index in 0..vertex_count {
            for axis in 0..3 {
                vertices[index][axis] = positions[index][axis] * vertices[index][3];
            }
        }
        gl21::BindBuffer(gl21::ARRAY_BUFFER, 0);
        gl21::VertexPointer(4, gl21::FLOAT, 0, vertices.as_ptr().cast());
        self.state.gpu_profile.begin(0);
        gl21::DrawElements(gl21::TRIANGLES, count, type_, indices);
        self.state.gpu_profile.end(0);
        gl21::BindBuffer(gl21::ARRAY_BUFFER, vertex_buffer as GLuint);
        gl21::VertexPointer(size, vertex_type as GLenum, stride, pointer);
        gl21::BindBuffer(gl21::ARRAY_BUFFER, array_buffer as GLuint);
        log_once!("[DSbloodSize] scaling blood quads around their own centers (TOUCHHLE_DS_BLOOD_SIZE, default 0.5)");
        true
    }

    unsafe fn draw_arrays_skinned(
        &mut self,
        mode: GLenum,
        first: GLint,
        count: GLsizei,
        skinned: &[GLfloat],
    ) {
        let mut vertex_size: GLint = 0;
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_SIZE, &mut vertex_size);
        let vertex_size = vertex_size.clamp(2, 4) as usize;
        self.with_skinned_vertex_array(skinned, vertex_size, |profile| {
            profile.begin(0);
            gl21::DrawArrays(mode, first, count);
            profile.end(0);
        });
    }

    unsafe fn draw_elements_skinned(
        &mut self,
        mode: GLenum,
        count: GLsizei,
        type_: GLenum,
        indices: *const GLvoid,
        skinned: &[GLfloat],
    ) {
        let mut vertex_size: GLint = 0;
        gl21::GetIntegerv(gl21::VERTEX_ARRAY_SIZE, &mut vertex_size);
        let vertex_size = vertex_size.clamp(2, 4) as usize;
        self.with_skinned_vertex_array(skinned, vertex_size, |profile| {
            profile.begin(0);
            gl21::DrawElements(mode, count, type_, indices);
            profile.end(0);
        });
    }
}

/// `glVertexPointer` etc. treat a stride of 0 as tightly packed; this just
/// forwards the value (the readers handle the 0 case per-array).
fn weight_stride_or(stride: GLint) -> usize {
    stride as usize
}

impl GLES for GLES1OnGL2<'_> {
    unsafe fn gpu_profile_begin(&mut self, stage: usize) {self.state.gpu_profile.begin(stage);}
    unsafe fn gpu_profile_end(&mut self, stage: usize) {self.state.gpu_profile.end(stage);}
    unsafe fn gpu_profile_poll(&mut self) {self.state.gpu_profile.poll();}
    unsafe fn gpu_profile_release(&mut self) {self.state.gpu_profile.release();}
    fn input_prompt_atlas(&mut self) -> Option<&mut Option<GLuint>> {
        Some(&mut self.state.input_prompt_atlas)
    }
    unsafe fn driver_description(&self) -> String {
        let version = CStr::from_ptr(gl21::GetString(gl21::VERSION) as *const _);
        let vendor = CStr::from_ptr(gl21::GetString(gl21::VENDOR) as *const _);
        let renderer = CStr::from_ptr(gl21::GetString(gl21::RENDERER) as *const _);
        // OpenGL's version string is just a number, so let's contextualize it.
        format!(
            "OpenGL {} / {} / {}",
            version.to_string_lossy(),
            vendor.to_string_lossy(),
            renderer.to_string_lossy()
        )
    }
    // Generic state manipulation
    unsafe fn GetError(&mut self) -> GLenum {
        gl21::GetError()
    }
    unsafe fn Enable(&mut self, cap: GLenum) {
        self.state.scene.cap(cap,true);
        if cap == gl21::CULL_FACE && crate::ds_draw_env::contains("TOUCHHLE_DS_FORCE_NO_CULL") {
            gl21::Disable(cap);
            return;
        }
        if ARRAYS.iter().any(|&ArrayInfo { name, .. }| name == cap) {
            log_dbg!("Tolerating glEnable({:#x}) of client state", cap);
        } else if cap == gles11::MATRIX_PALETTE_OES {
            // GL_OES_matrix_palette: enable CPU-side palette skinning. Desktop
            // GL 2.1 has no fixed-function palette skinning and Mesa does not
            // expose GL_ARB_matrix_palette, so we emulate it ourselves at draw
            // time (see draw_with_matrix_palette). Track the flag here and do
            // NOT forward to gl21::Enable (0x8840 is not a valid desktop cap
            // and would raise GL_INVALID_ENUM).
            self.state.matrix_palette_enabled = true;
            return;
        } else if cap == gl21::PERSPECTIVE_CORRECTION_HINT
            || cap == gl21::SMOOTH
            || cap == gl21::FLAT
            || cap == gl21::BLEND_EQUATION
            || cap == gl21::TEXTURE
        {
            log_dbg!("Tolerating glEnable({:#x})", cap);
            // Don't forward shading-model / hint enums to gl21::Enable —
            // they're not valid capabilities and would set GL_INVALID_ENUM.
            return;
        } else if !capability_supported(cap) {
            // Per the GLES 1.1 spec, invalid caps set GL_INVALID_ENUM but
            // must not crash. Apple's driver silently ignores unknown caps,
            // and at least Farm Frenzy passes GL_FLAT (0x1D00) here.
            log!(
                "Warning: Tolerating glEnable({:#x}) of unrecognized capability",
                cap
            );
            return;
        }
        if !self.state.state_cache.suppress(cap, true) {
            gl21::Enable(cap);
        }
    }
    unsafe fn IsEnabled(&mut self, cap: GLenum) -> GLboolean {
        if cap == gles11::MATRIX_PALETTE_OES {
            return if self.state.matrix_palette_enabled {
                gl21::TRUE
            } else {
                gl21::FALSE
            };
        }
        if cap == gles11::MATRIX_INDEX_ARRAY_OES {
            return if self.state.palette_index_state.enabled {
                gl21::TRUE
            } else {
                gl21::FALSE
            };
        }
        if cap == gles11::WEIGHT_ARRAY_OES {
            return if self.state.palette_weight_state.enabled {
                gl21::TRUE
            } else {
                gl21::FALSE
            };
        }
        if !(capability_supported(cap)
            || ARRAYS.iter().any(|&ArrayInfo { name, .. }| name == cap))
        {
            log!(
                "Warning: glIsEnabled({:#x}) of unrecognized capability, returning false",
                cap
            );
            return gl21::FALSE;
        }
        gl21::IsEnabled(cap)
    }
    unsafe fn Disable(&mut self, cap: GLenum) {
        self.state.scene.cap(cap,false);
        if cap == gles11::MATRIX_PALETTE_OES {
            // See Enable: emulated, never forwarded to the desktop driver.
            self.state.matrix_palette_enabled = false;
            return;
        } else if capability_supported(cap) {
            log_dbg!("glDisable{:#x}", cap);
        } else if ARRAYS.iter().any(|&ArrayInfo { name, .. }| name == cap) {
            log_dbg!("Tolerating glDisable({:#x}) of client state", cap);
        } else if UNSUPPORTED_CAPABILITIES.contains(&cap) {
            log_dbg!("Tolerating glDisable({:#x}) of unsupported capability", cap);
        } else if GET_PARAMS.contains(cap) || UNSUPPORTED_GET_PARAMS.contains(cap) {
            log_dbg!("Tolerating glDisable({:#x}) of parameter", cap);
        } else {
            log!(
                "Warning: Tolerating glDisable({:#x}) of unrecognized capability",
                cap
            );
            return;
        }
        if !self.state.state_cache.suppress(cap, false) {
            gl21::Disable(cap);
        }
    }
    unsafe fn ClientActiveTexture(&mut self, texture: GLenum) {
        gl21::ClientActiveTexture(texture);
    }
    unsafe fn EnableClientState(&mut self, array: GLenum) {
        self.state.scene.client(array,true);
        if array == gles11::MATRIX_INDEX_ARRAY_OES {
            self.state.palette_index_state.enabled = true;
            return;
        }
        if array == gles11::WEIGHT_ARRAY_OES {
            self.state.palette_weight_state.enabled = true;
            return;
        }
        if capability_supported(array) {
            log_dbg!(
                "Tolerating glEnableClientState({:#x}) of a capability",
                array
            );
        } else {
            assert!(ARRAYS.iter().any(|&ArrayInfo { name, .. }| name == array));
        }
        gl21::EnableClientState(array);
    }
    unsafe fn DisableClientState(&mut self, array: GLenum) {
        self.state.scene.client(array,false);
        if array == gles11::MATRIX_INDEX_ARRAY_OES {
            self.state.palette_index_state.enabled = false;
            return;
        }
        if array == gles11::WEIGHT_ARRAY_OES {
            self.state.palette_weight_state.enabled = false;
            return;
        }
        if capability_supported(array) {
            log_dbg!(
                "Tolerating glDisableClientState({:#x}) of a capability",
                array
            );
        } else {
            assert!(ARRAYS.iter().any(|&ArrayInfo { name, .. }| name == array));
        }
        gl21::DisableClientState(array);
    }
    unsafe fn GetBooleanv(&mut self, pname: GLenum, params: *mut GLboolean) {
        let (type_, count) = GET_PARAMS.get_type_info(pname);
        let count = usize::from(count.max(1));
        match type_ {
            ParamType::Boolean => {
                gl21::GetBooleanv(pname, params);
            }
            ParamType::Int => {
                // Per the GLES 1.1 spec, any non-zero integer maps to TRUE.
                let mut tmp = [0i32; 16];
                let slice = &mut tmp[..count];
                gl21::GetIntegerv(pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = if v != 0 { gl21::TRUE } else { gl21::FALSE };
                }
            }
            ParamType::Float | ParamType::FloatSpecial => {
                // Any non-zero float maps to TRUE.
                let mut tmp = [0f32; 16];
                let slice = &mut tmp[..count];
                gl21::GetFloatv(pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = if v != 0.0 { gl21::TRUE } else { gl21::FALSE };
                }
            }
            _ => gl21::GetBooleanv(pname, params),
        }
    }
    unsafe fn GetFloatv(&mut self, pname: GLenum, params: *mut GLfloat) {
        let (type_, count) = GET_PARAMS.get_type_info(pname);
        let count = usize::from(count.max(1));
        match type_ {
            ParamType::Float | ParamType::FloatSpecial => {
                gl21::GetFloatv(pname, params);
            }
            ParamType::Int => {
                // Integer state is widened to float verbatim.
                let mut tmp = [0i32; 16];
                let slice = &mut tmp[..count];
                gl21::GetIntegerv(pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = v as GLfloat;
                }
            }
            ParamType::Boolean => {
                let mut tmp = [0u8; 16];
                let slice = &mut tmp[..count];
                gl21::GetBooleanv(pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = if v != 0 { 1.0 } else { 0.0 };
                }
            }
            _ => gl21::GetFloatv(pname, params),
        }
    }
    /// OpenGL ES 1.1 `glGetFixedv`. Desktop GL 2.1 does not have this entry
    /// point, so we route the query to `GetFloatv` / `GetIntegerv` /
    /// `GetBooleanv` based on the underlying parameter type and then convert
    /// each component to the 16.16 fixed-point representation the guest
    /// expects.
    unsafe fn GetFixedv(&mut self, pname: GLenum, params: *mut GLfixed) {
        let (type_, count) = GET_PARAMS.get_type_info(pname);
        let count = usize::from(count.max(1));
        match type_ {
            ParamType::Float | ParamType::FloatSpecial => {
                // OpenGL specifies float-to-fixed conversion as multiplication
                // by 2^16; see the GLES 1.1 spec, "Data Conversions".
                let mut tmp = [0f32; 16];
                let slice = &mut tmp[..count];
                gl21::GetFloatv(pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = float_to_fixed(v);
                }
            }
            ParamType::Boolean => {
                // GL_TRUE / GL_FALSE map to 1 / 0 (no 16.16 scaling).
                let mut tmp = [0u8; 16];
                let slice = &mut tmp[..count];
                gl21::GetBooleanv(pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = if v != 0 { 1 } else { 0 };
                }
            }
            _ => {
                // Integer parameters are copied through verbatim: the GLES 1.1
                // spec says fixed-point queries against integer state must
                // not scale.
                let mut tmp = [0i32; 16];
                let slice = &mut tmp[..count];
                gl21::GetIntegerv(pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = v as GLfixed;
                }
            }
        }
    }
    /// OpenGL ES 1.1 `glGetIntegerv`. Desktop GL implements this entry point,
    /// but only routes integer-typed state through it natively. For
    /// floating-point or boolean state, the GLES 1.1 spec (section 6.1.2,
    /// "Data Conversions") requires that the value be converted to an integer
    /// before being returned. Without explicit conversion, calling the host
    /// `glGetIntegerv` against e.g. a clamp-range float state can return
    /// undefined or always-zero values (and historically this code asserted,
    /// crashing the emulator). We mirror the conversion fan-out used by
    /// `GetFixedv` above: query the underlying state at its native type and
    /// convert each component per the spec.
    unsafe fn GetIntegerv(&mut self, pname: GLenum, params: *mut GLint) {
        // GL_OES_matrix_palette queries: the host desktop driver does not
        // expose GL_ARB_matrix_palette / GL_ARB_vertex_blend (Mesa never
        // implemented them), so answer these from our emulated state instead
        // of forwarding to gl21::GetIntegerv (which would set GL_INVALID_ENUM
        // and leave *params untouched).
        match pname {
            gles11::MAX_PALETTE_MATRICES_OES => {
                *params = self.state.palette_matrices.len() as GLint;
                return;
            }
            gles11::MAX_VERTEX_UNITS_OES => {
                *params = MATRIX_PALETTE_MAX_UNITS as GLint;
                return;
            }
            gles11::CURRENT_PALETTE_MATRIX_OES => {
                *params = self.state.current_palette_matrix as GLint;
                return;
            }
            gles11::MATRIX_INDEX_ARRAY_SIZE_OES => {
                *params = self.state.palette_index_state.size;
                return;
            }
            gles11::MATRIX_INDEX_ARRAY_TYPE_OES => {
                *params = self.state.palette_index_state.type_ as GLint;
                return;
            }
            gles11::MATRIX_INDEX_ARRAY_STRIDE_OES => {
                *params = self.state.palette_index_state.stride;
                return;
            }
            gles11::WEIGHT_ARRAY_SIZE_OES => {
                *params = self.state.palette_weight_state.size;
                return;
            }
            gles11::WEIGHT_ARRAY_TYPE_OES => {
                *params = self.state.palette_weight_state.type_ as GLint;
                return;
            }
            gles11::WEIGHT_ARRAY_STRIDE_OES => {
                *params = self.state.palette_weight_state.stride;
                return;
            }
            _ => {}
        }
        let (type_, count) = GET_PARAMS.get_type_info(pname);
        let count = usize::from(count.max(1));
        match type_ {
            ParamType::Int => {
                gl21::GetIntegerv(pname, params);
            }
            ParamType::Boolean => {
                // GL_TRUE / GL_FALSE map to 1 / 0.
                let mut tmp = [0u8; 16];
                let slice = &mut tmp[..count];
                gl21::GetBooleanv(pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = if v != 0 { 1 } else { 0 };
                }
            }
            ParamType::Float => {
                // Round to the nearest integer, clamped to the GLint range.
                let mut tmp = [0f32; 16];
                let slice = &mut tmp[..count];
                gl21::GetFloatv(pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    let rounded = (v as f64).round();
                    let clamped = rounded.clamp(GLint::MIN as f64, GLint::MAX as f64);
                    *params.add(i) = clamped as GLint;
                }
            }
            ParamType::FloatSpecial => {
                // Normalized floating-point components (colors, clear values,
                // etc.) are scaled to the full GLint range per the GLES 1.1
                // spec table 6.1. We approximate the spec formula
                // c_i = round((2^32 - 1) * c_f - 1) / 2 by scaling by
                // GLint::MAX, which is well within float precision and
                // matches what real iPhone OS drivers return in practice.
                let mut tmp = [0f32; 16];
                let slice = &mut tmp[..count];
                gl21::GetFloatv(pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    let scaled = (v as f64) * (GLint::MAX as f64);
                    let clamped = scaled.clamp(GLint::MIN as f64, GLint::MAX as f64);
                    *params.add(i) = clamped.round() as GLint;
                }
            }
            _ => {
                // Fallback for unknown/future param types: pass through to
                // the host driver and hope for the best, rather than
                // crashing the whole emulator.
                gl21::GetIntegerv(pname, params);
            }
        }
    }
    unsafe fn GetTexEnviv(&mut self, target: GLenum, pname: GLenum, params: *mut GLint) {
        let (type_, _count) = TEX_ENV_PARAMS.get_type_info(pname);
        assert!(type_ == ParamType::Int);
        assert_eq!(target, gl21::TEXTURE_ENV);
        gl21::GetTexEnviv(target, pname, params);
    }
    unsafe fn GetTexEnvfv(&mut self, target: GLenum, pname: GLenum, params: *mut GLfloat) {
        let (type_, _count) = TEX_ENV_PARAMS.get_type_info(pname);
        assert!(type_ == ParamType::Float);
        assert_eq!(target, gl21::TEXTURE_ENV);
        gl21::GetTexEnvfv(target, pname, params);
    }
    unsafe fn GetTexEnvxv(&mut self, target: GLenum, pname: GLenum, params: *mut GLfixed) {
        let (type_, count) = TEX_ENV_PARAMS.get_type_info(pname);
        assert_eq!(target, gl21::TEXTURE_ENV);
        // Desktop GL 2.1 doesn't have an `x`-typed `glGetTexEnv` entry point
        // (fixed-point is ES-only), so query through the float/int path and
        // convert per the ES 1.1 conversion rules.
        match type_ {
            ParamType::Float | ParamType::FloatSpecial => {
                let mut tmp = [0f32; 16];
                let slice = &mut tmp[..count as usize];
                gl21::GetTexEnvfv(target, pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = float_to_fixed(v);
                }
            }
            ParamType::Boolean => {
                let mut tmp = [0i32; 16];
                let slice = &mut tmp[..count as usize];
                gl21::GetTexEnviv(target, pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = if v != 0 { 1 } else { 0 };
                }
            }
            _ => {
                let mut tmp = [0i32; 16];
                let slice = &mut tmp[..count as usize];
                gl21::GetTexEnviv(target, pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = v as GLfixed;
                }
            }
        }
    }
    unsafe fn GetTexParameteriv(&mut self, target: GLenum, pname: GLenum, params: *mut GLint) {
        assert!(target == gl21::TEXTURE_2D);
        TEX_PARAMS.assert_known_param(pname);
        gl21::GetTexParameteriv(target, pname, params);
    }
    unsafe fn GetTexParameterfv(&mut self, target: GLenum, pname: GLenum, params: *mut GLfloat) {
        assert!(target == gl21::TEXTURE_2D);
        TEX_PARAMS.assert_known_param(pname);
        gl21::GetTexParameterfv(target, pname, params);
    }
    unsafe fn GetTexParameterxv(&mut self, target: GLenum, pname: GLenum, params: *mut GLfixed) {
        assert!(target == gl21::TEXTURE_2D);
        let (type_, count) = TEX_PARAMS.get_type_info(pname);
        match type_ {
            ParamType::Float | ParamType::FloatSpecial => {
                let mut tmp = [0f32; 16];
                let slice = &mut tmp[..count as usize];
                gl21::GetTexParameterfv(target, pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = float_to_fixed(v);
                }
            }
            _ => {
                let mut tmp = [0i32; 16];
                let slice = &mut tmp[..count as usize];
                gl21::GetTexParameteriv(target, pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = v as GLfixed;
                }
            }
        }
    }
    unsafe fn GetClipPlanef(&mut self, plane: GLenum, equation: *mut GLfloat) {
        // Desktop GL 2.1 only has the double-precision entry point.
        let mut tmp = [0f64; 4];
        gl21::GetClipPlane(plane, tmp.as_mut_ptr());
        for (i, &v) in tmp.iter().enumerate() {
            *equation.add(i) = v as GLfloat;
        }
    }
    unsafe fn GetClipPlanex(&mut self, plane: GLenum, equation: *mut GLfixed) {
        let mut tmp = [0f64; 4];
        gl21::GetClipPlane(plane, tmp.as_mut_ptr());
        for (i, &v) in tmp.iter().enumerate() {
            *equation.add(i) = float_to_fixed(v as GLfloat);
        }
    }
    unsafe fn GetLightfv(&mut self, light: GLenum, pname: GLenum, params: *mut GLfloat) {
        LIGHT_PARAMS.assert_known_param(pname);
        gl21::GetLightfv(light, pname, params)
    }
    unsafe fn GetLightxv(&mut self, light: GLenum, pname: GLenum, params: *mut GLfixed) {
        let (type_, count) = LIGHT_PARAMS.get_type_info(pname);
        match type_ {
            ParamType::Float | ParamType::FloatSpecial => {
                let mut tmp = [0f32; 16];
                let slice = &mut tmp[..count as usize];
                gl21::GetLightfv(light, pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = float_to_fixed(v);
                }
            }
            _ => {
                let mut tmp = [0i32; 16];
                let slice = &mut tmp[..count as usize];
                gl21::GetLightiv(light, pname, slice.as_mut_ptr());
                for (i, &v) in slice.iter().enumerate() {
                    *params.add(i) = v as GLfixed;
                }
            }
        }
    }
    unsafe fn GetMaterialfv(&mut self, face: GLenum, pname: GLenum, params: *mut GLfloat) {
        assert!(face == gl21::FRONT || face == gl21::BACK);
        MATERIAL_PARAMS.assert_known_param(pname);
        gl21::GetMaterialfv(face, pname, params)
    }
    unsafe fn GetMaterialxv(&mut self, face: GLenum, pname: GLenum, params: *mut GLfixed) {
        assert!(face == gl21::FRONT || face == gl21::BACK);
        let (_type, count) = MATERIAL_PARAMS.get_type_info(pname);
        let mut tmp = [0f32; 16];
        let slice = &mut tmp[..count as usize];
        gl21::GetMaterialfv(face, pname, slice.as_mut_ptr());
        for (i, &v) in slice.iter().enumerate() {
            *params.add(i) = float_to_fixed(v);
        }
    }
    unsafe fn GetPointerv(&mut self, pname: GLenum, params: *mut *const GLvoid) {
        assert!(ARRAYS
            .iter()
            .any(|&ArrayInfo { pointer, .. }| pname == pointer));
        // The second argument to glGetPointerv must be a mutable pointer,
        // but gl_generator generates the wrong signature by mistake, see
        // https://github.com/brendanzab/gl-rs/issues/541
        gl21::GetPointerv(pname, params as *mut _ as *const _);
    }
    unsafe fn Hint(&mut self, target: GLenum, mode: GLenum) {
        assert!([
            gl21::FOG_HINT,
            gl21::GENERATE_MIPMAP_HINT,
            gl21::LINE_SMOOTH_HINT,
            gl21::PERSPECTIVE_CORRECTION_HINT,
            gl21::POINT_SMOOTH_HINT
        ]
        .contains(&target));
        if mode == 0x0 {
            log_dbg!("Tolerating glHint({:#x}, {:#x})", target, mode);
        } else {
            assert!(
                [gl21::FASTEST, gl21::NICEST, gl21::DONT_CARE].contains(&mode),
                "Unexpected mode in glHint({target:#x}, {mode:#x})"
            );
        }
        gl21::Hint(target, mode);
    }
    unsafe fn Finish(&mut self) {
        let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::Finish,1,0);
        gl21::Finish();
    }
    unsafe fn Flush(&mut self) {
        let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::Flush,1,0);
        gl21::Flush();
    }
    unsafe fn GetString(&mut self, name: GLenum) -> *const GLubyte {
        gl21::GetString(name)
    }

    // Other state manipulation
    unsafe fn AlphaFunc(&mut self, func: GLenum, ref_: GLclampf) {
        self.state.scene.set(2,((func as u64)<<32)|ref_.to_bits() as u64);
        assert!([
            gl21::NEVER,
            gl21::LESS,
            gl21::EQUAL,
            gl21::LEQUAL,
            gl21::GREATER,
            gl21::NOTEQUAL,
            gl21::GEQUAL,
            gl21::ALWAYS
        ]
        .contains(&func));
        gl21::AlphaFunc(func, ref_)
    }
    unsafe fn AlphaFuncx(&mut self, func: GLenum, ref_: GLclampx) {
        self.AlphaFunc(func, fixed_to_float(ref_))
    }
    unsafe fn BlendFunc(&mut self, sfactor: GLenum, dfactor: GLenum) {
        self.state.scene.set(1,((sfactor as u64)<<32)|dfactor as u64);
        let common_factors = [
            gl21::ZERO,
            gl21::ONE,
            gl21::SRC_ALPHA,
            gl21::ONE_MINUS_SRC_ALPHA,
            gl21::DST_ALPHA,
            gl21::ONE_MINUS_DST_ALPHA,
        ];
        let sfactors = [
            gl21::DST_COLOR,
            gl21::ONE_MINUS_DST_COLOR,
            gl21::SRC_ALPHA_SATURATE,
        ];
        let dfactors = [gl21::SRC_COLOR, gl21::ONE_MINUS_SRC_COLOR];
        assert!(
            common_factors.contains(&sfactor)
                || sfactors.contains(&sfactor)
                || dfactors.contains(&sfactor)
        );
        assert!(
            common_factors.contains(&dfactor)
                || sfactors.contains(&dfactor)
                || dfactors.contains(&dfactor)
        );
        if sfactors.contains(&dfactor) {
            log_dbg!("Tolerating sfactor {:#x} in dfactor argument", dfactor);
        }
        if dfactors.contains(&sfactor) {
            log_dbg!("Tolerating dfactor {:#x} in sfactor argument", sfactor);
        }
        gl21::BlendFunc(sfactor, dfactor);
    }
    unsafe fn BlendEquationOES(&mut self, mode: GLenum) {
        let functions = [
            gl21::FUNC_ADD,
            gl21::FUNC_SUBTRACT,
            gl21::FUNC_REVERSE_SUBTRACT,
        ];
        assert!(functions.contains(&mode));
        gl21::BlendEquation(mode);
    }
    unsafe fn ColorMask(
        &mut self,
        red: GLboolean,
        green: GLboolean,
        blue: GLboolean,
        alpha: GLboolean,
    ) {
        gl21::ColorMask(red, green, blue, alpha)
    }
    unsafe fn ClipPlanef(&mut self, plane: GLenum, equation: *const GLfloat) {
        let mut max_planes = 0;
        gl21::GetIntegerv(gl21::MAX_CLIP_PLANES, &mut max_planes);
        assert!(gl21::CLIP_PLANE0 <= plane && plane < (gl21::CLIP_PLANE0 + max_planes as u32));

        let mut equation_double: [GLdouble; 4] = [0.0; 4];
        #[allow(clippy::needless_range_loop)]
        for i in 0..4 {
            equation_double[i] = *equation.wrapping_add(i) as GLdouble;
        }
        gl21::ClipPlane(plane, &equation_double as _)
    }
    unsafe fn ClipPlanex(&mut self, plane: GLenum, equation: *const GLfixed) {
        let mut max_planes = 0;
        gl21::GetIntegerv(gl21::MAX_CLIP_PLANES, &mut max_planes);
        assert!(gl21::CLIP_PLANE0 <= plane && plane < (gl21::CLIP_PLANE0 + max_planes as u32));

        let mut equation_double: [GLdouble; 4] = [0.0; 4];
        #[allow(clippy::needless_range_loop)]
        for i in 0..4 {
            equation_double[i] = fixed_to_float(*equation.wrapping_add(i)) as GLdouble;
        }
        gl21::ClipPlane(plane, &equation_double as _)
    }
    unsafe fn CullFace(&mut self, mode: GLenum) {
        if mode == gl21::CCW {
            log_dbg!("Tolerating glCullFace({:#x})", mode);
        } else {
            assert!(
                [gl21::FRONT, gl21::BACK, gl21::FRONT_AND_BACK].contains(&mode),
                "Unexpected glCullFace({mode:#x})"
            );
        }
        let forced = crate::ds_draw_env::var("TOUCHHLE_DS_FORCE_CULL_FACE").ok();
        let effective = match forced.as_deref() {
            Some("front") => gl21::FRONT,
            Some("back") => gl21::BACK,
            Some("both") => gl21::FRONT_AND_BACK,
            _ => mode,
        };
        if effective != mode {
            log_once!("TOUCHHLE_DS_FORCE_CULL_FACE: overriding cull face mode");
        }
        gl21::CullFace(effective);
    }
    unsafe fn DepthFunc(&mut self, func: GLenum) {
        self.state.scene.set(3,func as u64);
        assert!([
            gl21::NEVER,
            gl21::LESS,
            gl21::EQUAL,
            gl21::LEQUAL,
            gl21::GREATER,
            gl21::NOTEQUAL,
            gl21::GEQUAL,
            gl21::ALWAYS
        ]
        .contains(&func));
        gl21::DepthFunc(func)
    }
    unsafe fn DepthMask(&mut self, flag: GLboolean) {
        gl21::DepthMask(flag)
    }
    unsafe fn FrontFace(&mut self, mode: GLenum) {
        assert!(mode == gl21::CW || mode == gl21::CCW);
        gl21::FrontFace(mode);
    }
    unsafe fn DepthRangef(&mut self, near: GLclampf, far: GLclampf) {
        if crate::ds_draw_env::contains("TOUCHHLE_DS_PROJ_TRACE") {
            log!("[PROJTRACE] DepthRangef {} {}", near, far);
        }
        gl21::DepthRange(near.into(), far.into())
    }
    unsafe fn DepthRangex(&mut self, near: GLclampx, far: GLclampx) {
        gl21::DepthRange(fixed_to_float(near).into(), fixed_to_float(far).into())
    }
    unsafe fn PolygonOffset(&mut self, factor: GLfloat, units: GLfloat) {
        gl21::PolygonOffset(factor, units)
    }
    unsafe fn PolygonOffsetx(&mut self, factor: GLfixed, units: GLfixed) {
        gl21::PolygonOffset(fixed_to_float(factor), fixed_to_float(units))
    }
    unsafe fn SampleCoverage(&mut self, value: GLclampf, invert: GLboolean) {
        gl21::SampleCoverage(value, invert)
    }
    unsafe fn SampleCoveragex(&mut self, value: GLclampx, invert: GLboolean) {
        gl21::SampleCoverage(fixed_to_float(value), invert)
    }
    unsafe fn ShadeModel(&mut self, mode: GLenum) {
        self.state.scene.set(4,mode as u64);
        assert!(mode == gl21::FLAT || mode == gl21::SMOOTH);
        gl21::ShadeModel(mode);
    }
    unsafe fn Scissor(&mut self, x: GLint, y: GLint, width: GLsizei, height: GLsizei) {
        gl21::Scissor(x, y, width, height)
    }
    unsafe fn Viewport(&mut self, x: GLint, y: GLint, width: GLsizei, height: GLsizei) {
        gl21::Viewport(x, y, width, height)
    }
    unsafe fn LineWidth(&mut self, val: GLfloat) {
        gl21::LineWidth(val)
    }
    unsafe fn LineWidthx(&mut self, val: GLfixed) {
        gl21::LineWidth(fixed_to_float(val))
    }
    unsafe fn StencilFunc(&mut self, func: GLenum, ref_: GLint, mask: GLuint) {
        assert!([
            gl21::NEVER,
            gl21::LESS,
            gl21::EQUAL,
            gl21::LEQUAL,
            gl21::GREATER,
            gl21::NOTEQUAL,
            gl21::GEQUAL,
            gl21::ALWAYS
        ]
        .contains(&func));
        gl21::StencilFunc(func, ref_, mask);
    }
    unsafe fn StencilOp(&mut self, sfail: GLenum, dpfail: GLenum, dppass: GLenum) {
        for enum_ in [sfail, dpfail, dppass].iter() {
            assert!([
                gl21::KEEP,
                gl21::ZERO,
                gl21::REPLACE,
                gl21::INCR,
                gl21::DECR,
                gl21::INVERT,
            ]
            .contains(enum_));
        }
        gl21::StencilOp(sfail, dpfail, dppass);
    }
    unsafe fn StencilMask(&mut self, mask: GLuint) {
        gl21::StencilMask(mask);
    }
    unsafe fn LogicOp(&mut self, opcode: GLenum) {
        assert!([
            gl21::CLEAR,
            gl21::SET,
            gl21::COPY,
            gl21::COPY_INVERTED,
            gl21::NOOP,
            gl21::INVERT,
            gl21::AND,
            gl21::NAND,
            gl21::OR,
            gl21::NOR,
            gl21::XOR,
            gl21::EQUIV,
            gl21::AND_REVERSE,
            gl21::AND_INVERTED,
            gl21::OR_REVERSE,
            gl21::OR_INVERTED,
        ]
        .contains(&opcode));
        gl21::LogicOp(opcode);
    }

    // Points
    unsafe fn PointSize(&mut self, size: GLfloat) {
        gl21::PointSize(size)
    }
    unsafe fn PointSizex(&mut self, size: GLfixed) {
        gl21::PointSize(fixed_to_float(size))
    }
    unsafe fn PointParameterf(&mut self, pname: GLenum, param: GLfloat) {
        gl21::PointParameterf(pname, param)
    }
    unsafe fn PointParameterx(&mut self, pname: GLenum, param: GLfixed) {
        POINT_PARAMS.setx(
            |param| gl21::PointParameterf(pname, param),
            |_| unreachable!(), // no integer parameters exist
            pname,
            param,
        );
    }
    unsafe fn PointParameterfv(&mut self, pname: GLenum, params: *const GLfloat) {
        gl21::PointParameterfv(pname, params)
    }
    unsafe fn PointParameterxv(&mut self, pname: GLenum, params: *const GLfixed) {
        POINT_PARAMS.setxv(
            |params| gl21::PointParameterfv(pname, params),
            |_| unreachable!(), // no integer parameters exist
            pname,
            params,
        );
    }

    // Lighting and materials
    unsafe fn Fogf(&mut self, pname: GLenum, param: GLfloat) {
        FOG_PARAMS.assert_component_count(pname, 1);
        gl21::Fogf(pname, param);
    }
    unsafe fn Fogx(&mut self, pname: GLenum, param: GLfixed) {
        FOG_PARAMS.setx(
            |param| gl21::Fogf(pname, param),
            |param| gl21::Fogi(pname, param),
            pname,
            param,
        )
    }
    unsafe fn Fogfv(&mut self, pname: GLenum, params: *const GLfloat) {
        FOG_PARAMS.assert_known_param(pname);
        gl21::Fogfv(pname, params);
    }
    unsafe fn Fogxv(&mut self, pname: GLenum, params: *const GLfixed) {
        FOG_PARAMS.setxv(
            |params| gl21::Fogfv(pname, params),
            |params| gl21::Fogiv(pname, params),
            pname,
            params,
        )
    }
    unsafe fn Lightf(&mut self, light: GLenum, pname: GLenum, param: GLfloat) {
        LIGHT_PARAMS.assert_component_count(pname, 1);
        gl21::Lightf(light, pname, param);
    }
    unsafe fn Lightx(&mut self, light: GLenum, pname: GLenum, param: GLfixed) {
        LIGHT_PARAMS.setx(
            |param| gl21::Lightf(light, pname, param),
            |param| gl21::Lighti(light, pname, param),
            pname,
            param,
        )
    }
    unsafe fn Lightfv(&mut self, light: GLenum, pname: GLenum, params: *const GLfloat) {
        LIGHT_PARAMS.assert_known_param(pname);
        gl21::Lightfv(light, pname, params);
    }
    unsafe fn Lightxv(&mut self, light: GLenum, pname: GLenum, params: *const GLfixed) {
        LIGHT_PARAMS.setxv(
            |params| gl21::Lightfv(light, pname, params),
            |params| gl21::Lightiv(light, pname, params),
            pname,
            params,
        )
    }
    unsafe fn LightModelf(&mut self, pname: GLenum, param: GLfloat) {
        LIGHT_MODEL_PARAMS.assert_component_count(pname, 1);
        gl21::LightModelf(pname, param)
    }
    unsafe fn LightModelx(&mut self, pname: GLenum, param: GLfixed) {
        LIGHT_MODEL_PARAMS.setx(
            |param| gl21::LightModelf(pname, param),
            |param| gl21::LightModeli(pname, param),
            pname,
            param,
        )
    }
    unsafe fn LightModelfv(&mut self, pname: GLenum, params: *const GLfloat) {
        LIGHT_MODEL_PARAMS.assert_known_param(pname);
        gl21::LightModelfv(pname, params)
    }
    unsafe fn LightModelxv(&mut self, pname: GLenum, params: *const GLfixed) {
        LIGHT_MODEL_PARAMS.setxv(
            |param| gl21::LightModelfv(pname, param),
            |param| gl21::LightModeliv(pname, param),
            pname,
            params,
        )
    }
    unsafe fn Materialf(&mut self, face: GLenum, pname: GLenum, param: GLfloat) {
        assert!(face == gl21::FRONT_AND_BACK);
        MATERIAL_PARAMS.assert_component_count(pname, 1);
        gl21::Materialf(face, pname, param);
    }
    unsafe fn Materialx(&mut self, face: GLenum, pname: GLenum, param: GLfixed) {
        assert!(face == gl21::FRONT_AND_BACK);
        MATERIAL_PARAMS.setx(
            |param| gl21::Materialf(face, pname, param),
            |_| unreachable!(), // no integer parameters exist
            pname,
            param,
        )
    }
    unsafe fn Materialfv(&mut self, face: GLenum, pname: GLenum, params: *const GLfloat) {
        if face == gl21::FRONT || face == gl21::BACK {
            log!(
                "App is calling glMaterialfv({:#x}, {:#x}, {:?}) with wrong face value, ignoring",
                face,
                pname,
                params
            );
            return;
        }
        assert!(face == gl21::FRONT_AND_BACK);
        MATERIAL_PARAMS.assert_known_param(pname);
        gl21::Materialfv(face, pname, params);
    }
    unsafe fn Materialxv(&mut self, face: GLenum, pname: GLenum, params: *const GLfixed) {
        assert!(face == gl21::FRONT_AND_BACK);
        MATERIAL_PARAMS.setxv(
            |params| gl21::Materialfv(face, pname, params),
            |_| unreachable!(), // no integer parameters exist
            pname,
            params,
        )
    }

    // Buffers
    unsafe fn IsBuffer(&mut self, buffer: GLuint) -> GLboolean {
        gl21::IsBuffer(buffer)
    }
    unsafe fn GenBuffers(&mut self, n: GLsizei, buffers: *mut GLuint) {
        gl21::GenBuffers(n, buffers)
    }
    unsafe fn DeleteBuffers(&mut self, n: GLsizei, buffers: *const GLuint) {
        gl21::DeleteBuffers(n, buffers)
    }
    unsafe fn BindBuffer(&mut self, target: GLenum, buffer: GLuint) {
        assert!(target == gl21::ARRAY_BUFFER || target == gl21::ELEMENT_ARRAY_BUFFER);
        gl21::BindBuffer(target, buffer)
    }
    unsafe fn BufferData(
        &mut self,
        target: GLenum,
        size: GLsizeiptr,
        data: *const GLvoid,
        usage: GLenum,
    ) {
        assert!(target == gl21::ARRAY_BUFFER || target == gl21::ELEMENT_ARRAY_BUFFER);
        gl21::BufferData(target, size, data, usage)
    }

    unsafe fn BufferSubData(
        &mut self,
        target: GLenum,
        offset: GLintptr,
        size: GLsizeiptr,
        data: *const GLvoid,
    ) {
        assert!(target == gl21::ARRAY_BUFFER || target == gl21::ELEMENT_ARRAY_BUFFER);
        gl21::BufferSubData(target, offset, size, data)
    }

    // Non-pointers
    unsafe fn Color4f(&mut self, red: GLfloat, green: GLfloat, blue: GLfloat, alpha: GLfloat) {
        gl21::Color4f(red, green, blue, alpha)
    }
    unsafe fn Color4x(&mut self, red: GLfixed, green: GLfixed, blue: GLfixed, alpha: GLfixed) {
        gl21::Color4f(
            fixed_to_float(red),
            fixed_to_float(green),
            fixed_to_float(blue),
            fixed_to_float(alpha),
        )
    }
    unsafe fn Color4ub(&mut self, red: GLubyte, green: GLubyte, blue: GLubyte, alpha: GLubyte) {
        gl21::Color4ub(red, green, blue, alpha)
    }
    unsafe fn Normal3f(&mut self, nx: GLfloat, ny: GLfloat, nz: GLfloat) {
        gl21::Normal3f(nx, ny, nz)
    }
    unsafe fn Normal3x(&mut self, nx: GLfixed, ny: GLfixed, nz: GLfixed) {
        gl21::Normal3f(fixed_to_float(nx), fixed_to_float(ny), fixed_to_float(nz))
    }

    // Pointers
    unsafe fn ColorPointer(
        &mut self,
        size: GLint,
        type_: GLenum,
        stride: GLsizei,
        pointer: *const GLvoid,
    ) {
        assert!(size == 4);
        if type_ == gles11::FIXED {
            // Translation deferred until draw call
            self.state.pointer_is_fixed_point[0] = true;
            gl21::ColorPointer(size, gl21::FLOAT, stride, pointer)
        } else {
            assert!(type_ == gl21::UNSIGNED_BYTE || type_ == gl21::FLOAT);
            self.state.pointer_is_fixed_point[0] = false;
            gl21::ColorPointer(size, type_, stride, pointer)
        }
    }
    unsafe fn NormalPointer(&mut self, type_: GLenum, stride: GLsizei, pointer: *const GLvoid) {
        if type_ == gles11::FIXED {
            // Translation deferred until draw call
            self.state.pointer_is_fixed_point[1] = true;
            gl21::NormalPointer(gl21::FLOAT, stride, pointer)
        } else {
            assert!(type_ == gl21::BYTE || type_ == gl21::SHORT || type_ == gl21::FLOAT);
            self.state.pointer_is_fixed_point[1] = false;
            gl21::NormalPointer(type_, stride, pointer)
        }
    }
    unsafe fn TexCoordPointer(
        &mut self,
        size: GLint,
        type_: GLenum,
        stride: GLsizei,
        pointer: *const GLvoid,
    ) {
        assert!(size == 2 || size == 3 || size == 4);
        let mut active_texture: GLenum = 0;
        gl21::GetIntegerv(
            gl21::CLIENT_ACTIVE_TEXTURE,
            &mut active_texture as *mut _ as *mut _,
        );
        if type_ == gles11::FIXED {
            // Translation deferred until draw call.
            // There is one texture co-ordinates pointer per texture unit.
            self.state.fixed_point_texture_units.insert(active_texture);
            self.state.pointer_is_fixed_point[2] = true;
            gl21::TexCoordPointer(size, gl21::FLOAT, stride, pointer)
        } else {
            // TODO: byte
            assert!(type_ == gl21::SHORT || type_ == gl21::FLOAT);
            self.state.fixed_point_texture_units.remove(&active_texture);
            if self.state.fixed_point_texture_units.is_empty() {
                self.state.pointer_is_fixed_point[2] = false;
            }
            gl21::TexCoordPointer(size, type_, stride, pointer)
        }
    }
    unsafe fn VertexPointer(
        &mut self,
        size: GLint,
        type_: GLenum,
        stride: GLsizei,
        pointer: *const GLvoid,
    ) {
        self.state.scene.set(9,((size as u64)<<32)|type_ as u64);
        assert!(size == 2 || size == 3 || size == 4);
        if type_ == gles11::FIXED {
            // Translation deferred until draw call
            self.state.pointer_is_fixed_point[3] = true;
            gl21::VertexPointer(size, gl21::FLOAT, stride, pointer)
        } else {
            // TODO: byte
            assert!(type_ == gl21::SHORT || type_ == gl21::FLOAT);
            self.state.pointer_is_fixed_point[3] = false;
            gl21::VertexPointer(size, type_, stride, pointer)
        }
    }

    unsafe fn MatrixIndexPointerOES(
        &mut self,
        size: GLint,
        type_: GLenum,
        stride: GLsizei,
        pointer: *const GLvoid,
    ) {
        // GL_OES_matrix_palette: per-vertex matrix indices for CPU skinning.
        // We capture the array description (and any bound buffer) here and
        // perform the actual blend in draw_with_matrix_palette at draw time.
        let mut buffer_binding: GLint = 0;
        gl21::GetIntegerv(gl21::ARRAY_BUFFER_BINDING, &mut buffer_binding);
        self.state.palette_index_state.size = size;
        self.state.palette_index_state.type_ = type_;
        self.state.palette_index_state.stride = stride;
        self.state.palette_index_state.pointer = pointer;
        self.state.palette_index_state.buffer_binding = buffer_binding as GLuint;
    }
    unsafe fn WeightPointerOES(
        &mut self,
        size: GLint,
        type_: GLenum,
        stride: GLsizei,
        pointer: *const GLvoid,
    ) {
        // GL_OES_matrix_palette: per-vertex blend weights for CPU skinning.
        let mut buffer_binding: GLint = 0;
        gl21::GetIntegerv(gl21::ARRAY_BUFFER_BINDING, &mut buffer_binding);
        self.state.palette_weight_state.size = size;
        self.state.palette_weight_state.type_ = type_;
        self.state.palette_weight_state.stride = stride;
        self.state.palette_weight_state.pointer = pointer;
        self.state.palette_weight_state.buffer_binding = buffer_binding as GLuint;
    }

    // Drawing
    unsafe fn DrawArrays(&mut self, mode: GLenum, first: GLint, count: GLsizei) {
        let _gl_profile = crate::ds_gl_profile::begin(crate::ds_gl_profile::Kind::DrawArrays);
        self.state.scene.draw(mode,0);
        self.log_draw_probe(mode, count);
        self.log_ds_texture_draw("arrays", mode, first, count, None);
        unsafe { ds_ring_probe("arrays", count) };
        assert!([
            gl21::POINTS,
            gl21::LINE_STRIP,
            gl21::LINE_LOOP,
            gl21::LINES,
            gl21::TRIANGLE_STRIP,
            gl21::TRIANGLE_FAN,
            gl21::TRIANGLES
        ]
        .contains(&mode));

        // GL_OES_matrix_palette skinning: transform vertices on the CPU and
        // draw with the blended positions if palette skinning is active.
        if self.matrix_palette_active() {
            if let Some(skinned) = self.skin_vertices(first, count) {
                self.draw_arrays_skinned(mode, first, count, &skinned);
                return;
            }
        }

        if skip_tex_active(count) {
            return;
        }
        let opening_fog = opening_fog_begin(&mut self.state.opening_fog);
        let nofog = nofog_begin(count);
        let nofog_o = nofog_ortho_begin();
        let fogsc = fog_scale_begin();
        let fogred = fog_red_begin();
        log_fog_state();
        let fixed_point_arrays_state_backup = self.translate_fixed_point_arrays(first, count);
        let nofog_e = nofog;

        let texmat_identity_backup = force_texmat_identity_begin();
        let texmat_identity_hash_count =
            force_texmat_identity_hash_count_begin(count);
        let proj_zzero = proj_z_zero_begin(count);
        let redtex = redtex_begin(count);
        let texidcol = texid_color_begin();
        let vcoloff = vcolor_off_begin();
        let texfilter = texfilter_begin(count);
        let texlod = texlod_begin();
        let texwrap = texwrap_begin();
        let texnodepth = texnodepth_begin(count);
        let texflat = texflat_begin();
        let texblack = texblack_begin(count);
        let alphaov = alpha_override_begin();
        let pretex = pretransform_texcoords_begin(first, count, count);
        let unit_disable = tex_unit_disable_begin();
        let no_blend_hash_count = no_blend_hash_count_begin(count);
        let no_cull_hash_count = no_cull_hash_count_begin(count);
        let flip_winding_hash_count = flip_winding_hash_count_begin(count);
        clear_opening_depth_once_per_fbo();
        clear_depth_hash_count_once(count);
        let depth_always_hash_count = depth_always_hash_count_begin(count);
        let dst_fill_hash_count = dst_fill_hash_count_begin(count);
        let alpha_hash_count = alpha_hash_count_begin(count);
        let blood_depth = blood_depth_begin(mode, count);
        let pixel_trace = pixel_trace_begin();
        self.state.gpu_profile.begin(0);
        gl21::DrawArrays(mode, first, count);
        self.state.gpu_profile.end(0);
        pixel_trace_end(pixel_trace, "arrays", mode, count);
        blood_depth_end(blood_depth);
        alpha_hash_count_end(alpha_hash_count);
        dst_fill_hash_count_end(dst_fill_hash_count);
        depth_always_hash_count_end(depth_always_hash_count);
        flip_winding_hash_count_end(flip_winding_hash_count);
        no_cull_hash_count_end(no_cull_hash_count);
        no_blend_hash_count_end(no_blend_hash_count);
        tex_unit_disable_end(unit_disable);
        pretransform_texcoords_end(pretex);
        alpha_override_end(alphaov);
        texflat_end(texblack);
        texflat_end(texflat);
        texnodepth_end(texnodepth);
        texwrap_end(texwrap);
        texlod_end(texlod);
        texfilter_end(texfilter);
        redtex_end(redtex);
        vcolor_off_end(vcoloff);
        texid_color_end(texidcol);
        proj_z_zero_end(proj_zzero);
        force_texmat_identity_hash_count_end(texmat_identity_hash_count);
        force_texmat_identity_end(texmat_identity_backup);

        nofog_end(nofog_e);
        nofog_end(nofog_o);
        fog_scale_end(fogsc);
        fog_red_end(fogred);
        opening_fog_end(opening_fog);
        self.restore_fixed_point_arrays(fixed_point_arrays_state_backup);
    }
    unsafe fn DrawElements(
        &mut self,
        mode: GLenum,
        count: GLsizei,
        type_: GLenum,
        indices: *const GLvoid,
    ) {
        let _gl_profile = crate::ds_gl_profile::begin(crate::ds_gl_profile::Kind::DrawElements);
        self.state.scene.draw(mode,type_);
        if skip_tex_active(count) {
            return;
        }
        let opening_fog = opening_fog_begin(&mut self.state.opening_fog);
        let nofog = nofog_begin(count);
        let nofog_o = nofog_ortho_begin();
        let fogsc = fog_scale_begin();
        let fogred = fog_red_begin();
        log_fog_state();
        self.log_ds_texture_draw("elements", mode, 0, count, Some((indices, type_)));
        unsafe { ds_ring_probe("elements", count) };
        assert!([
            gl21::POINTS,
            gl21::LINE_STRIP,
            gl21::LINE_LOOP,
            gl21::LINES,
            gl21::TRIANGLE_STRIP,
            gl21::TRIANGLE_FAN,
            gl21::TRIANGLES
        ]
        .contains(&mode));
        assert!(type_ == gl21::UNSIGNED_BYTE || type_ == gl21::UNSIGNED_SHORT);

        // GL_OES_matrix_palette skinning for indexed draws: skin the full
        // range of referenced vertices, then draw with blended positions.
        if self.matrix_palette_active() {
            if let Some((first, vcount)) = self.indexed_draw_vertex_range(count, type_, indices) {
                if let Some(skinned) = self.skin_vertices(first, vcount) {
                    self.draw_elements_skinned(mode, count, type_, indices, &skinned);
                    nofog_end(nofog_o);
                    nofog_end(nofog);
                    fog_scale_end(fogsc);
                    fog_red_end(fogred);
                    opening_fog_end(opening_fog);
                    return;
                }
            }
        }

        let fixed_point_arrays_state_backup = if self
            .state
            .pointer_is_fixed_point
            .iter()
            .any(|&is_fixed| is_fixed)
        {
            // Scan the index buffer to find the range of data that may need
            // fixed-point translation.
            // TODO: Would it be more efficient to turn this into a
            // non-indexed draw-call instead?

            let mut index_buffer_binding = 0;
            gl21::GetIntegerv(
                gl21::ELEMENT_ARRAY_BUFFER_BINDING,
                &mut index_buffer_binding,
            );
            let indices = if index_buffer_binding != 0 {
                let mapped_buffer = gl21::MapBuffer(gl21::ELEMENT_ARRAY_BUFFER, gl21::READ_ONLY);
                assert!(!mapped_buffer.is_null());
                // in this case the indices is actually an offest!
                mapped_buffer.add(indices as usize)
            } else {
                indices
            };

            let mut first = usize::MAX;
            let mut last = usize::MIN;
            assert!(count >= 0);
            match type_ {
                gl21::UNSIGNED_BYTE => {
                    let indices_ptr: *const GLubyte = indices.cast();
                    for i in 0..(count as usize) {
                        let index = indices_ptr.add(i).read_unaligned();
                        first = first.min(index as usize);
                        last = last.max(index as usize);
                    }
                }
                gl21::UNSIGNED_SHORT => {
                    let indices_ptr: *const GLushort = indices.cast();
                    for i in 0..(count as usize) {
                        let index = indices_ptr.add(i).read_unaligned();
                        first = first.min(index as usize);
                        last = last.max(index as usize);
                    }
                }
                _ => unreachable!(),
            }

            let (first, count) = if first == usize::MAX && last == usize::MIN {
                assert!(count == 0);
                (0, 0)
            } else {
                (
                    first.try_into().unwrap(),
                    (last + 1 - first).try_into().unwrap(),
                )
            };

            if index_buffer_binding != 0 {
                gl21::UnmapBuffer(gl21::ELEMENT_ARRAY_BUFFER);
            }

            Some(self.translate_fixed_point_arrays(first, count))
        } else {
            None
        };

        let texmat_identity_backup = force_texmat_identity_begin();
        let texmat_identity_hash_count =
            force_texmat_identity_hash_count_begin(count);
        let proj_zzero = proj_z_zero_begin(count);
        let redtex = redtex_begin(count);
        let texidcol = texid_color_begin();
        let vcoloff = vcolor_off_begin();
        let texfilter = texfilter_begin(count);
        let texlod = texlod_begin();
        let texwrap = texwrap_begin();
        let texnodepth = texnodepth_begin(count);
        let texflat = texflat_begin();
        let texblack = texblack_begin(count);
        let alphaov = alpha_override_begin();
        // The optional texcoord diagnostic must not map/read/scan every index
        // when it is disabled. Matrix-palette and GL_FIXED range walks above
        // remain required and unchanged. Live-env mode preserves the old path
        // for same-binary A/B and compatibility rollback.
        let pretex = if crate::ds_draw_env::enabled()
            && crate::ds_draw_env::var("TOUCHHLE_DS_PRETRANSFORM_TEX_HASH").is_err()
        {
            None
        } else {
            let (first, vertices) = self.indexed_draw_vertex_range(count, type_, indices)
                .unwrap_or((0, count));
            pretransform_texcoords_begin(first, vertices, count)
        };
        let unit_disable = tex_unit_disable_begin();
        let no_blend_hash_count = no_blend_hash_count_begin(count);
        let no_cull_hash_count = no_cull_hash_count_begin(count);
        let flip_winding_hash_count = flip_winding_hash_count_begin(count);
        clear_opening_depth_once_per_fbo();
        clear_depth_hash_count_once(count);
        let depth_always_hash_count = depth_always_hash_count_begin(count);
        let dst_fill_hash_count = dst_fill_hash_count_begin(count);
        let alpha_hash_count = alpha_hash_count_begin(count);
        let blood_depth = blood_depth_begin(mode, count);
        let pixel_trace = pixel_trace_begin();
        if !(blood_depth && self.draw_blood_scaled_elements(count, type_, indices))
            && !draw_elements_skipping_triangles(mode, count, type_, indices)
            && !draw_elements_with_no_cull_triangles(mode, count, type_, indices)
            && !draw_elements_with_unlit_backfaces(mode, count, type_, indices)
            && !draw_elements_with_depth_always_triangles(mode, count, type_, indices)
            && !split_triangle_depth_draw_elements(mode, count, type_, indices)
        {
            self.state.gpu_profile.begin(0);
            gl21::DrawElements(mode, count, type_, indices);
            self.state.gpu_profile.end(0);
        }
        pixel_trace_end(pixel_trace, "elements", mode, count);
        blood_depth_end(blood_depth);
        alpha_hash_count_end(alpha_hash_count);
        dst_fill_hash_count_end(dst_fill_hash_count);
        depth_always_hash_count_end(depth_always_hash_count);
        flip_winding_hash_count_end(flip_winding_hash_count);
        no_cull_hash_count_end(no_cull_hash_count);
        no_blend_hash_count_end(no_blend_hash_count);
        tex_unit_disable_end(unit_disable);
        pretransform_texcoords_end(pretex);
        alpha_override_end(alphaov);
        texflat_end(texblack);
        texflat_end(texflat);
        texnodepth_end(texnodepth);
        texwrap_end(texwrap);
        texlod_end(texlod);
        texfilter_end(texfilter);
        redtex_end(redtex);
        vcolor_off_end(vcoloff);
        texid_color_end(texidcol);
        proj_z_zero_end(proj_zzero);
        force_texmat_identity_hash_count_end(texmat_identity_hash_count);
        force_texmat_identity_end(texmat_identity_backup);

        // Fog overrides apply to every indexed draw, including float arrays.
        // Restore them independently of fixed-point array translation.
        fog_red_end(fogred);
        fog_scale_end(fogsc);
        nofog_end(nofog_o);
        nofog_end(nofog);
        opening_fog_end(opening_fog);
        if let Some(fixed_point_arrays_state_backup) = fixed_point_arrays_state_backup {
            self.restore_fixed_point_arrays(fixed_point_arrays_state_backup);
        }
    }

    unsafe fn diagnostic_frame_ready(&mut self) {
        pixel_trace_trigger_tick();
    }

    // Clearing
    unsafe fn Clear(&mut self, mask: GLbitfield) {
        assert!(
            mask & !(gl21::COLOR_BUFFER_BIT | gl21::DEPTH_BUFFER_BIT | gl21::STENCIL_BUFFER_BIT)
                == 0
        );
        self.state.gpu_profile.begin(9);
        gl21::Clear(mask);
        self.state.gpu_profile.end(9);
    }
    unsafe fn ClearColor(
        &mut self,
        red: GLclampf,
        green: GLclampf,
        blue: GLclampf,
        alpha: GLclampf,
    ) {
        gl21::ClearColor(red, green, blue, alpha)
    }
    unsafe fn ClearColorx(
        &mut self,
        red: GLclampx,
        green: GLclampx,
        blue: GLclampx,
        alpha: GLclampx,
    ) {
        gl21::ClearColor(
            fixed_to_float(red),
            fixed_to_float(green),
            fixed_to_float(blue),
            fixed_to_float(alpha),
        )
    }
    unsafe fn ClearDepthf(&mut self, depth: GLclampf) {
        gl21::ClearDepth(depth.into())
    }
    unsafe fn ClearDepthx(&mut self, depth: GLclampx) {
        self.ClearDepthf(fixed_to_float(depth))
    }
    unsafe fn ClearStencil(&mut self, s: GLint) {
        gl21::ClearStencil(s)
    }

    // Textures
    unsafe fn PixelStorei(&mut self, pname: GLenum, param: GLint) {
        assert!(pname == gl21::PACK_ALIGNMENT || pname == gl21::UNPACK_ALIGNMENT);
        assert!(param == 1 || param == 2 || param == 4 || param == 8);
        gl21::PixelStorei(pname, param)
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
        ds_observed_read_pixels(x, y, width, height, format, type_, pixels)
    }

    /// `glDrawTex*OES` from `GL_OES_draw_texture`.
    ///
    /// The extension specifies that the destination rectangle is given
    /// directly in window (viewport) coordinates and is *not* transformed by
    /// the model-view or projection matrices, and that the texture matrix is
    /// ignored. Texture coordinates come from the bound texture's
    /// `GL_TEXTURE_CROP_RECT_OES` parameter:
    ///
    /// ```text
    /// s = (Ucr + (X - Xs) * (Wcr / Ws)) / Wt
    /// t = (Vcr + (Y - Ys) * (Hcr / Hs)) / Ht
    /// ```
    unsafe fn DrawTexfOES(
        &mut self,
        x: GLfloat,
        y: GLfloat,
        z: GLfloat,
        width: GLfloat,
        height: GLfloat,
    ) {
        if !(width > 0.0) || !(height > 0.0) {
            // The spec calls this INVALID_VALUE; be lenient and draw nothing.
            return;
        }

        // Use the texture bound to the active unit. Some apps leave a spare
        // (texture-less) unit selected, so fall back to unit 0 in that case.
        let mut texture: GLint = 0;
        gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut texture);
        if texture == 0 {
            let mut prev_unit: GLint = 0;
            gl21::GetIntegerv(gl21::ACTIVE_TEXTURE, &mut prev_unit);
            if prev_unit as GLenum != gl21::TEXTURE0 {
                gl21::ActiveTexture(gl21::TEXTURE0);
                gl21::GetIntegerv(gl21::TEXTURE_BINDING_2D, &mut texture);
                gl21::ActiveTexture(prev_unit as GLenum);
            }
        }
        if texture == 0 {
            return;
        }

        let (mut tex_w, mut tex_h): (GLint, GLint) = (0, 0);
        gl21::GetTexLevelParameteriv(gl21::TEXTURE_2D, 0, gl21::TEXTURE_WIDTH, &mut tex_w);
        gl21::GetTexLevelParameteriv(gl21::TEXTURE_2D, 0, gl21::TEXTURE_HEIGHT, &mut tex_h);
        if tex_w <= 0 || tex_h <= 0 {
            return;
        }

        // The extension's state table gives (0, 0, 0, 0) as the initial value,
        // so an app that never sets the parameter gets a degenerate rect.
        let crop = self
            .state
            .texture_crop_rects
            .get(&(texture as GLuint))
            .copied()
            .unwrap_or([0, 0, 0, 0]);
        {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNT: AtomicUsize = AtomicUsize::new(0);
            let n = COUNT.fetch_add(1, Ordering::Relaxed);
            if n < 24 {
                log!(
                    "DrawTexfOES #{} x={} y={} z={} w={} h={} texture={} crop=[{},{},{},{}] tex={}x{}",
                    n, x, y, z, width, height, texture,
                    crop[0], crop[1], crop[2], crop[3], tex_w, tex_h
                );
            }
        }

        let mut viewport = [0 as GLint; 4];
        gl21::GetIntegerv(gl21::VIEWPORT, viewport.as_mut_ptr());
        let (vp_x, vp_y, vp_w, vp_h) = (viewport[0], viewport[1], viewport[2], viewport[3]);
        if vp_w <= 0 || vp_h <= 0 {
            return;
        }

        let (ucr, vcr, wcr, hcr) = (
            crop[0] as GLfloat,
            crop[1] as GLfloat,
            crop[2] as GLfloat,
            crop[3] as GLfloat,
        );
        let (tex_w, tex_h) = (tex_w as GLfloat, tex_h as GLfloat);
        let s0 = ucr / tex_w;
        let s1 = (ucr + wcr) / tex_w;
        let t0 = vcr / tex_h;
        let t1 = (vcr + hcr) / tex_h;

        let (x0, y0) = (x, y);
        let (x1, y1) = (x + width, y + height);
        // `TOUCHHLE_DS_BLIT_LOG=<n>` - calibration aid: log the first n HUD
        // blits with both coordinate spaces. A blit's destination rectangle is
        // already in *window* coordinates, so this is what tells whether the
        // landscape mapping used by the prompt recorder is right (compare with a
        // HUD element whose screen position is known).
        if let Ok(n) = crate::ds_draw_env::var("TOUCHHLE_DS_BLIT_LOG") {
            use std::sync::atomic::{AtomicUsize as BlOrd, Ordering as BlOrdering};
            static BL_N: BlOrd = BlOrd::new(0);
            let cap: usize = n.trim().parse().unwrap_or(40);
            let k = BL_N.fetch_add(1, BlOrdering::Relaxed);
            if k < cap {
                let (fl, ft) = (480.0f32 / vp_h as f32, 320.0f32 / vp_w as f32);
                log!(
                    "[DSblit] #{} tex={} {}x{} win=[{:.0},{:.0}..{:.0},{:.0}] land=[{:.0},{:.0}..{:.0},{:.0}] vp={}x{}",
                    k, texture, tex_w, tex_h,
                    x0, y0, x1, y1,
                    y0 * fl, x0 * ft, y1 * fl, x1 * ft,
                    vp_w, vp_h
                );
            }
        }
        // Data-coordinate prompt recording for HUD blits.
        //
        // The in-game HUD is drawn with `glDrawTexfOES` (see the note further
        // down), whose destination rectangle is already in window coordinates -
        // so unlike the mesh path there is nothing to un-project: the blit rect
        // *is* the prompt's screen box. `TOUCHHLE_DS_PROMPT_AT="x,y[,max_span[,tol]]"`
        // therefore also applies here: if the requested landscape point falls
        // inside a small blit, record the blit's centre through the same
        // `ds_set_prompt_xy` channel the E key reads.
        //
        // Landscape points: the colour buffer is a portrait target that the
        // compositor rotates 90 degrees (same convention as the tracer), so
        // landscape X = GL-y * 480/vh and landscape Y = GL-x * 320/vw.
        if let Ok(spec) = crate::ds_draw_env::var("TOUCHHLE_DS_PROMPT_AT") {
            let mut it = spec.split(',');
            let txo = it.next().and_then(|s| s.trim().parse::<f32>().ok());
            let tyo = it.next().and_then(|s| s.trim().parse::<f32>().ok());
            let max_span = it
                .next()
                .and_then(|s| s.trim().parse::<f32>().ok())
                .unwrap_or(160.0);
            if let (Some(tx), Some(ty)) = (txo, tyo) {
                let (fl, ft) = (480.0f32 / vp_h as f32, 320.0f32 / vp_w as f32);
                let (lx0, lx1) = (y0 * fl, y1 * fl);
                let (ly0, ly1) = (x0 * ft, x1 * ft);
                let span = (lx1 - lx0).max(ly1 - ly0);
                let want_box = crate::ds_draw_env::var("TOUCHHLE_DS_PROMPT_AT_BOX")
                    .ok()
                    .and_then(|s| {
                        let v: Vec<f32> = s
                            .split(',')
                            .filter_map(|t| t.trim().parse::<f32>().ok())
                            .collect();
                        if v.len() == 4 {
                            Some((v[0], v[1], v[2], v[3]))
                        } else {
                            None
                        }
                    });
                let in_box = want_box.map_or(true, |(bx0, by0, bx1, by1)| {
                    lx1 >= bx0 && lx0 <= bx1 && ly1 >= by0 && ly0 <= by1
                });
                if span > 0.0 && span <= max_span && in_box {
                    let inside = tx >= lx0 && tx <= lx1 && ty >= ly0 && ty <= ly1;
                    if inside {
                        let cx = (lx0 + lx1) * 0.5;
                        let cy = (ly0 + ly1) * 0.5;
                        {
                            use std::sync::atomic::{AtomicUsize, Ordering as BTord};
                            static BSEEN: AtomicUsize = AtomicUsize::new(0);
                            if BSEEN.fetch_add(1, BTord::Relaxed) < 24 {
                                log!(
                                    "[DSpromptBlit] tex={} {}x{} rect=[{:.0},{:.0}..{:.0},{:.0}] span={:.1} -> ({:.1},{:.1})",
                                    texture, tex_w, tex_h, lx0, ly0, lx1, ly1, span, cx, cy
                                );
                            }
                        }
                        crate::window::ds_set_prompt_xy(cx, cy);
                    }
                }
            }
        }
        let zc = if z < 0.0 {
            0.0
        } else if z > 1.0 {
            1.0
        } else {
            z
        };

        // Save everything we disturb. Client array state is saved wholesale so
        // the caller's enabled arrays and pointers survive the draw.
        gl21::PushClientAttrib(gl21::CLIENT_VERTEX_ARRAY_BIT);

        let cull_was_enabled = gl21::IsEnabled(gl21::CULL_FACE) != 0;
        if cull_was_enabled {
            gl21::Disable(gl21::CULL_FACE);
        }

        // Window coordinates map directly onto the viewport, and the depth
        // range defaults to [0, 1] which matches the ortho near/far below.
        gl21::MatrixMode(gl21::PROJECTION);
        gl21::PushMatrix();
        gl21::LoadIdentity();
        gl21::Ortho(
            vp_x as f64,
            (vp_x + vp_w) as f64,
            vp_y as f64,
            (vp_y + vp_h) as f64,
            0.0,
            1.0,
        );

        gl21::MatrixMode(gl21::MODELVIEW);
        gl21::PushMatrix();
        gl21::LoadIdentity();

        // The texture matrix must not affect the result.
        gl21::MatrixMode(gl21::TEXTURE);
        gl21::PushMatrix();
        gl21::LoadIdentity();

        let vertices: [GLfloat; 12] = [x0, y0, zc, x1, y0, zc, x0, y1, zc, x1, y1, zc];
        let tex_coords: [GLfloat; 8] = [s0, t0, s1, t0, s0, t1, s1, t1];
        gl21::EnableClientState(gl21::VERTEX_ARRAY);
        gl21::VertexPointer(3, gl21::FLOAT, 0, vertices.as_ptr() as *const GLvoid);
        gl21::EnableClientState(gl21::TEXTURE_COORD_ARRAY);
        gl21::TexCoordPointer(2, gl21::FLOAT, 0, tex_coords.as_ptr() as *const GLvoid);
        // The HUD in this game goes through `glDrawTexfOES`, not through
        // DrawArrays/DrawElements - so the per-draw diagnostic hooks have to be
        // applied here too, otherwise `REDTEX`/`TEXNODEPTH`/texture tracing
        // silently skip every HUD sprite. (Found 2026-10-09: a REDTEX_HASH run
        // painted a muzzle flash but never the locator ring.)
        let redtex = redtex_begin(4);
        let texnodepth = texnodepth_begin(4);
        gl21::DrawArrays(gl21::TRIANGLE_STRIP, 0, 4);
        texnodepth_end(texnodepth);
        redtex_end(redtex);

        gl21::MatrixMode(gl21::TEXTURE);
        gl21::PopMatrix();
        gl21::MatrixMode(gl21::MODELVIEW);
        gl21::PopMatrix();
        gl21::MatrixMode(gl21::PROJECTION);
        gl21::PopMatrix();

        if cull_was_enabled {
            gl21::Enable(gl21::CULL_FACE);
        }

        gl21::PopClientAttrib();
    }
    unsafe fn GenTextures(&mut self, n: GLsizei, textures: *mut GLuint) {
        gl21::GenTextures(n, textures)
    }
    unsafe fn DeleteTextures(&mut self, n: GLsizei, textures: *const GLuint) {
        gl21::DeleteTextures(n, textures)
    }
    unsafe fn ActiveTexture(&mut self, texture: GLenum) {
        self.state.scene.active(texture);
        gl21::ActiveTexture(texture)
    }
    unsafe fn IsTexture(&mut self, texture: GLuint) -> GLboolean {
        gl21::IsTexture(texture)
    }
    unsafe fn BindTexture(&mut self, target: GLenum, texture: GLuint) {
        assert!(target == gl21::TEXTURE_2D);
        gl21::BindTexture(target, texture)
    }
    unsafe fn TexParameteri(&mut self, target: GLenum, pname: GLenum, param: GLint) {
        assert!(target == gl21::TEXTURE_2D);
        if UNSUPPORTED_TEX_PARAMS.contains(pname) {
            log_dbg!(
                "Tolerating TexParameteri({:#x}, {:#x}) of parameter",
                target,
                pname
            );
        } else {
            TEX_PARAMS.assert_known_param(pname);
        }
        gl21::TexParameteri(target, pname, param);
    }
    unsafe fn TexParameterf(&mut self, target: GLenum, pname: GLenum, param: GLfloat) {
        assert!(target == gl21::TEXTURE_2D);
        TEX_PARAMS.assert_known_param(pname);
        gl21::TexParameterf(target, pname, param);
    }
    unsafe fn TexParameterx(&mut self, target: GLenum, pname: GLenum, param: GLfixed) {
        assert!(target == gl21::TEXTURE_2D);
        TEX_PARAMS.setx(
            |param| gl21::TexParameterf(target, pname, param),
            |param| gl21::TexParameteri(target, pname, param),
            pname,
            param,
        )
    }
    unsafe fn TexParameteriv(&mut self, target: GLenum, pname: GLenum, params: *const GLint) {
        assert!(target == gl21::TEXTURE_2D);
        if pname == gles11::TEXTURE_CROP_RECT_OES {
            let values = [
                params.read_unaligned(),
                params.add(1).read_unaligned(),
                params.add(2).read_unaligned(),
                params.add(3).read_unaligned(),
            ];
            self.set_texture_crop_rect(values);
            return;
        }
        TEX_PARAMS.assert_known_param(pname);
        gl21::TexParameteriv(target, pname, params);
    }
    unsafe fn TexParameterfv(&mut self, target: GLenum, pname: GLenum, params: *const GLfloat) {
        assert!(target == gl21::TEXTURE_2D);
        if pname == gles11::TEXTURE_CROP_RECT_OES {
            let values = [
                params.read_unaligned() as GLint,
                params.add(1).read_unaligned() as GLint,
                params.add(2).read_unaligned() as GLint,
                params.add(3).read_unaligned() as GLint,
            ];
            self.set_texture_crop_rect(values);
            return;
        }
        TEX_PARAMS.assert_known_param(pname);
        gl21::TexParameterfv(target, pname, params);
    }
    unsafe fn TexParameterxv(&mut self, target: GLenum, pname: GLenum, params: *const GLfixed) {
        assert!(target == gl21::TEXTURE_2D);
        if pname == gles11::TEXTURE_CROP_RECT_OES {
            let values = [
                fixed_to_float(params.read_unaligned()) as GLint,
                fixed_to_float(params.add(1).read_unaligned()) as GLint,
                fixed_to_float(params.add(2).read_unaligned()) as GLint,
                fixed_to_float(params.add(3).read_unaligned()) as GLint,
            ];
            self.set_texture_crop_rect(values);
            return;
        }
        TEX_PARAMS.setxv(
            |params| gl21::TexParameterfv(target, pname, params),
            |params| gl21::TexParameteriv(target, pname, params),
            pname,
            params,
        )
    }
    unsafe fn TexImage2D(
        &mut self,
        target: GLenum,
        level: GLint,
        internalformat: GLint,
        width: GLsizei,
        height: GLsizei,
        border: GLint,
        format: GLenum,
        type_: GLenum,
        pixels: *const GLvoid,
    ) {
        assert!(target == gl21::TEXTURE_2D);
        assert!(level >= 0);
        assert!(
            internalformat as GLenum == gl21::ALPHA
                || internalformat as GLenum == gl21::RGB
                || internalformat as GLenum == gl21::RGBA
                || internalformat as GLenum == gl21::LUMINANCE
                || internalformat as GLenum == gl21::LUMINANCE_ALPHA
        );
        assert!(border == 0);
        assert!(
            format == gl21::ALPHA
                || format == gl21::RGB
                || format == gl21::RGBA
                || format == gl21::LUMINANCE
                || format == gl21::LUMINANCE_ALPHA
                || format == gl21::BGRA
        );
        assert!(
            type_ == gl21::UNSIGNED_BYTE
                || type_ == gl21::UNSIGNED_SHORT_5_6_5
                || type_ == gl21::UNSIGNED_SHORT_4_4_4_4
                || type_ == gl21::UNSIGNED_SHORT_5_5_5_1
        );
        // Dead Space rasterises its UI text into an ALPHA-only texture and draws
        // it with a texture environment that takes the colour from the texture
        // (the in-world prompt inside the cyan ring). GLES 1.1's ALPHA base
        // format has RGB = 1, but *desktop* OpenGL gives an ALPHA texture an RGB
        // of zero, so here the glyphs come out black - invisible on the dark
        // door - while the ring around them still draws. Expand ALPHA uploads to
        // RGBA with RGB = 255 so both sampling and the texture environment
        // behave like the device. `TOUCHHLE_DS_ALPHA_AS_WHITE=0` restores the
        // pass-through for A/B.
        let alpha_as_white = crate::ds_draw_env::var("TOUCHHLE_DS_ALPHA_AS_WHITE")
            .map_or(false, |v| v == "1" || v == "true")
            || crate::ds_draw_env::contains("TOUCHHLE_DS_ALPHA_TINT");
        // Diagnostic: expand ALPHA uploads with an arbitrary tint instead of
        // white, so "is this drawn from an ALPHA texture at all?" can be
        // answered by looking for a colour that cannot occur naturally. Set
        // TOUCHHLE_DS_ALPHA_TINT="r,g,b" (e.g. "255,0,255").
        let alpha_tint: Option<[u8; 3]> =
            crate::ds_draw_env::var("TOUCHHLE_DS_ALPHA_TINT").ok().and_then(|s| {
                let v: Vec<u8> = s.split(',').filter_map(|t| t.trim().parse().ok()).collect();
                if v.len() == 3 {
                    Some([v[0], v[1], v[2]])
                } else {
                    None
                }
            });
        if alpha_as_white
            && internalformat as GLenum == gl21::ALPHA
            && format == gl21::ALPHA
            && type_ == gl21::UNSIGNED_BYTE
            && !pixels.is_null()
            && width > 0
            && height > 0
        {
            let n = (width as usize) * (height as usize);
            let src = std::slice::from_raw_parts(pixels as *const u8, n);
            let tint = alpha_tint.unwrap_or([255u8, 255u8, 255u8]);
            let mut rgba: Vec<u8> = Vec::with_capacity(n * 4);
            for &a in src {
                rgba.extend_from_slice(&[tint[0], tint[1], tint[2], a]);
            }
            return gl21::TexImage2D(
                target,
                level,
                gl21::RGBA as GLint,
                width,
                height,
                border,
                gl21::RGBA,
                gl21::UNSIGNED_BYTE,
                rgba.as_ptr() as *const GLvoid,
            );
        }
        gl21::TexImage2D(
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
        assert!(target == gl21::TEXTURE_2D);
        assert!(level >= 0);
        assert!(
            format == gl21::ALPHA
                || format == gl21::RGB
                || format == gl21::RGBA
                || format == gl21::LUMINANCE
                || format == gl21::LUMINANCE_ALPHA
                || format == gl21::BGRA
        );
        assert!(
            type_ == gl21::UNSIGNED_BYTE
                || type_ == gl21::UNSIGNED_SHORT_5_6_5
                || type_ == gl21::UNSIGNED_SHORT_4_4_4_4
                || type_ == gl21::UNSIGNED_SHORT_5_5_5_1
        );
        gl21::TexSubImage2D(
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
        let data = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), image_size as usize) };
        // IMG_texture_compression_pvrtc (only on Imagination/Apple GPUs)
        // TODO: It would be more efficient to use hardware decoding where
        // available (I just don't have a suitable device to try this on)
        if try_decode_pvrtc(
            self,
            target,
            level,
            internalformat,
            width,
            height,
            border,
            data,
        ) {
            log_dbg!("Decoded PVRTC");
        // OES_compressed_paletted_texture is only in OpenGL ES, so we'll need
        // to decompress those formats.
        } else if let Some(PalettedTextureFormat {
            index_is_nibble,
            palette_entry_format,
            palette_entry_type,
        }) = PalettedTextureFormat::get_info(internalformat)
        {
            // This should be invalid use? (TODO)
            assert!(border == 0);

            let palette_entry_size = match palette_entry_type {
                gl21::UNSIGNED_BYTE => match palette_entry_format {
                    gl21::RGB => 3,
                    gl21::RGBA => 4,
                    _ => unreachable!(),
                },
                gl21::UNSIGNED_SHORT_5_6_5
                | gl21::UNSIGNED_SHORT_4_4_4_4
                | gl21::UNSIGNED_SHORT_5_5_5_1 => 2,
                _ => unreachable!(),
            };
            let palette_entry_count = match index_is_nibble {
                true => 16,
                false => 256,
            };
            let palette_size = palette_entry_size * palette_entry_count;

            let index_count = width as usize * height as usize;
            let (index_word_size, index_word_count) = match index_is_nibble {
                true => (1, index_count.div_ceil(2)),
                false => (4, index_count.div_ceil(4)),
            };
            let indices_size = index_word_size * index_word_count;

            // TODO: support multiple miplevels in one image
            assert!(level == 0);
            assert_eq!(data.len(), palette_size + indices_size);
            let (palette, indices) = data.split_at(palette_size);

            let mut decoded = Vec::<u8>::with_capacity(palette_entry_size * index_count);
            for i in 0..index_count {
                let index = if index_is_nibble {
                    (indices[i / 2] >> ((1 - (i % 2)) * 4)) & 0xf
                } else {
                    indices[i]
                } as usize;
                let palette_entry = &palette[index * palette_entry_size..][..palette_entry_size];
                decoded.extend_from_slice(palette_entry);
            }
            assert!(decoded.len() == palette_entry_size * index_count);

            log_dbg!("Decoded paletted texture");
            gl21::TexImage2D(
                target,
                level,
                palette_entry_format as _,
                width,
                height,
                border,
                palette_entry_format,
                palette_entry_type,
                decoded.as_ptr() as *const _,
            )
        } else {
            log!(
                "Warning: CompressedTexImage2D: unsupported internalformat {:#x}; skipping upload.",
                internalformat
            );
        }
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
        assert!(target == gl21::TEXTURE_2D);
        assert!(level >= 0);
        // PVRTC sub-image updates are very rare (Apple's OpenGL ES 1.1
        // surface rejects them too), but if we ever see one we
        // software-decode the entire sub-region to RGBA and use the
        // uncompressed sub-image path. Paletted formats are not legal here
        // per the OES_compressed_paletted_texture spec.
        let data_slice = if data.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(data.cast::<u8>(), image_size as usize)
        };
        let is_pvrtc_2bit = matches!(
            format,
            gles11::COMPRESSED_RGB_PVRTC_2BPPV1_IMG | gles11::COMPRESSED_RGBA_PVRTC_2BPPV1_IMG
        );
        let is_pvrtc_4bit = matches!(
            format,
            gles11::COMPRESSED_RGB_PVRTC_4BPPV1_IMG | gles11::COMPRESSED_RGBA_PVRTC_4BPPV1_IMG
        );
        if is_pvrtc_2bit || is_pvrtc_4bit {
            let Ok(width_u) = u32::try_from(width) else {
                log!("Warning: CompressedTexSubImage2D: invalid width {width}; skipping.");
                return;
            };
            let Ok(height_u) = u32::try_from(height) else {
                log!("Warning: CompressedTexSubImage2D: invalid height {height}; skipping.");
                return;
            };
            let pixels = crate::image::decode_pvrtc(data_slice, is_pvrtc_2bit, width_u, height_u);
            gl21::TexSubImage2D(
                target,
                level,
                xoffset,
                yoffset,
                width,
                height,
                gl21::RGBA,
                gl21::UNSIGNED_BYTE,
                pixels.as_ptr() as *const _,
            );
            return;
        }
        // Forward any format the desktop driver natively understands.
        gl21::CompressedTexSubImage2D(
            target, level, xoffset, yoffset, width, height, format, image_size, data,
        )
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
        assert!(target == gl21::TEXTURE_2D);
        assert!(level >= 0);
        assert!(
            internalformat as GLenum == gl21::ALPHA
                || internalformat as GLenum == gl21::RGB
                || internalformat as GLenum == gl21::RGBA
                || internalformat as GLenum == gl21::LUMINANCE
                || internalformat as GLenum == gl21::LUMINANCE_ALPHA
        );
        assert!(border == 0);
        gl21::CopyTexImage2D(target, level, internalformat, x, y, width, height, border)
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
        assert!(target == gl21::TEXTURE_2D);
        assert!(level >= 0);
        gl21::CopyTexSubImage2D(target, level, xoffset, yoffset, x, y, width, height)
    }
    unsafe fn TexEnvf(&mut self, target: GLenum, pname: GLenum, param: GLfloat) {
        self.state.scene.env(target,pname,param as u32);
        match target {
            gl21::TEXTURE_ENV => {
                TEX_ENV_PARAMS.assert_component_count(pname, 1);
                gl21::TexEnvf(target, pname, param)
            }
            gl21::TEXTURE_FILTER_CONTROL_EXT => {
                assert!(pname == gl21::TEXTURE_LOD_BIAS_EXT);
                gl21::TexEnvf(target, pname, param)
            }
            gl21::POINT_SPRITE => {
                assert!(pname == gl21::COORD_REPLACE);
                gl21::TexEnvf(target, pname, param)
            }
            gl21::TEXTURE_2D => {
                // This is not a valid TexEnvf target, but we're tolerating it
                // for a Driver case.
                assert_eq!(pname, gl21::TEXTURE_ENV_MODE);
                log_dbg!(
                    "Tolerating glTexEnvf(GL_TEXTURE_2D, TEXTURE_ENV_MODE, {})",
                    param
                );
                gl21::TexEnvf(target, pname, param)
            }
            _ => {
                log!(
                    "Warning: TexEnvf: unsupported target {:#x}; ignoring call.",
                    target
                );
            }
        }
    }
    unsafe fn TexEnvx(&mut self, target: GLenum, pname: GLenum, param: GLfixed) {
        match target {
            gl21::TEXTURE_ENV => TEX_ENV_PARAMS.setx(
                |param| gl21::TexEnvf(target, pname, param),
                |param| gl21::TexEnvi(target, pname, param),
                pname,
                param,
            ),
            gl21::TEXTURE_FILTER_CONTROL_EXT => {
                assert!(pname == gl21::TEXTURE_LOD_BIAS_EXT);
                gl21::TexEnvf(target, pname, fixed_to_float(param))
            }
            gl21::POINT_SPRITE => {
                assert!(pname == gl21::COORD_REPLACE);
                gl21::TexEnvf(target, pname, fixed_to_float(param))
            }
            _ => {
                log!(
                    "Warning: TexEnvx: unsupported target {:#x}; ignoring call.",
                    target
                );
            }
        }
    }
    unsafe fn TexEnvi(&mut self, target: GLenum, pname: GLenum, param: GLint) {
        self.state.scene.env(target,pname,param as u32);
        match target {
            gl21::TEXTURE_ENV => {
                TEX_ENV_PARAMS.assert_component_count(pname, 1);
                gl21::TexEnvi(target, pname, param)
            }
            gl21::TEXTURE_FILTER_CONTROL_EXT => {
                assert!(pname == gl21::TEXTURE_LOD_BIAS_EXT);
                gl21::TexEnvi(target, pname, param)
            }
            gl21::POINT_SPRITE => {
                assert!(pname == gl21::COORD_REPLACE);
                gl21::TexEnvi(target, pname, param)
            }
            gl21::TEXTURE_2D => {
                // This is not a valid TexEnvi target, but we're tolerating it
                // for a Rayman 2 case.
                assert!(pname == gl21::TEXTURE_ENV_MODE);
                log_dbg!(
                    "Tolerating glTexEnvi(GL_TEXTURE_2D, TEXTURE_ENV_MODE, {})",
                    param
                );
                gl21::TexEnvi(target, pname, param)
            }
            _ => {
                log!(
                    "Warning: TexEnvi: unsupported target 0x{:X}, pname 0x{:X}; ignoring call.",
                    target,
                    pname
                );
            }
        }
    }
    unsafe fn TexEnvfv(&mut self, target: GLenum, pname: GLenum, params: *const GLfloat) {
        if target == gles11::TEXTURE_FILTER_CONTROL_EXT {
            assert!(pname == gl21::TEXTURE_LOD_BIAS_EXT);
            unsafe {
                if !CStr::from_ptr(gl21::GetString(gl21::EXTENSIONS) as _)
                    .to_str()
                    .unwrap()
                    .contains("EXT_texture_lod_bias")
                {
                    log_dbg!("GL_EXT_texture_lod_bias is unsupported, skipping TexEnvfv({:#x}, {:#x}, ...) call", target, pname);
                    return;
                }
            };
        }
        match target {
            gl21::TEXTURE_ENV => {
                TEX_ENV_PARAMS.assert_known_param(pname);
                gl21::TexEnvfv(target, pname, params)
            }
            gl21::TEXTURE_FILTER_CONTROL_EXT => {
                assert!(pname == gl21::TEXTURE_LOD_BIAS_EXT);
                gl21::TexEnvfv(target, pname, params)
            }
            gl21::POINT_SPRITE => {
                assert!(pname == gl21::COORD_REPLACE);
                gl21::TexEnvfv(target, pname, params)
            }
            _ => {
                log!(
                    "Warning: TexEnvfv: unsupported target {:#x}; ignoring call.",
                    target
                );
            }
        }
    }
    unsafe fn TexEnvxv(&mut self, target: GLenum, pname: GLenum, params: *const GLfixed) {
        match target {
            gl21::TEXTURE_ENV => TEX_ENV_PARAMS.setxv(
                |params| gl21::TexEnvfv(target, pname, params),
                |params| gl21::TexEnviv(target, pname, params),
                pname,
                params,
            ),
            gl21::TEXTURE_FILTER_CONTROL_EXT => {
                assert!(pname == gl21::TEXTURE_LOD_BIAS_EXT);
                let param = fixed_to_float(params.read());
                gl21::TexEnvfv(target, pname, &param)
            }
            gl21::POINT_SPRITE => {
                assert!(pname == gl21::COORD_REPLACE);
                let param = fixed_to_float(params.read());
                gl21::TexEnvfv(target, pname, &param)
            }
            _ => {
                log!(
                    "Warning: TexEnvxv: unsupported target {:#x}; ignoring call.",
                    target
                );
            }
        }
    }
    unsafe fn TexEnviv(&mut self, target: GLenum, pname: GLenum, params: *const GLint) {
        match target {
            gl21::TEXTURE_ENV => {
                TEX_ENV_PARAMS.assert_known_param(pname);
                gl21::TexEnviv(target, pname, params)
            }
            gl21::TEXTURE_FILTER_CONTROL_EXT => {
                assert!(pname == gl21::TEXTURE_LOD_BIAS_EXT);
                gl21::TexEnviv(target, pname, params)
            }
            gl21::POINT_SPRITE => {
                assert!(pname == gl21::COORD_REPLACE);
                gl21::TexEnviv(target, pname, params)
            }
            _ => {
                log!(
                    "Warning: TexEnviv: unsupported target {:#x}; ignoring call.",
                    target
                );
            }
        }
    }

    unsafe fn MultiTexCoord4f(
        &mut self,
        target: GLenum,
        s: GLfloat,
        t: GLfloat,
        r: GLfloat,
        q: GLfloat,
    ) {
        gl21::MultiTexCoord4f(target, s, t, r, q)
    }
    unsafe fn MultiTexCoord4x(
        &mut self,
        target: GLenum,
        s: GLfixed,
        t: GLfixed,
        r: GLfixed,
        q: GLfixed,
    ) {
        gl21::MultiTexCoord4f(
            target,
            fixed_to_float(s),
            fixed_to_float(t),
            fixed_to_float(r),
            fixed_to_float(q),
        )
    }

    // Matrix stack operations
    unsafe fn MatrixMode(&mut self, mode: GLenum) {
        let new_mode = match mode {
            gl21::MODELVIEW => MatrixModeState::ModelView,
            gl21::PROJECTION => MatrixModeState::Projection,
            gl21::TEXTURE => MatrixModeState::Texture,
            // GL_MATRIX_PALETTE_OES == GL_MATRIX_PALETTE_ARB == 0x8840, from
            // OES_matrix_palette. Subsequent matrix-stack operations target the
            // palette slot selected by glCurrentPaletteMatrixOES. We emulate the
            // palette CPU-side (desktop GL 2.1 / Mesa do not expose working
            // fixed-function palette skinning), so don't forward this to the
            // host MatrixMode (it would raise GL_INVALID_ENUM).
            gles11::MATRIX_PALETTE_OES => MatrixModeState::MatrixPalette,
            _ => {
                log!(
                    "Warning: glMatrixMode({:#x}) selected an unrecognized matrix mode; \
                     ignoring matrix-stack writes until a supported mode is selected",
                    mode
                );
                self.state.matrix_mode = MatrixModeState::MatrixPalette;
                return;
            }
        };
        self.state.matrix_mode = new_mode;
        if self.state.matrix_mode == MatrixModeState::ModelView {
            ds_mv_trace_log("MATRIX_MODE ModelView");
        }
        if self.state.matrix_mode == MatrixModeState::Texture {
            ds_texmat_trace_log("MATRIX_MODE Texture");
        }
        if new_mode != MatrixModeState::MatrixPalette {
            gl21::MatrixMode(mode);
        }
    }
    unsafe fn CurrentPaletteMatrixOES(&mut self, matrixpaletteindex: GLuint) {
        // INVALID_VALUE if index >= MAX_PALETTE_MATRICES_OES; clamp instead of
        // crashing so a misbehaving guest can't take the emulator down.
        if (matrixpaletteindex as usize) >= self.state.palette_matrices.len() {
            log!(
                "Warning: glCurrentPaletteMatrixOES({}) out of range (max {}); clamping",
                matrixpaletteindex,
                self.state.palette_matrices.len()
            );
            self.state.current_palette_matrix =
                (self.state.palette_matrices.len() as GLuint).saturating_sub(1);
            return;
        }
        self.state.current_palette_matrix = matrixpaletteindex;
    }
    unsafe fn LoadPaletteFromModelViewMatrixOES(&mut self) {
        // Copy the live GL MODELVIEW matrix into the current palette slot.
        let mut modelview = [0.0f32; 16];
        gl21::GetFloatv(gl21::MODELVIEW_MATRIX, modelview.as_mut_ptr());
        let idx = self.state.current_palette_matrix as usize;
        if let Some(slot) = self.state.palette_matrices.get_mut(idx) {
            *slot = modelview;
        }
    }
    unsafe fn LoadIdentity(&mut self) {
        if self.state.matrix_mode == MatrixModeState::Projection
            && crate::ds_draw_env::contains("TOUCHHLE_DS_PROJ_TRACE")
        {
            log!("[PROJTRACE] LoadIdentity");
        }
        if self.state.matrix_mode == MatrixModeState::ModelView {
            ds_mv_trace_log("LoadIdentity");
        }
        if self.state.matrix_mode == MatrixModeState::Texture {
            ds_texmat_trace_log("LoadIdentity");
        }
        if self.state.matrix_mode == MatrixModeState::MatrixPalette {
            let idx = self.state.current_palette_matrix as usize;
            if let Some(slot) = self.state.palette_matrices.get_mut(idx) {
                *slot = MATRIX_IDENTITY;
            }
            return;
        }
        gl21::LoadIdentity();
    }
    unsafe fn LoadMatrixf(&mut self, m: *const GLfloat) {
        if self.state.matrix_mode == MatrixModeState::Projection
            && crate::ds_draw_env::contains("TOUCHHLE_DS_PROJ_TRACE")
        {
            let vals = std::slice::from_raw_parts(m, 16);
            log!("[PROJTRACE] LoadMatrixf {:?}", vals);
        }
        if self.state.matrix_mode == MatrixModeState::ModelView {
            let vals = std::slice::from_raw_parts(m, 16);
            ds_mv_trace_log(format_args!("LoadMatrixf {:?}", vals));
        }
        if self.state.matrix_mode == MatrixModeState::Texture {
            let vals = std::slice::from_raw_parts(m, 16);
            ds_texmat_trace_log(format_args!("LoadMatrixf {:?}", vals));
        }
        if self.state.matrix_mode == MatrixModeState::MatrixPalette {
            let idx = self.state.current_palette_matrix as usize;
            if let Some(slot) = self.state.palette_matrices.get_mut(idx) {
                for (i, cell) in slot.iter_mut().enumerate() {
                    *cell = m.add(i).read_unaligned();
                }
            }
            return;
        }
        gl21::LoadMatrixf(m);
    }
    unsafe fn LoadMatrixx(&mut self, m: *const GLfixed) {
        let matrix = matrix_fixed_to_float(m);
        self.LoadMatrixf(matrix.as_ptr());
    }
    unsafe fn MultMatrixf(&mut self, m: *const GLfloat) {
        if self.state.matrix_mode == MatrixModeState::Projection
            && crate::ds_draw_env::contains("TOUCHHLE_DS_PROJ_TRACE")
        {
            let vals = std::slice::from_raw_parts(m, 16);
            log!("[PROJTRACE] MultMatrixf {:?}", vals);
        }
        if self.state.matrix_mode == MatrixModeState::ModelView {
            let vals = std::slice::from_raw_parts(m, 16);
            ds_mv_trace_log(format_args!("MultMatrixf {:?}", vals));
        }
        if self.state.matrix_mode == MatrixModeState::Texture {
            let vals = std::slice::from_raw_parts(m, 16);
            ds_texmat_trace_log(format_args!("MultMatrixf {:?}", vals));
        }
        if self.state.matrix_mode == MatrixModeState::MatrixPalette {
            let idx = self.state.current_palette_matrix as usize;
            if let Some(slot) = self.state.palette_matrices.get(idx).copied() {
                let mut rhs = [0.0f32; 16];
                for (i, cell) in rhs.iter_mut().enumerate() {
                    *cell = m.add(i).read_unaligned();
                }
                let product = mat4_multiply(&slot, &rhs);
                if let Some(dst) = self.state.palette_matrices.get_mut(idx) {
                    *dst = product;
                }
            }
            return;
        }
        gl21::MultMatrixf(m);
    }
    unsafe fn MultMatrixx(&mut self, m: *const GLfixed) {
        let matrix = matrix_fixed_to_float(m);
        self.MultMatrixf(matrix.as_ptr());
    }
    unsafe fn PushMatrix(&mut self) {
        if self.state.matrix_mode == MatrixModeState::MatrixPalette {
            // The palette has no matrix stack (OES_matrix_palette issue #5);
            // ignore push/pop while in palette mode.
            return;
        }
        gl21::PushMatrix();
    }
    unsafe fn PopMatrix(&mut self) {
        if self.state.matrix_mode == MatrixModeState::MatrixPalette {
            return;
        }
        gl21::PopMatrix();
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
        if crate::ds_draw_env::contains("TOUCHHLE_DS_PROJ_TRACE") {
            log!(
                "[PROJTRACE] Orthof {} {} {} {} {} {}",
                left,
                right,
                bottom,
                top,
                near,
                far
            );
        }
        gl21::Ortho(
            left.into(),
            right.into(),
            bottom.into(),
            top.into(),
            near.into(),
            far.into(),
        );
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
        gl21::Ortho(
            fixed_to_float(left).into(),
            fixed_to_float(right).into(),
            fixed_to_float(bottom).into(),
            fixed_to_float(top).into(),
            fixed_to_float(near).into(),
            fixed_to_float(far).into(),
        );
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
        if crate::ds_draw_env::contains("TOUCHHLE_DS_PROJ_TRACE") {
            log!(
                "[PROJTRACE] Frustumf {} {} {} {} {} {}",
                left,
                right,
                bottom,
                top,
                near,
                far
            );
        }
        let far = ds_adjust_opening_frustum_far(near, far);
        let far = if let Ok(v) = crate::ds_draw_env::var("TOUCHHLE_DS_FRUSTUM_FAR_SCALE") {
            v.parse::<f32>().map(|s| far * s).unwrap_or(far)
        } else {
            far
        };
        gl21::Frustum(
            left.into(),
            right.into(),
            bottom.into(),
            top.into(),
            near.into(),
            far.into(),
        );
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
        let mut far_f = ds_adjust_opening_frustum_far(near_f, fixed_to_float(far));
        if let Ok(v) = crate::ds_draw_env::var("TOUCHHLE_DS_FRUSTUM_FAR_SCALE") {
            if let Ok(s) = v.parse::<f32>() {
                far_f *= s;
            }
        }
        gl21::Frustum(
            fixed_to_float(left).into(),
            fixed_to_float(right).into(),
            fixed_to_float(bottom).into(),
            fixed_to_float(top).into(),
            near_f.into(),
            far_f.into(),
        );
    }
    unsafe fn Rotatef(&mut self, angle: GLfloat, x: GLfloat, y: GLfloat, z: GLfloat) {
        if self.state.matrix_mode == MatrixModeState::ModelView {
            ds_mv_trace_log(format_args!("Rotatef angle, x, y, z"));
        }
        gl21::Rotatef(angle, x, y, z);
    }
    unsafe fn Rotatex(&mut self, angle: GLfixed, x: GLfixed, y: GLfixed, z: GLfixed) {
        gl21::Rotatef(
            fixed_to_float(angle),
            fixed_to_float(x),
            fixed_to_float(y),
            fixed_to_float(z),
        );
    }
    unsafe fn Scalef(&mut self, x: GLfloat, y: GLfloat, z: GLfloat) {
        if self.state.matrix_mode == MatrixModeState::ModelView {
            ds_mv_trace_log(format_args!("Scalef x, y, z"));
        }
        gl21::Scalef(x, y, z);
    }
    unsafe fn Scalex(&mut self, x: GLfixed, y: GLfixed, z: GLfixed) {
        gl21::Scalef(fixed_to_float(x), fixed_to_float(y), fixed_to_float(z));
    }
    unsafe fn Translatef(&mut self, x: GLfloat, y: GLfloat, z: GLfloat) {
        if self.state.matrix_mode == MatrixModeState::ModelView {
            ds_mv_trace_log(format_args!("Translatef x, y, z"));
        }
        gl21::Translatef(x, y, z);
    }
    unsafe fn Translatex(&mut self, x: GLfixed, y: GLfixed, z: GLfixed) {
        gl21::Translatef(fixed_to_float(x), fixed_to_float(y), fixed_to_float(z));
    }

    // OES_framebuffer_object -> EXT_framebuffer_object
    unsafe fn GenFramebuffersOES(&mut self, n: GLsizei, framebuffers: *mut GLuint) {
        gl21::GenFramebuffersEXT(n, framebuffers)
    }
    unsafe fn GenRenderbuffersOES(&mut self, n: GLsizei, renderbuffers: *mut GLuint) {
        gl21::GenRenderbuffersEXT(n, renderbuffers)
    }
    unsafe fn IsFramebufferOES(&mut self, renderbuffer: GLuint) -> GLboolean {
        gl21::IsFramebufferEXT(renderbuffer)
    }
    unsafe fn IsRenderbufferOES(&mut self, renderbuffer: GLuint) -> GLboolean {
        gl21::IsRenderbufferEXT(renderbuffer)
    }
    unsafe fn BindFramebufferOES(&mut self, target: GLenum, framebuffer: GLuint) {
        gl21::BindFramebufferEXT(target, framebuffer)
    }
    unsafe fn BindRenderbufferOES(&mut self, target: GLenum, renderbuffer: GLuint) {
        gl21::BindRenderbufferEXT(target, renderbuffer)
    }
    unsafe fn RenderbufferStorageOES(
        &mut self,
        target: GLenum,
        internalformat: GLenum,
        width: GLsizei,
        height: GLsizei,
    ) {
        // Permanent correctness repair, user accepted 2026-10-30 (R4).
        // Keep this independent of AA, resolution and performance presets;
        // PC_DEPTH24=0 is an explicit diagnostic bypass, not a quality preset.
        // The game replaces the drawable's auto-depth with its own D16 FBO.
        // SDL_GL_DEPTH_SIZE=24 only covers the host window. The PC MSAA path
        // happens to allocate D24, so MSAA=off used to expose the old deck
        // quantization again. Match the native GLES1 D16->D24 repair without
        // changing geometry, depth tests or the user's antialiasing setting.
        let requested_d16 = target == gl21::RENDERBUFFER_EXT
            && internalformat == 0x81a5 && width > 0 && height > 0;
        if requested_d16 && crate::ds_draw_env::var("TOUCHHLE_DS_PC_DEPTH24").as_deref() == Ok("1") {
            gl21::RenderbufferStorageEXT(target, 0x81a6, width, height);
            let mut bits = 0;
            gl21::GetRenderbufferParameterivEXT(target, 0x8d54, &mut bits);
            if bits >= 24 {
                log!("[DSpcDepth] guest D16 promoted to D24: {}x{} actual_bits={}", width, height, bits);
                return;
            }
            log!("[DSpcDepth] D24 unavailable (bits={}); retaining guest D16", bits);
        }
        gl21::RenderbufferStorageEXT(target, internalformat, width, height);
        if requested_d16 {
            let mut bits = 0;
            gl21::GetRenderbufferParameterivEXT(target, 0x8d54, &mut bits);
            log!("[DSpcDepth] guest D16 retained: {}x{} actual_bits={}", width, height, bits);
        }
    }
    unsafe fn FramebufferRenderbufferOES(
        &mut self,
        target: GLenum,
        attachment: GLenum,
        renderbuffertarget: GLenum,
        renderbuffer: GLuint,
    ) {
        gl21::FramebufferRenderbufferEXT(target, attachment, renderbuffertarget, renderbuffer)
    }
    unsafe fn FramebufferTexture2DOES(
        &mut self,
        target: GLenum,
        attachment: GLenum,
        textarget: GLenum,
        texture: GLuint,
        level: i32,
    ) {
        gl21::FramebufferTexture2DEXT(target, attachment, textarget, texture, level)
    }
    unsafe fn GetFramebufferAttachmentParameterivOES(
        &mut self,
        target: GLenum,
        attachment: GLenum,
        pname: GLenum,
        params: *mut GLint,
    ) {
        gl21::GetFramebufferAttachmentParameterivEXT(target, attachment, pname, params)
    }
    unsafe fn GetRenderbufferParameterivOES(
        &mut self,
        target: GLenum,
        pname: GLenum,
        params: *mut GLint,
    ) {
        gl21::GetRenderbufferParameterivEXT(target, pname, params)
    }
    unsafe fn CheckFramebufferStatusOES(&mut self, target: GLenum) -> GLenum {
        gl21::CheckFramebufferStatusEXT(target)
    }
    unsafe fn DeleteFramebuffersOES(&mut self, n: GLsizei, framebuffers: *const GLuint) {
        gl21::DeleteFramebuffersEXT(n, framebuffers)
    }
    unsafe fn DeleteRenderbuffersOES(&mut self, n: GLsizei, renderbuffers: *const GLuint) {
        gl21::DeleteRenderbuffersEXT(n, renderbuffers)
    }
    unsafe fn GenerateMipmapOES(&mut self, target: GLenum) {
        gl21::GenerateMipmapEXT(target)
    }

    // GL_APPLE_framebuffer_multisample → GL_EXT_framebuffer_multisample +
    // GL_EXT_framebuffer_blit, which are baseline on every desktop GL that
    // can host this layer.
    unsafe fn RenderbufferStorageMultisampleAPPLE(
        &mut self,
        target: GLenum,
        samples: GLsizei,
        internalformat: GLenum,
        width: GLsizei,
        height: GLsizei,
    ) {
        gl21::RenderbufferStorageMultisampleEXT(target, samples, internalformat, width, height)
    }
    unsafe fn ResolveMultisampleFramebufferAPPLE(&mut self) {
        // Apple's GL_APPLE_framebuffer_multisample doesn't take any arguments:
        // the source is whatever is currently bound to GL_READ_FRAMEBUFFER_APPLE
        // and the destination is whatever is currently bound to
        // GL_DRAW_FRAMEBUFFER_APPLE. Their numeric values are identical to
        // GL_READ_FRAMEBUFFER_EXT / GL_DRAW_FRAMEBUFFER_EXT, so we can hand
        // them straight to glBlitFramebufferEXT.
        //
        // Figure out the rectangle to blit from the READ framebuffer's color
        // attachment so that the blit covers exactly the rendered area.
        let mut color_rb: GLint = 0;
        gl21::GetFramebufferAttachmentParameterivEXT(
            gl21::READ_FRAMEBUFFER_EXT,
            gl21::COLOR_ATTACHMENT0_EXT,
            gl21::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME_EXT,
            &mut color_rb,
        );
        // Remember and restore the renderbuffer binding so we don't perturb
        // whatever the guest expects to be current.
        let mut old_rb: GLint = 0;
        gl21::GetIntegerv(gl21::RENDERBUFFER_BINDING_EXT, &mut old_rb);
        gl21::BindRenderbufferEXT(gl21::RENDERBUFFER_EXT, color_rb as GLuint);
        let mut width: GLint = 0;
        let mut height: GLint = 0;
        gl21::GetRenderbufferParameterivEXT(
            gl21::RENDERBUFFER_EXT,
            gl21::RENDERBUFFER_WIDTH_EXT,
            &mut width,
        );
        gl21::GetRenderbufferParameterivEXT(
            gl21::RENDERBUFFER_EXT,
            gl21::RENDERBUFFER_HEIGHT_EXT,
            &mut height,
        );
        gl21::BindRenderbufferEXT(gl21::RENDERBUFFER_EXT, old_rb as GLuint);

        gl21::BlitFramebufferEXT(
            0,
            0,
            width,
            height,
            0,
            0,
            width,
            height,
            gl21::COLOR_BUFFER_BIT,
            gl21::NEAREST,
        );
    }

    // Non-OES aliases for OES_framebuffer_object functions.
    // Some GLES1 apps call the suffix-free ES2-style names directly.
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
        gl21::GetBufferParameteriv(target, pname, params)
    }
    unsafe fn MapBufferOES(&mut self, target: GLenum, access: GLenum) -> *mut GLvoid {
        gl21::MapBuffer(target, access)
    }
    unsafe fn UnmapBufferOES(&mut self, target: GLenum) -> GLboolean {
        gl21::UnmapBuffer(target)
    }

    // OpenGL ES 2.0 entry points implemented on top of OpenGL 2.1's shader
    // pipeline. ES 2.0's GLSL 1.00 source is translated to desktop GLSL 1.20
    // by [super::gles2_glsl] before being passed to the driver.
    unsafe fn CreateShader(&mut self, type_: GLenum) -> GLuint {
        gl21::CreateShader(type_)
    }
    unsafe fn DeleteShader(&mut self, shader: GLuint) {
        gl21::DeleteShader(shader)
    }
    unsafe fn ShaderSource(
        &mut self,
        shader: GLuint,
        count: GLsizei,
        string: *const *const super::gles_generic::GLchar,
        length: *const GLint,
    ) {
        // Concatenate the GLSL ES source into one string, translate it to
        // desktop GLSL 1.20, and submit it as a single source string.
        let mut combined = String::new();
        for i in 0..count as usize {
            let str_ptr = *string.add(i);
            if str_ptr.is_null() {
                continue;
            }
            let bytes: &[u8] = if !length.is_null() && *length.add(i) >= 0 {
                let len = *length.add(i) as usize;
                std::slice::from_raw_parts(str_ptr as *const u8, len)
            } else {
                std::ffi::CStr::from_ptr(str_ptr).to_bytes()
            };
            combined.push_str(&String::from_utf8_lossy(bytes));
        }
        let translated = super::gles2_glsl::translate_glsl_es_to_120(&combined);
        let cstr = std::ffi::CString::new(translated).unwrap_or_default();
        let ptr = cstr.as_ptr();
        let len = cstr.as_bytes().len() as GLint;
        gl21::ShaderSource(shader, 1, &ptr, &len);
    }
    unsafe fn CompileShader(&mut self, shader: GLuint) {
        let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::ShaderCompile,1,shader as u64);
        let _profile = crate::ds_perf::Timer::new(crate::ds_perf::Stage::ShaderCompile);
        gl21::CompileShader(shader);
    }
    unsafe fn GetShaderiv(&mut self, shader: GLuint, pname: GLenum, params: *mut GLint) {
        gl21::GetShaderiv(shader, pname, params);
    }
    unsafe fn GetShaderInfoLog(
        &mut self,
        shader: GLuint,
        maxLength: GLsizei,
        length: *mut GLsizei,
        infoLog: *mut super::gles_generic::GLchar,
    ) {
        gl21::GetShaderInfoLog(shader, maxLength, length, infoLog);
    }
    unsafe fn IsShader(&mut self, shader: GLuint) -> GLboolean {
        gl21::IsShader(shader)
    }
    unsafe fn CreateProgram(&mut self) -> GLuint {
        gl21::CreateProgram()
    }
    unsafe fn DeleteProgram(&mut self, program: GLuint) {
        gl21::DeleteProgram(program);
    }
    unsafe fn AttachShader(&mut self, program: GLuint, shader: GLuint) {
        gl21::AttachShader(program, shader);
    }
    unsafe fn DetachShader(&mut self, program: GLuint, shader: GLuint) {
        gl21::DetachShader(program, shader);
    }
    unsafe fn LinkProgram(&mut self, program: GLuint) {
        let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::ShaderLink,1,program as u64);
        let _profile = crate::ds_perf::Timer::new(crate::ds_perf::Stage::ShaderLink);
        gl21::LinkProgram(program);
    }
    unsafe fn UseProgram(&mut self, program: GLuint) {
        self.state.scene.set(7,program as u64);
        gl21::UseProgram(program);
    }
    unsafe fn GetProgramiv(&mut self, program: GLuint, pname: GLenum, params: *mut GLint) {
        gl21::GetProgramiv(program, pname, params);
    }
    unsafe fn GetProgramInfoLog(
        &mut self,
        program: GLuint,
        maxLength: GLsizei,
        length: *mut GLsizei,
        infoLog: *mut super::gles_generic::GLchar,
    ) {
        gl21::GetProgramInfoLog(program, maxLength, length, infoLog);
    }
    unsafe fn IsProgram(&mut self, program: GLuint) -> GLboolean {
        gl21::IsProgram(program)
    }
    unsafe fn ValidateProgram(&mut self, program: GLuint) {
        gl21::ValidateProgram(program);
    }
    unsafe fn BindAttribLocation(
        &mut self,
        program: GLuint,
        index: GLuint,
        name: *const super::gles_generic::GLchar,
    ) {
        gl21::BindAttribLocation(program, index, name);
    }
    unsafe fn GetAttribLocation(
        &mut self,
        program: GLuint,
        name: *const super::gles_generic::GLchar,
    ) -> GLint {
        gl21::GetAttribLocation(program, name)
    }
    unsafe fn GetUniformLocation(
        &mut self,
        program: GLuint,
        name: *const super::gles_generic::GLchar,
    ) -> GLint {
        gl21::GetUniformLocation(program, name)
    }
    unsafe fn GetActiveAttrib(
        &mut self,
        program: GLuint,
        index: GLuint,
        bufSize: GLsizei,
        length: *mut GLsizei,
        size: *mut GLint,
        type_: *mut GLenum,
        name: *mut super::gles_generic::GLchar,
    ) {
        gl21::GetActiveAttrib(program, index, bufSize, length, size, type_, name);
    }
    unsafe fn GetActiveUniform(
        &mut self,
        program: GLuint,
        index: GLuint,
        bufSize: GLsizei,
        length: *mut GLsizei,
        size: *mut GLint,
        type_: *mut GLenum,
        name: *mut super::gles_generic::GLchar,
    ) {
        gl21::GetActiveUniform(program, index, bufSize, length, size, type_, name);
    }
    unsafe fn EnableVertexAttribArray(&mut self, index: GLuint) {
        gl21::EnableVertexAttribArray(index);
    }
    unsafe fn DisableVertexAttribArray(&mut self, index: GLuint) {
        gl21::DisableVertexAttribArray(index);
    }
    unsafe fn VertexAttribPointer(
        &mut self,
        index: GLuint,
        size: GLint,
        type_: GLenum,
        normalized: GLboolean,
        stride: GLsizei,
        pointer: *const GLvoid,
    ) {
        gl21::VertexAttribPointer(index, size, type_, normalized, stride, pointer);
    }
    unsafe fn VertexAttrib1f(&mut self, index: GLuint, x: GLfloat) {
        gl21::VertexAttrib1f(index, x);
    }
    unsafe fn VertexAttrib2f(&mut self, index: GLuint, x: GLfloat, y: GLfloat) {
        gl21::VertexAttrib2f(index, x, y);
    }
    unsafe fn VertexAttrib3f(&mut self, index: GLuint, x: GLfloat, y: GLfloat, z: GLfloat) {
        gl21::VertexAttrib3f(index, x, y, z);
    }
    unsafe fn VertexAttrib4f(
        &mut self,
        index: GLuint,
        x: GLfloat,
        y: GLfloat,
        z: GLfloat,
        w: GLfloat,
    ) {
        gl21::VertexAttrib4f(index, x, y, z, w);
    }
    unsafe fn VertexAttrib1fv(&mut self, index: GLuint, v: *const GLfloat) {
        gl21::VertexAttrib1fv(index, v);
    }
    unsafe fn VertexAttrib2fv(&mut self, index: GLuint, v: *const GLfloat) {
        gl21::VertexAttrib2fv(index, v);
    }
    unsafe fn VertexAttrib3fv(&mut self, index: GLuint, v: *const GLfloat) {
        gl21::VertexAttrib3fv(index, v);
    }
    unsafe fn VertexAttrib4fv(&mut self, index: GLuint, v: *const GLfloat) {
        gl21::VertexAttrib4fv(index, v);
    }
    unsafe fn Uniform1f(&mut self, location: GLint, v0: GLfloat) {
        gl21::Uniform1f(location, v0);
    }
    unsafe fn Uniform2f(&mut self, location: GLint, v0: GLfloat, v1: GLfloat) {
        gl21::Uniform2f(location, v0, v1);
    }
    unsafe fn Uniform3f(&mut self, location: GLint, v0: GLfloat, v1: GLfloat, v2: GLfloat) {
        gl21::Uniform3f(location, v0, v1, v2);
    }
    unsafe fn Uniform4f(
        &mut self,
        location: GLint,
        v0: GLfloat,
        v1: GLfloat,
        v2: GLfloat,
        v3: GLfloat,
    ) {
        gl21::Uniform4f(location, v0, v1, v2, v3);
    }
    unsafe fn Uniform1i(&mut self, location: GLint, v0: GLint) {
        gl21::Uniform1i(location, v0);
    }
    unsafe fn Uniform2i(&mut self, location: GLint, v0: GLint, v1: GLint) {
        gl21::Uniform2i(location, v0, v1);
    }
    unsafe fn Uniform3i(&mut self, location: GLint, v0: GLint, v1: GLint, v2: GLint) {
        gl21::Uniform3i(location, v0, v1, v2);
    }
    unsafe fn Uniform4i(&mut self, location: GLint, v0: GLint, v1: GLint, v2: GLint, v3: GLint) {
        gl21::Uniform4i(location, v0, v1, v2, v3);
    }
    unsafe fn Uniform1fv(&mut self, location: GLint, count: GLsizei, value: *const GLfloat) {
        gl21::Uniform1fv(location, count, value);
    }
    unsafe fn Uniform2fv(&mut self, location: GLint, count: GLsizei, value: *const GLfloat) {
        gl21::Uniform2fv(location, count, value);
    }
    unsafe fn Uniform3fv(&mut self, location: GLint, count: GLsizei, value: *const GLfloat) {
        gl21::Uniform3fv(location, count, value);
    }
    unsafe fn Uniform4fv(&mut self, location: GLint, count: GLsizei, value: *const GLfloat) {
        gl21::Uniform4fv(location, count, value);
    }
    unsafe fn Uniform1iv(&mut self, location: GLint, count: GLsizei, value: *const GLint) {
        gl21::Uniform1iv(location, count, value);
    }
    unsafe fn Uniform2iv(&mut self, location: GLint, count: GLsizei, value: *const GLint) {
        gl21::Uniform2iv(location, count, value);
    }
    unsafe fn Uniform3iv(&mut self, location: GLint, count: GLsizei, value: *const GLint) {
        gl21::Uniform3iv(location, count, value);
    }
    unsafe fn Uniform4iv(&mut self, location: GLint, count: GLsizei, value: *const GLint) {
        gl21::Uniform4iv(location, count, value);
    }
    unsafe fn UniformMatrix2fv(
        &mut self,
        location: GLint,
        count: GLsizei,
        transpose: GLboolean,
        value: *const GLfloat,
    ) {
        gl21::UniformMatrix2fv(location, count, transpose, value);
    }
    unsafe fn UniformMatrix3fv(
        &mut self,
        location: GLint,
        count: GLsizei,
        transpose: GLboolean,
        value: *const GLfloat,
    ) {
        gl21::UniformMatrix3fv(location, count, transpose, value);
    }
    unsafe fn UniformMatrix4fv(
        &mut self,
        location: GLint,
        count: GLsizei,
        transpose: GLboolean,
        value: *const GLfloat,
    ) {
        gl21::UniformMatrix4fv(location, count, transpose, value);
    }
    unsafe fn BlendColor(&mut self, r: GLclampf, g: GLclampf, b: GLclampf, a: GLclampf) {
        gl21::BlendColor(r, g, b, a);
    }
    unsafe fn BlendEquation(&mut self, mode: GLenum) {
        gl21::BlendEquation(mode);
    }
    unsafe fn BlendEquationSeparate(&mut self, modeRGB: GLenum, modeAlpha: GLenum) {
        gl21::BlendEquationSeparate(modeRGB, modeAlpha);
    }
    unsafe fn BlendFuncSeparate(
        &mut self,
        srcRGB: GLenum,
        dstRGB: GLenum,
        srcAlpha: GLenum,
        dstAlpha: GLenum,
    ) {
        gl21::BlendFuncSeparate(srcRGB, dstRGB, srcAlpha, dstAlpha);
    }
    unsafe fn StencilFuncSeparate(
        &mut self,
        face: GLenum,
        func: GLenum,
        ref_: GLint,
        mask: GLuint,
    ) {
        gl21::StencilFuncSeparate(face, func, ref_, mask);
    }
    unsafe fn StencilOpSeparate(
        &mut self,
        face: GLenum,
        sfail: GLenum,
        dpfail: GLenum,
        dppass: GLenum,
    ) {
        gl21::StencilOpSeparate(face, sfail, dpfail, dppass);
    }
    unsafe fn StencilMaskSeparate(&mut self, face: GLenum, mask: GLuint) {
        gl21::StencilMaskSeparate(face, mask);
    }
    unsafe fn GetVertexAttribiv(&mut self, index: GLuint, pname: GLenum, params: *mut GLint) {
        gl21::GetVertexAttribiv(index, pname, params);
    }
    unsafe fn GetVertexAttribfv(&mut self, index: GLuint, pname: GLenum, params: *mut GLfloat) {
        gl21::GetVertexAttribfv(index, pname, params);
    }
    unsafe fn GetVertexAttribPointerv(
        &mut self,
        index: GLuint,
        pname: GLenum,
        pointer: *mut *mut GLvoid,
    ) {
        gl21::GetVertexAttribPointerv(index, pname, pointer);
    }
    unsafe fn GetUniformiv(&mut self, program: GLuint, location: GLint, params: *mut GLint) {
        gl21::GetUniformiv(program, location, params);
    }
    unsafe fn GetUniformfv(&mut self, program: GLuint, location: GLint, params: *mut GLfloat) {
        gl21::GetUniformfv(program, location, params);
    }
    unsafe fn GetAttachedShaders(
        &mut self,
        program: GLuint,
        maxCount: GLsizei,
        count: *mut GLsizei,
        shaders: *mut GLuint,
    ) {
        gl21::GetAttachedShaders(program, maxCount, count, shaders);
    }
    unsafe fn GetShaderSource(
        &mut self,
        shader: GLuint,
        bufSize: GLsizei,
        length: *mut GLsizei,
        source: *mut super::gles_generic::GLchar,
    ) {
        gl21::GetShaderSource(shader, bufSize, length, source);
    }
    unsafe fn ReleaseShaderCompiler(&mut self) {
        // Desktop GL doesn't have ReleaseShaderCompiler; this is a hint and
        // safe to ignore.
    }
    unsafe fn GetShaderPrecisionFormat(
        &mut self,
        _shadertype: GLenum,
        precisiontype: GLenum,
        range: *mut GLint,
        precision: *mut GLint,
    ) {
        // Desktop GL 2.1 lacks this entry point. Report IEEE-754 single
        // precision floating point ranges and full integer ranges, which
        // matches the behaviour of typical desktop drivers.
        if !range.is_null() {
            let (rmin, rmax) = match precisiontype {
                gl21::INT_VEC2 // sentinel; we use the actual GL_LOW_INT etc.
                | 0x8DF3 /* GL_LOW_INT */ | 0x8DF4 /* GL_MEDIUM_INT */
                | 0x8DF5 /* GL_HIGH_INT */ => (31, 30),
                _ => (127, 127), // float types
            };
            *range.add(0) = rmin;
            *range.add(1) = rmax;
        }
        if !precision.is_null() {
            *precision = match precisiontype {
                0x8DF3..=0x8DF5 /* GL_HIGH_INT */ => 0,
                _ => 23, // mantissa bits
            };
        }
    }
    unsafe fn ShaderBinary(
        &mut self,
        _count: GLsizei,
        _shaders: *const GLuint,
        _binaryformat: GLenum,
        _binary: *const GLvoid,
        _length: GLsizei,
    ) {
        // Desktop GL 2.1 has no shader binary format we can pass through;
        // signal failure via GL_INVALID_ENUM.
        gl21::GetError(); // discard prior error
                          // GL has no direct way to set INVALID_ENUM, but
                          // issuing an invalid
                          // call achieves it. Easiest: call Enable with an
                          // invalid cap.
        gl21::Enable(0);
    }
}

#[cfg(test)]
mod matrix_palette_tests {
    use super::{mat4_multiply, mat4_transform, MATRIX_IDENTITY};

    #[test]
    fn identity_transform_is_noop() {
        let v = [1.5, -2.0, 3.25, 1.0];
        assert_eq!(mat4_transform(&MATRIX_IDENTITY, v), v);
    }

    #[test]
    fn identity_multiply_is_noop() {
        // A non-trivial column-major matrix.
        let m = [
            1.0, 2.0, 3.0, 4.0, //
            5.0, 6.0, 7.0, 8.0, //
            9.0, 10.0, 11.0, 12.0, //
            13.0, 14.0, 15.0, 16.0, //
        ];
        assert_eq!(mat4_multiply(&MATRIX_IDENTITY, &m), m);
        assert_eq!(mat4_multiply(&m, &MATRIX_IDENTITY), m);
    }

    #[test]
    fn translation_transform_matches_gl_convention() {
        // Column-major translation by (10, 20, 30): translation in last column.
        let t = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            10.0, 20.0, 30.0, 1.0, //
        ];
        let v = [1.0, 2.0, 3.0, 1.0];
        assert_eq!(mat4_transform(&t, v), [11.0, 22.0, 33.0, 1.0]);
    }

    #[test]
    fn multiply_then_transform_equals_sequential_transforms() {
        // Translate by (1,0,0) then scale by 2 about origin.
        let translate = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            1.0, 0.0, 0.0, 1.0, //
        ];
        let scale = [
            2.0, 0.0, 0.0, 0.0, //
            0.0, 2.0, 0.0, 0.0, //
            0.0, 0.0, 2.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, //
        ];
        // scale * translate applied to v == scale(translate(v))
        let combined = mat4_multiply(&scale, &translate);
        let v = [3.0, 4.0, 5.0, 1.0];
        let sequential = mat4_transform(&scale, mat4_transform(&translate, v));
        assert_eq!(mat4_transform(&combined, v), sequential);
    }

    #[test]
    fn weighted_blend_of_two_matrices() {
        // Two translations; blending with weights 0.5/0.5 yields the average.
        let a = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let mut b = a;
        b[12] = 10.0; // translate x by 10
        let v = [0.0, 0.0, 0.0, 1.0];
        let ta = mat4_transform(&a, v);
        let tb = mat4_transform(&b, v);
        let mut blended = [0.0f32; 4];
        for c in 0..4 {
            blended[c] = 0.5 * ta[c] + 0.5 * tb[c];
        }
        assert_eq!(blended, [5.0, 0.0, 0.0, 1.0]);
    }
}

#[cfg(test)]
mod capability_tests {
    use super::{capability_supported, CAPABILITIES};

    #[test]
    fn fast_capability_dispatch_matches_declared_table() {
        for &cap in CAPABILITIES {
            assert!(capability_supported(cap), "missing capability {cap:#x}");
        }
        assert!(!capability_supported(0));
        assert!(!capability_supported(0xffff_ffff));
    }
}

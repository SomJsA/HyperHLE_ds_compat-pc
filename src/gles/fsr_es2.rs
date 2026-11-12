/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! Shader-backed final-frame processing for Android's native ES 1.1 backend.
//!
//! Android's GLES1Native backend cannot compile shaders, but the hardware can
//! still run a tiny private ES 2.0 context. That context shares objects with
//! the guest ES 1.1 context, so the already-uploaded present texture can be
//! consumed by the FSR1/CAS shader without a CPU readback or an extra frame
//! copy. The guest context is restored before returning.

use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;

use touchHLE_gl_bindings::gles2;

use crate::gles::gles2_native::GLES2Native;
use crate::gles::gles_generic::GLES;
use crate::gles::present;

type EGLBoolean = u32;
type EGLenum = u32;
type EGLint = i32;
type EGLDisplay = *mut c_void;
type EGLConfig = *mut c_void;
type EGLSurface = *mut c_void;
type EGLContext = *mut c_void;

const EGL_FALSE: EGLBoolean = 0;
const EGL_NONE: EGLint = 0x3038;
const EGL_OPENGL_ES_API: EGLenum = 0x30a0;
const EGL_OPENGL_ES2_BIT: EGLint = 0x0004;
const EGL_OPENGL_ES3_BIT_KHR: EGLint = 0x0040;
const EGL_CONTEXT_CLIENT_VERSION: EGLint = 0x3098;
const EGL_DRAW: EGLint = 0x3059;
const EGL_READ: EGLint = 0x305a;
const EGL_CONFIG_ID: EGLint = 0x3028;
const EGL_WIDTH: EGLint = 0x3057;
const EGL_HEIGHT: EGLint = 0x3056;
const EGL_SURFACE_TYPE: EGLint = 0x3033;
const EGL_WINDOW_BIT: EGLint = 0x0004;
const EGL_PBUFFER_BIT: EGLint = 0x0001;
const EGL_RENDERABLE_TYPE: EGLint = 0x3040;
const EGL_RED_SIZE: EGLint = 0x3024;
const EGL_GREEN_SIZE: EGLint = 0x3023;
const EGL_BLUE_SIZE: EGLint = 0x3022;
const EGL_ALPHA_SIZE: EGLint = 0x3021;

#[link(name = "EGL")]
extern "C" {
    fn eglGetCurrentDisplay() -> EGLDisplay;
    fn eglGetCurrentSurface(readdraw: EGLint) -> EGLSurface;
    fn eglGetCurrentContext() -> EGLContext;
    fn eglQuerySurface(
        dpy: EGLDisplay,
        surface: EGLSurface,
        attribute: EGLint,
        value: *mut EGLint,
    ) -> EGLBoolean;
    fn eglChooseConfig(
        dpy: EGLDisplay,
        attrib_list: *const EGLint,
        configs: *mut EGLConfig,
        config_size: EGLint,
        num_config: *mut EGLint,
    ) -> EGLBoolean;
    fn eglBindAPI(api: EGLenum) -> EGLBoolean;
    fn eglCreateContext(
        dpy: EGLDisplay,
        config: EGLConfig,
        share_context: EGLContext,
        attrib_list: *const EGLint,
    ) -> EGLContext;
    fn eglCreatePbufferSurface(
        dpy: EGLDisplay,
        config: EGLConfig,
        attrib_list: *const EGLint,
    ) -> EGLSurface;
    fn eglDestroyContext(dpy: EGLDisplay, ctx: EGLContext) -> EGLBoolean;
    fn eglDestroySurface(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;
    fn eglMakeCurrent(
        dpy: EGLDisplay,
        draw: EGLSurface,
        read: EGLSurface,
        ctx: EGLContext,
    ) -> EGLBoolean;
    fn eglGetError() -> EGLint;
    fn eglGetProcAddress(procname: *const c_char) -> *const c_void;
}

#[derive(Clone)]
struct FsrContext {
    present_state: std::rc::Rc<RefCell<present::PresentState>>,
    display: EGLDisplay,
    draw_surface: EGLSurface,
    read_surface: EGLSurface,
    guest_context: EGLContext,
    shader_context: EGLContext,
    depth_context: EGLContext,
    depth_surface: EGLSurface,
}

thread_local! {
    static FSR_CONTEXT: RefCell<Option<FsrContext>> = const { RefCell::new(None) };
}

fn requested() -> bool {
    let fsr = std::env::var("TOUCHHLE_DS_PRESENT_FSR")
        .ok()
        .and_then(|v| v.trim().parse::<u8>().ok())
        .unwrap_or(0)
        > 0;
    let sharpen = std::env::var("TOUCHHLE_DS_PRESENT_SHARPEN")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(0.0)
        > 0.0;
    let aa = std::env::var("TOUCHHLE_DS_AA")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false);
    let fsr3_ahb = std::env::var("TOUCHHLE_DS_FSR3_AHB").as_deref() == Ok("1");
    fsr || sharpen || aa || fsr3_ahb
}

fn load_gl(name: &'static str) -> *const c_void {
    let Ok(c_name) = CString::new(name) else {
        return ptr::null();
    };
    let mut proc = unsafe { eglGetProcAddress(c_name.as_ptr()) };
    if proc.is_null() {
        proc = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c_name.as_ptr()) };
    }
    proc
}

unsafe fn query_surface_int(display: EGLDisplay, surface: EGLSurface, attr: EGLint) -> Option<i32> {
    let mut value = 0;
    if eglQuerySurface(display, surface, attr, &mut value) == EGL_FALSE {
        None
    } else {
        Some(value)
    }
}

unsafe fn choose_config_for_surface(display: EGLDisplay, surface: EGLSurface) -> Option<EGLConfig> {
    let mut config_id = 0;
    if eglQuerySurface(display, surface, EGL_CONFIG_ID, &mut config_id) != EGL_FALSE {
        let attrs = [EGL_CONFIG_ID, config_id, EGL_NONE];
        let mut config = ptr::null_mut();
        let mut count = 0;
        if eglChooseConfig(display, attrs.as_ptr(), &mut config, 1, &mut count) != EGL_FALSE
            && count == 1
            && !config.is_null()
        {
            return Some(config);
        }
    }

    // Some Android EGL implementations reject EGL_CONFIG_ID in
    // eglChooseConfig even though they return it from eglQuerySurface. Fall
    // back to the standard RGBA8888 ES2 window configuration.
    let attrs = [
        EGL_SURFACE_TYPE,
        EGL_WINDOW_BIT,
        EGL_RENDERABLE_TYPE,
        EGL_OPENGL_ES2_BIT,
        EGL_RED_SIZE,
        8,
        EGL_GREEN_SIZE,
        8,
        EGL_BLUE_SIZE,
        8,
        EGL_ALPHA_SIZE,
        8,
        EGL_NONE,
    ];
    let mut config = ptr::null_mut();
    let mut count = 0;
    if eglChooseConfig(display, attrs.as_ptr(), &mut config, 1, &mut count) != EGL_FALSE
        && count == 1
        && !config.is_null()
    {
        Some(config)
    } else {
        None
    }
}

unsafe fn choose_es3_config(display: EGLDisplay) -> Option<EGLConfig> {
    let attrs = [
        EGL_SURFACE_TYPE,
        EGL_PBUFFER_BIT,
        EGL_RENDERABLE_TYPE,
        EGL_OPENGL_ES3_BIT_KHR,
        EGL_RED_SIZE,
        8,
        EGL_GREEN_SIZE,
        8,
        EGL_BLUE_SIZE,
        8,
        EGL_ALPHA_SIZE,
        8,
        EGL_NONE,
    ];
    let mut config = ptr::null_mut();
    let mut count = 0;
    if eglChooseConfig(display, attrs.as_ptr(), &mut config, 1, &mut count) != EGL_FALSE
        && count == 1
        && !config.is_null()
    {
        Some(config)
    } else {
        None
    }
}

unsafe fn create_es3_depth_context(
    display: EGLDisplay,
    guest_context: EGLContext,
) -> Option<(EGLContext, EGLSurface)> {
    let config = choose_es3_config(display)?;
    let pbuffer_attrs = [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE];
    let surface = eglCreatePbufferSurface(display, config, pbuffer_attrs.as_ptr());
    if surface.is_null() {
        log!(
            "[DSdepth] eglCreatePbufferSurface(ES3) failed: {:#x}",
            eglGetError()
        );
        return None;
    }
    let context_attrs = [EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE];
    let context = eglCreateContext(display, config, guest_context, context_attrs.as_ptr());
    if context.is_null() {
        log!(
            "[DSdepth] eglCreateContext(ES3 depth) failed: {:#x}",
            eglGetError()
        );
        let _ = eglDestroySurface(display, surface);
        return None;
    }
    Some((context, surface))
}

unsafe fn create_context() -> Option<FsrContext> {
    let display = eglGetCurrentDisplay();
    let draw_surface = eglGetCurrentSurface(EGL_DRAW);
    let read_surface = {
        let surface = eglGetCurrentSurface(EGL_READ);
        if surface.is_null() {
            draw_surface
        } else {
            surface
        }
    };
    let guest_context = eglGetCurrentContext();
    if display.is_null() || draw_surface.is_null() || guest_context.is_null() {
        log!("[DSfsr] no current EGL display/surface/context; shader present unavailable");
        return None;
    }

    let Some(config) = choose_config_for_surface(display, draw_surface) else {
        log!("[DSfsr] no compatible EGL config for private ES2 context");
        return None;
    };

    if eglBindAPI(EGL_OPENGL_ES_API) == EGL_FALSE {
        log!(
            "[DSfsr] eglBindAPI(EGL_OPENGL_ES_API) failed: {:#x}",
            eglGetError()
        );
        return None;
    }

    let es2_attrs = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
    let mut shader_context = eglCreateContext(display, config, guest_context, es2_attrs.as_ptr());
    if shader_context.is_null() {
        log!(
            "[DSfsr] eglCreateContext(ES2 present, share=guest) failed: {:#x}",
            eglGetError()
        );
        return None;
    }
    if eglMakeCurrent(display, draw_surface, read_surface, shader_context) == EGL_FALSE {
        log!(
            "[DSfsr] activating private ES2 present context failed: EGL error {:#x}",
            eglGetError()
        );
        let _ = eglDestroyContext(display, shader_context);
        return None;
    }

    gles2::load_with(load_gl);
    if !gles2::CreateShader::is_loaded()
        || !gles2::CreateProgram::is_loaded()
        || !gles2::GenFramebuffers::is_loaded()
        || !gles2::GetString::is_loaded()
    {
        log!("[DSfsr] private ES2 context is missing required GL entry points");
        let _ = eglMakeCurrent(display, draw_surface, read_surface, guest_context);
        let _ = eglDestroyContext(display, shader_context);
        return None;
    }

    let version = CStr::from_ptr(gles2::GetString(gles2::VERSION) as *const c_char)
        .to_string_lossy()
        .into_owned();
    let renderer = CStr::from_ptr(gles2::GetString(gles2::RENDERER) as *const c_char)
        .to_string_lossy()
        .into_owned();
    let (depth_context, depth_surface) = create_es3_depth_context(display, guest_context)
        .unwrap_or((ptr::null_mut(), ptr::null_mut()));
    log!(
        "[DSfsr] private ES2 present context ready: {} / {}; ES3 depth={}",
        version,
        renderer,
        if depth_context.is_null() {
            "unavailable"
        } else {
            "ready"
        }
    );

    let state = FsrContext {
        present_state: Default::default(),
        display,
        draw_surface,
        read_surface,
        guest_context,
        shader_context,
        depth_context,
        depth_surface,
    };

    let _ = eglMakeCurrent(display, draw_surface, read_surface, guest_context);
    Some(state)
}

fn destroy_cached_context() {
    let stale = FSR_CONTEXT.with(|slot| slot.borrow_mut().take());
    let Some(context) = stale else {
        return;
    };
    unsafe {
        // The private context owns the presentation resources. Delete them
        // while it is current, before releasing the EGL context/sharegroup.
        if eglMakeCurrent(context.display, context.draw_surface, context.read_surface,
                          context.shader_context) != EGL_FALSE {
            gles2::load_with(load_gl);
            let mut gles = GLES2Native::assume_current();
            context.present_state.borrow_mut().release(&mut gles);
            let _ = eglMakeCurrent(context.display, context.draw_surface,
                                   context.read_surface, context.guest_context);
        }
        crate::gles::ds_ahb_bridge::depth_image_cache_reset();
        // GL names and the AHB/EGLImage bridge must be released before the
        // private EGL context. The guest context is current here and shares
        // texture names with the private context on Android EGL.
        crate::gles::ds_ahb_bridge::destroy();
        if !context.depth_surface.is_null() {
            let _ = eglDestroySurface(context.display, context.depth_surface);
        }
        if !context.depth_context.is_null() {
            let _ = eglDestroyContext(context.display, context.depth_context);
        }
        let _ = eglDestroyContext(context.display, context.shader_context);
    }
    log!("[DSfsr] destroyed stale private context and AHB/FSR3 session");
}

fn get_or_create_context() -> Option<FsrContext> {
    let current_draw_surface = unsafe { eglGetCurrentSurface(EGL_DRAW) };
    let current_guest_context = unsafe { eglGetCurrentContext() };
    if let Some(context) = FSR_CONTEXT.with(|slot| slot.borrow().as_ref().cloned()) {
        if context.draw_surface == current_draw_surface
            && context.guest_context == current_guest_context
        {
            return Some(context);
        }
        // SDL destroys and recreates its EGL surface after HOME/resume or a
        // display-mode change. Drop the stale handle; the old EGL objects are
        // owned by SDL/the platform and are already gone.
        destroy_cached_context();
        log!("[DSfsr] surface/context changed; rebuilding private shader context");
    }
    let context = unsafe { create_context() }?;
    FSR_CONTEXT.with(|slot| *slot.borrow_mut() = Some(context.clone()));
    Some(context)
}

/// Read depth from a guest renderbuffer through the private ES2 context.
/// The ES1.1 native driver on Adreno rejects depth readback, but the shared
/// ES3.2 context can read the same FBO/renderbuffer objects.
pub(crate) unsafe fn capture_depth_from_renderbuffer(
    depth_rb: u32,
    width: u32,
    height: u32,
) -> Option<Vec<f32>> {
    if depth_rb == 0 || width == 0 || height == 0 {
        return None;
    }
    let timing_started = std::time::Instant::now();
    let Some(state) = get_or_create_context() else {
        return None;
    };
    if eglGetCurrentContext() != state.guest_context {
        return None;
    }
    if state.depth_context.is_null() || state.depth_surface.is_null() {
        log_once!("[DSdepth] real depth capture requires a private ES3 Pbuffer context");
        return None;
    }

    // Renderbuffer names are not shared between EGL contexts even when the
    // contexts are in the same share group. Wrap the guest renderbuffer with
    // EGL_KHR_gl_renderbuffer_image while the guest context is current, then
    // resolve the image through a renderbuffer owned by the private ES2
    // context.
    let image = crate::gles::ds_ahb_bridge::depth_image_create(depth_rb, width, height);
    if image.is_null() {
        return None;
    }
    if eglMakeCurrent(
        state.display,
        state.depth_surface,
        state.depth_surface,
        state.depth_context,
    ) == EGL_FALSE
    {
        return None;
    }

    gles2::load_with(load_gl);
    crate::gles::ds_ahb_bridge::depth_ahb_probe(width, height);
    let pixel_count = (width * height) as usize;
    let mut depth = vec![0f32; pixel_count];
    let read_ok = crate::gles::ds_ahb_bridge::depth_image_read(image, width, height, &mut depth);
    let restored = eglMakeCurrent(
        state.display,
        state.draw_surface,
        state.read_surface,
        state.guest_context,
    ) != EGL_FALSE;
    if !restored {
        log!("[DSdepth] failed to restore guest context after ES2 depth read");
        return None;
    }
    if !read_ok {
        use std::sync::atomic::{AtomicBool, Ordering};
        static FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);
        if !FAILURE_LOGGED.swap(true, Ordering::Relaxed) {
            log!(
                "[DSdepth] EGLImage depth read failed: depth_rb={} size={}x{}",
                depth_rb,
                width,
                height
            );
        }
        return None;
    }
    use std::sync::atomic::{AtomicU64, Ordering};
    static READ_FRAMES: AtomicU64 = AtomicU64::new(0);
    let frames = READ_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
    if frames == 1 || frames % 120 == 0 {
        log!(
            "[DSdepth] EGLImage depth read active: frame={} depth_rb={} size={}x{} centre={}",
            frames,
            depth_rb,
            width,
            height,
            depth[pixel_count / 2]
        );
    }
    static TIMING_FRAMES: AtomicU64 = AtomicU64::new(0);
    static TIMING_TOTAL_US: AtomicU64 = AtomicU64::new(0);
    let elapsed_us = timing_started.elapsed().as_micros() as u64;
    let timing_frames = TIMING_FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
    let total_us = TIMING_TOTAL_US.fetch_add(elapsed_us, Ordering::Relaxed) + elapsed_us;
    if timing_frames == 1 || timing_frames % 120 == 0 {
        log!(
            "[DSdepth] capture timing: avg {:.2} ms over {} frames",
            total_us as f64 / timing_frames as f64 / 1000.0,
            timing_frames
        );
    }
    Some(depth)
}

pub(crate) unsafe fn depth_image_from_renderbuffer(
    depth_rb: u32,
    width: u32,
    height: u32,
) -> *mut c_void {
    if depth_rb == 0 || width == 0 || height == 0 {
        return ptr::null_mut();
    }
    let Some(state) = get_or_create_context() else {
        return ptr::null_mut();
    };
    if eglGetCurrentContext() != state.guest_context {
        return ptr::null_mut();
    }
    crate::gles::ds_ahb_bridge::depth_image_create(depth_rb, width, height)
}

/// Copy a shared present texture into the AHB-backed texture from the private
/// ES2 context. This is the GLES half of the FSR3 zero-readback bridge.
pub(crate) unsafe fn try_ahb_copy(
    texture: u32,
    width: u32,
    height: u32,
    depth: Option<&[f32]>,
    depth_image: *mut c_void,
    requested_output_width: u32,
    requested_output_height: u32,
    viewport: (u32, u32, u32, u32),
    rotation_matrix: crate::matrix::Matrix<2>,
) -> Option<(u32, u32, u32)> {
    if std::env::var("TOUCHHLE_DS_FSR3_AHB").as_deref() != Ok("1") {
        log_once!("[DSFSR3] AHB branch disabled: TOUCHHLE_DS_FSR3_AHB is no longer 1");
        return None;
    }
    if texture == 0 {
        log_once!("[DSFSR3] AHB branch skipped: present texture is zero");
        return None;
    }

    use std::sync::atomic::{AtomicU64, Ordering};
    static ATTEMPTS: AtomicU64 = AtomicU64::new(0);
    let attempt = ATTEMPTS.fetch_add(1, Ordering::Relaxed) + 1;
    if attempt == 1 || attempt % 120 == 0 {
        log!(
            "[DSFSR3] AHB copy attempt={} source_texture={}",
            attempt,
            texture
        );
    }

    let Some(state) = get_or_create_context() else {
        log_once!("[DSFSR3] AHB copy skipped: no private shared ES2 context");
        return None;
    };
    if eglGetCurrentContext() != state.guest_context {
        log_once!("[DSFSR3] AHB copy skipped: current guest EGL context changed");
        return None;
    }
    if eglMakeCurrent(
        state.display,
        state.draw_surface,
        state.read_surface,
        state.shader_context,
    ) == EGL_FALSE
    {
        log_once!("[DSFSR3] activating private ES2 context failed");
        return None;
    }

    gles2::load_with(load_gl);
    let async_requested =
        std::env::var("TOUCHHLE_DS_FSR3_ASYNC").as_deref() == Ok("1");
    let ready_output = if async_requested {
        log_once!(
            "[DSFSR3] async pipeline enabled: present previous FSR output while current guest frame advances"
        );
        // Complete the prior GL present before reusing its output AHB. The
        // Vulkan fence wait then runs before either the color or depth AHB is
        // overwritten by the current frame.
        gles2::Finish();
        crate::gles::ds_ahb_bridge::fsr3_wait_pending()
    } else {
        None
    };
    let mut depth_ahb = std::ptr::null_mut();
    if !depth_image.is_null() {
        if crate::gles::ds_ahb_bridge::depth_image_copy_to_ahb(
            depth_image,
            width,
            height,
        ) {
            depth_ahb = crate::gles::ds_ahb_bridge::depth_ahb_handle();
        } else {
            log_once!("[DSdepth] GPU depth AHB copy failed; depth disabled this frame");
        }
    }
    let surface_width = query_surface_int(state.display, state.draw_surface, EGL_WIDTH)
        .unwrap_or(width as i32)
        .max(1) as u32;
    let surface_height = query_surface_int(state.display, state.draw_surface, EGL_HEIGHT)
        .unwrap_or(height as i32)
        .max(1) as u32;
    let output_width = requested_output_width.clamp(1, surface_width);
    let output_height = requested_output_height.clamp(1, surface_height);
    let ok = crate::gles::ds_ahb_bridge::init(width, height, output_width, output_height)
        && crate::gles::ds_ahb_bridge::copy_from_texture(texture, width, height);
    if !ok {
        log_once!("[DSFSR3] AHB bridge init/copy failed");
    }
    let diagnostic_readback = std::env::var("TOUCHHLE_DS_FSR3_DIAG_READBACK").as_deref() == Ok("1");
    let pixel = if ok && diagnostic_readback {
        crate::gles::ds_ahb_bridge::verify_pixel()
    } else {
        None
    };

    static COPY_COUNT: AtomicU64 = AtomicU64::new(0);
    let frame = COPY_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if ok && (frame == 1 || frame % 120 == 0) {
        log!(
            "[DSFSR3] AHB GLES bridge: frame={} source={}x{} texture={} centre_pixel={:?}",
            frame,
            width,
            height,
            texture,
            pixel
        );
    }
    if ok && diagnostic_readback && frame == 120 {
        let handle = crate::gles::ds_ahb_bridge::handle();
        let vulkan_ok = crate::gles::ds_ahb_bridge::vulkan_probe(handle);
        log!(
            "[DSFSR3] in-process Vulkan AHB import: handle={:?} success={}",
            handle,
            vulkan_ok
        );
    }
    if ok {
        let upscaled =
            crate::gles::ds_ahb_bridge::fsr3_upscale_with_depth(depth, depth_ahb);
        if frame == 1 || frame % 120 == 0 {
            let has_depth = depth.is_some() || !depth_ahb.is_null();
            log!(
                "[DSFSR3] persistent upscale: frame={} output={}x{} depth={} input_centre={:?} output_centre={:?}",
                frame,
                output_width,
                output_height,
                has_depth,
                pixel,
                upscaled
            );
        }
        if upscaled.is_some() {
            let (output_texture, actual_output_width, actual_output_height) =
                if async_requested {
                    let Some(ready) = ready_output else {
                        log_once!(
                            "[DSFSR3] async pipeline submitted its first frame; using native present until the next frame"
                        );
                        let _ = eglMakeCurrent(
                            state.display,
                            state.draw_surface,
                            state.read_surface,
                            state.guest_context,
                        );
                        return None;
                    };
                    ready
                } else {
                    let texture = crate::gles::ds_ahb_bridge::output_texture();
                    let (width, height) = crate::gles::ds_ahb_bridge::output_size();
                    (texture, width, height)
                };
            if output_texture != 0 {
                if actual_output_width == 0 || actual_output_height == 0 {
                    log_once!("[DSFSR3] bridge reported invalid output dimensions");
                    return None;
                }
                let presented = draw_present_current(
                    &mut state.present_state.borrow_mut(),
                    output_texture,
                    (actual_output_width, actual_output_height),
                    viewport,
                    rotation_matrix,
                );
                let restored = eglMakeCurrent(
                    state.display,
                    state.draw_surface,
                    state.read_surface,
                    state.guest_context,
                ) != EGL_FALSE;
                if !restored {
                    log!("[DSFSR3] failed to restore guest EGL context after FSR present");
                    return None;
                }
                return presented.then_some((
                    output_texture,
                    actual_output_width,
                    actual_output_height,
                ));
            }
            log_once!("[DSFSR3] persistent upscale succeeded but the output GL texture is zero");
        }
    }
    let _ = eglMakeCurrent(
        state.display,
        state.draw_surface,
        state.read_surface,
        state.guest_context,
    );
    None
}

/// Try to present `texture` through the shared ES2 post-processing context.
///
/// Returns `true` only when the shader pass completed and the guest EGL
/// context was restored successfully. Any failure returns `false`, allowing
/// the caller to use the conservative fixed-function present path.
pub(crate) unsafe fn try_present(
    texture: u32,
    source_size: (u32, u32),
    viewport: (u32, u32, u32, u32),
    rotation_matrix: crate::matrix::Matrix<2>,
) -> bool {
    if !requested() || texture == 0 || viewport.2 == 0 || viewport.3 == 0 {
        return false;
    }

    let Some(state) = get_or_create_context() else {
        return false;
    };
    if eglGetCurrentContext() != state.guest_context {
        log_once!("[DSfsr] current guest context changed; shader present disabled for this frame");
        return false;
    }

    let started = std::time::Instant::now();
    if eglMakeCurrent(
        state.display,
        state.draw_surface,
        state.read_surface,
        state.shader_context,
    ) == EGL_FALSE
    {
        log_once!("[DSfsr] activating cached ES2 context failed; using fixed-function present");
        return false;
    }

    let presented = draw_present_current(&mut state.present_state.borrow_mut(), texture, source_size, viewport, rotation_matrix);

    if eglMakeCurrent(
        state.display,
        state.draw_surface,
        state.read_surface,
        state.guest_context,
    ) == EGL_FALSE
    {
        log!(
            "[DSfsr] failed to restore guest EGL context: EGL error {:#x}",
            eglGetError()
        );
        return false;
    }

    use std::sync::atomic::{AtomicU64, Ordering};
    static FRAMES: AtomicU64 = AtomicU64::new(0);
    static TOTAL_US: AtomicU64 = AtomicU64::new(0);
    let elapsed_us = started.elapsed().as_micros() as u64;
    let frames = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
    let total_us = TOTAL_US.fetch_add(elapsed_us, Ordering::Relaxed) + elapsed_us;
    if frames % 120 == 0 {
        log!(
            "[DSfsr] shader pass timing: avg {:.2} ms over {} frames",
            total_us as f64 / frames as f64 / 1000.0,
            frames
        );
    }

    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log!(
            "[DSfsr] shader present active: source={}x{} viewport={}x{}+{},{} fsr={} sharpen={}",
            source_size.0,
            source_size.1,
            viewport.2,
            viewport.3,
            viewport.0,
            viewport.1,
            std::env::var("TOUCHHLE_DS_PRESENT_FSR").unwrap_or_else(|_| "0".into()),
            std::env::var("TOUCHHLE_DS_PRESENT_SHARPEN").unwrap_or_else(|_| "0".into()),
        );
    }
    presented
}

unsafe fn draw_present_current(
    present_state: &mut present::PresentState,
    texture: u32,
    source_size: (u32, u32),
    viewport: (u32, u32, u32, u32),
    rotation_matrix: crate::matrix::Matrix<2>,
) -> bool {
    // The raw function table is global. Reload before drawing: another guest
    // EAGL context may have replaced it since the previous activation.
    gles2::load_with(load_gl);
    gles2::BindFramebuffer(gles2::FRAMEBUFFER, 0);
    gles2::Disable(gles2::SCISSOR_TEST);
    gles2::Disable(gles2::DEPTH_TEST);
    gles2::Disable(gles2::STENCIL_TEST);
    gles2::Disable(gles2::BLEND);
    gles2::Disable(gles2::CULL_FACE);
    gles2::Viewport(
        viewport.0 as _,
        viewport.1 as _,
        viewport.2 as _,
        viewport.3 as _,
    );
    gles2::ClearColor(0.0, 0.0, 0.0, 1.0);
    gles2::Clear(gles2::COLOR_BUFFER_BIT);
    gles2::ActiveTexture(gles2::TEXTURE0);
    gles2::BindTexture(gles2::TEXTURE_2D, texture as _);

    let mut gles = GLES2Native::assume_current();
    let _ = gles.GetError();
    let r = crate::matrix::Matrix::<4>::from(&rotation_matrix);
    let to_center = crate::matrix::Matrix::<4>::translate_3d(-0.5, -0.5, 0.0);
    let from_center = crate::matrix::Matrix::<4>::translate_3d(0.5, 0.5, 0.0);
    let centered_rotation = to_center.multiply(&r).multiply(&from_center);
    let shader_ok = present::draw_present_sharpen_quad(
        &mut gles,
        present_state,
        &centered_rotation,
        source_size,
        0.0,
        0.0,
        0.0,
        None,
    );
    let skip_present_finish = !crate::ds_perf_control::readback_finish_enabled();
    if skip_present_finish {
        log_once!("[DSfsr] skipping FSR present glFinish (diagnostic)");
    } else {
        gles.Finish();
    }
    let gl_error = gles.GetError();
    drop(gles);
    if gl_error != 0 {
        log!(
            "[DSfsr] shader present emitted GL error {:#x}; using fixed-function present",
            gl_error
        );
    }

    shader_ok && gl_error == 0
}

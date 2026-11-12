/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! EAGL.

use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::core_animation::ca_eagl_layer::{
    find_fullscreen_eagl_layer, get_pixels_vec_for_presenting, invalidate_presented_pixels, present_pixels,
};
use crate::frameworks::core_graphics::{CGRect, CGSize};
use crate::frameworks::foundation::ns_string::get_static_str;
use crate::frameworks::foundation::NSUInteger;
use crate::frameworks::uikit;
use crate::gles::gles11_raw as gles11; // constants only
use crate::gles::gles11_raw::types::*;
use crate::gles::present::{present_frame, FpsCounter};
use crate::gles::{
    create_gles1_ctx, create_gles2_ctx, create_gles3_ctx, gles1_on_gl2, GLESContext, GLES,
};
use crate::mem::MutPtr;
use crate::objc::{
    id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
};
use crate::options::Options;
use crate::Environment;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

// These are used by the EAGLDrawable protocol implemented by CAEAGLayer.
// Since these have the ABI of constant symbols rather than literal constants,
// the values shouldn't matter, and haven't been checked against real iPhone OS.
/// Frames presented so far. Used by the render-loop milestones below and by the
/// scripted-input executor (`TOUCHHLE_DS_SCRIPT`), which needs a frame clock that
/// matches the compositor's frame numbering.
pub static DS_FRAME_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub const kEAGLDrawablePropertyColorFormat: &str = "ColorFormat";
pub const kEAGLDrawablePropertyRetainedBacking: &str = "RetainedBacking";
pub const kEAGLColorFormatRGBA8: &str = "RGBA8";
pub const kEAGLColorFormatRGB565: &str = "RGB565";
/// `kEAGLColorFormatSRGBA8` — sRGB 8888 EAGL color format, added in
/// iOS 7. Apps that supply this string in `drawableProperties` are
/// requesting a sRGB-encoded color renderbuffer (per
/// `EAGLDrawable.h`).
pub const kEAGLColorFormatSRGBA8: &str = "SRGBA8";

pub const CONSTANTS: ConstantExports = &[
    (
        "_kEAGLDrawablePropertyColorFormat",
        HostConstant::NSString(kEAGLDrawablePropertyColorFormat),
    ),
    (
        "_kEAGLDrawablePropertyRetainedBacking",
        HostConstant::NSString(kEAGLDrawablePropertyRetainedBacking),
    ),
    (
        "_kEAGLColorFormatRGBA8",
        HostConstant::NSString(kEAGLColorFormatRGBA8),
    ),
    (
        "_kEAGLColorFormatRGB565",
        HostConstant::NSString(kEAGLColorFormatRGB565),
    ),
    (
        "_kEAGLColorFormatSRGBA8",
        HostConstant::NSString(kEAGLColorFormatSRGBA8),
    ),
];

type EAGLRenderingAPI = u32;
const kEAGLRenderingAPIOpenGLES1: EAGLRenderingAPI = 1;
const kEAGLRenderingAPIOpenGLES2: EAGLRenderingAPI = 2;
const kEAGLRenderingAPIOpenGLES3: EAGLRenderingAPI = 3;

/// Resolve the EAGL rendering API the host should actually create a context
/// for. When `prefer_gles2_context` is set and the app requested ES 1.1, we
/// transparently upgrade to ES 2.0 so apps that ask for ES 1.1 but drive
/// rendering with shader entry points (`glUseProgram`, `glCreateShader`, …)
/// route through the real native ES 2.0 backend instead of falling through
/// to the GLES 1.1-only stubs in `gles_generic`.
fn effective_eagl_api(requested: EAGLRenderingAPI, prefer_gles2_context: bool) -> EAGLRenderingAPI {
    if prefer_gles2_context && requested == kEAGLRenderingAPIOpenGLES1 {
        log!(
            "EAGL: --prefer-gles2-context active, upgrading initWithAPI:{} \
             (kEAGLRenderingAPIOpenGLES1) to kEAGLRenderingAPIOpenGLES2",
            requested
        );
        return kEAGLRenderingAPIOpenGLES2;
    }
    requested
}

#[derive(Default)]
pub(super) struct EAGLContextHostObject {
    pub(super) gles_ctx: Option<Box<dyn GLESContext>>,
    present_state: Rc<RefCell<crate::gles::present::PresentState>>,
    transfer_state: Rc<RefCell<TransferState>>,
    /// Which EAGL rendering API was requested. This influences how
    /// [super::gles_guest] dispatches calls and how the present-renderbuffer
    /// path saves and restores state.
    pub(super) api: EAGLRenderingAPI,
    /// Mapping of OpenGL ES renderbuffer names to `EAGLDrawable` instances
    /// (always `CAEAGLLayer*`). Retains the instance so it won't dangle.
    pub(super) renderbuffer_drawable_bindings: Rc<RefCell<HashMap<GLuint, id>>>,
    fps_counter: Option<FpsCounter>,
    next_frame_due: Option<Instant>,
    /// Framebuffer object that has this context's drawable renderbuffer
    /// attached to `COLOR_ATTACHMENT0_OES`.
    ///
    /// On iOS the guest's framebuffer 0 *is* the EAGL drawable
    /// (window-system framebuffer). touchHLE maps framebuffer 0 to the host
    /// SDL window instead, so apps that draw their final frame straight into
    /// framebuffer 0 (Dead Space 1.3.34 does exactly this) would never reach
    /// the drawable renderbuffer that `presentRenderbuffer:` copies out.
    /// `gles_guest::glBindFramebufferOES` aliases guest framebuffer 0 to this
    /// object so the iOS behaviour is reproduced.
    pub(super) drawable_fbo: Option<GLuint>,
    /// Optional multisample draw target aliased by guest framebuffer 0.
    pub(super) msaa_fbo: Option<GLuint>,
    pub(super) msaa_color_renderbuffer: Option<GLuint>,
    pub(super) msaa_depth_renderbuffer: Option<GLuint>,
    /// Guest FBOs may attach the same drawable colour renderbuffer but use
    /// distinct depth renderbuffers. Keep one multisample depth attachment per
    /// guest FBO so composition passes cannot inherit scene depth from each
    /// other. The colour attachment remains shared because the guest's drawable
    /// colour renderbuffer is shared.
    pub(super) msaa_depth_by_fbo: HashMap<GLuint, GLuint>,
    pub(super) msaa_samples: GLsizei,
    /// Reused source FBO for the slow glReadPixels present path. This field is
    /// owned by the EAGL context, so GL object lifetime and context switches
    /// cannot leave a stale name in another context.
    pub(super) readback_fbo: Option<GLuint>,
    gpu_text_overlay: Rc<RefCell<crate::ds_host_overlay::GpuState>>,
    pub mapped_buffers: HashMap<GLuint, (MutPtr<GLvoid>, *mut GLvoid)>,
}
impl HostObject for EAGLContextHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation EAGLContext: NSObject

+ (id)alloc {
    let host_object = Box::new(EAGLContextHostObject {
        gles_ctx: None,
        present_state: Default::default(),
        transfer_state: Default::default(),
        api: kEAGLRenderingAPIOpenGLES1,
        renderbuffer_drawable_bindings: Rc::new(RefCell::new(HashMap::new())),
        fps_counter: None,
        next_frame_due: None,
        drawable_fbo: None,
        msaa_fbo: None,
        msaa_color_renderbuffer: None,
        msaa_depth_renderbuffer: None,
        msaa_depth_by_fbo: HashMap::new(),
        msaa_samples: 0,
        readback_fbo: None,
        gpu_text_overlay: Default::default(),
        mapped_buffers: HashMap::new(),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)currentContext {
    env.framework_state.opengles.current_ctx_for_thread(env.current_thread).unwrap_or(nil)
}
+ (bool)setCurrentContext:(id)context { // EAGLContext*
    retain(env, context);

    let current_ctx = env.framework_state.opengles.current_ctx_for_thread(env.current_thread);

    if let Some(old_ctx) = std::mem::take(current_ctx) {
        release(env, old_ctx);
    }

    // reborrow
    let current_ctx = env.framework_state.opengles.current_ctx_for_thread(env.current_thread);

    if context != nil {
        *current_ctx = Some(context);
    }

    true
}

- (id)initWithAPI:(EAGLRenderingAPI)api sharegroup:(id)group {
    if api != kEAGLRenderingAPIOpenGLES1
        && api != kEAGLRenderingAPIOpenGLES2
        && api != kEAGLRenderingAPIOpenGLES3
    {
        log!(
            "App requested EAGL initWithAPI:{} sharegroup:{:?}, returning nil as we only support APIs 1, 2 and 3",
            api,
            group
        );
        return nil;
    }

    if group == nil {
        return msg![env; this initWithAPI:api];
    }

    let window = env.window.as_mut().expect("OpenGL ES is not supported in headless mode");
    let prev_context = env.objc.borrow_mut::<EAGLContextHostObject>(group).gles_ctx.as_mut().unwrap();

    // This is sort of a hack - we set the "current" context, then immediately
    // drop it. Since we know all the code between here and creating the new
    // context, we know that there won't be any context switches, so it's fine
    // to do this.
    {
        let _prev_ctx = prev_context.make_current(window);
    }
    env.window.as_mut().unwrap().set_share_with_current_context(true);

    let effective_api = effective_eagl_api(api, env.options.prefer_gles2_context);

    let mut gles_ins = match effective_api {
        kEAGLRenderingAPIOpenGLES3 => create_gles3_ctx(env),
        kEAGLRenderingAPIOpenGLES2 => create_gles2_ctx(env),
        _ => create_gles1_ctx(env),
    };

    let window = env.window.as_mut().expect("OpenGL ES is not supported in headless mode");
    {
        let gles_ctx = gles_ins.make_current(window);
        log!("Driver info: {}", unsafe { gles_ctx.driver_description() });
    }

    env.objc.borrow_mut::<EAGLContextHostObject>(this).gles_ctx = Some(gles_ins);
    env.objc.borrow_mut::<EAGLContextHostObject>(this).api = effective_api;

    env.window.as_mut().unwrap().set_share_with_current_context(false);

    env.objc.borrow_mut::<EAGLContextHostObject>(this).renderbuffer_drawable_bindings = env.objc.borrow::<EAGLContextHostObject>(group).renderbuffer_drawable_bindings.clone();
    this
}

- (id)initWithAPI:(EAGLRenderingAPI)api {
    if api != kEAGLRenderingAPIOpenGLES1
        && api != kEAGLRenderingAPIOpenGLES2
        && api != kEAGLRenderingAPIOpenGLES3
    {
        log!(
            "App requested EAGL initWithAPI:{}, returning nil as we only support APIs 1, 2 and 3",
            api
        );
        return nil;
    }

    let effective_api = effective_eagl_api(api, env.options.prefer_gles2_context);

    let mut gles_ins = match effective_api {
        kEAGLRenderingAPIOpenGLES3 => create_gles3_ctx(env),
        kEAGLRenderingAPIOpenGLES2 => create_gles2_ctx(env),
        _ => create_gles1_ctx(env),
    };

    let window = env.window.as_mut().expect("OpenGL ES is not supported in headless mode");
    {
        let gles_ctx = gles_ins.make_current(window);
        log!("Driver info: {}", unsafe { gles_ctx.driver_description() });
    }

    env.objc.borrow_mut::<EAGLContextHostObject>(this).gles_ctx = Some(gles_ins);
    env.objc.borrow_mut::<EAGLContextHostObject>(this).api = effective_api;

    this
}

- (EAGLRenderingAPI)API {
    env.objc.borrow::<EAGLContextHostObject>(this).api
}

- (id)sharegroup {
    // We use object itself as the sharegroup.
    // Check initWithAPI:sharegroup: for more info
    this
}

- (())dealloc {
    // Shared contexts can outlive this one, so dropping the GL context alone
    // does not necessarily reclaim its private textures/programs.
    if let Some(window) = env.window.as_mut() {
        let host = env.objc.borrow_mut::<EAGLContextHostObject>(this);
        if let Some(context) = host.gles_ctx.as_mut() {
            let mut gl = context.make_current(window);
            unsafe {
                host.transfer_state.borrow_mut().release(gl.as_mut());
                host.present_state.borrow_mut().release(gl.as_mut());
                host.gpu_text_overlay.borrow_mut().release(gl.as_mut());
                gl.gpu_profile_release();
            }
        }
    }
    let host_obj = env.objc.borrow_mut::<EAGLContextHostObject>(this);
    for &(guest_buf, _host_buf) in host_obj.mapped_buffers.values() {
        env.mem.free(guest_buf);
    }
    if Rc::strong_count(&host_obj.renderbuffer_drawable_bindings) == 1 {
        let bindings = std::mem::take(&mut host_obj.renderbuffer_drawable_bindings);
        for (_renderbuffer, drawable) in bindings.take() {
            release(env, drawable);
        }
    }
    env.objc.dealloc_object(this, &mut env.mem);
    // Explicit cleanup made the retiring context current. Restore the existing
    // thread/main fallback binding without changing the guest's context map.
    if env.framework_state.opengles.effective_ctx_id(env.current_thread)
        .is_some_and(|context| context != this) {
        if let Some(window) = env.window.as_mut() {
            let _ = super::sync_context(&mut env.framework_state.opengles, &mut env.objc,
                window, env.current_thread);
        }
    }
}

- (bool)renderbufferStorage:(NSUInteger)target
               fromDrawable:(id)drawable { // EAGLDrawable (always CAEAGLayer*)
    log!("[EAGLContext renderbufferStorage:{:#x} fromDrawable:{:?}]", target, drawable);

    assert!(target == gles11::RENDERBUFFER_OES);

    // Apple's `EAGLContext` documentation for
    // `-renderbufferStorage:fromDrawable:` states that passing `nil` for the
    // drawable "deletes any earlier binding" of the currently bound
    // renderbuffer to a drawable and returns `YES`. touchHLE used to
    // assert against this case (`drawable != nil`), which crashed apps
    // that legitimately unbind during teardown (e.g. Resident Evil 4 —
    // HyperHLE log #5 — calls `renderbufferStorage:fromDrawable:nil`
    // while tearing down its EAGL surface during a scene transition).
    //
    // Spec behaviour: look up the currently bound renderbuffer (via
    // `RENDERBUFFER_BINDING_OES`), drop its entry from the
    // (renderbuffer -> drawable) map, and release the retained drawable.
    // No new storage is allocated in this case.
    if drawable == nil {
        let window = env
            .window
            .as_mut()
            .expect("OpenGL ES is not supported in headless mode");
        let current_renderbuffer = {
            let Some(mut gles) = super::sync_context(
                &mut env.framework_state.opengles,
                &mut env.objc,
                window,
                env.current_thread,
            ) else {
                // No current EAGL context: there can't be a meaningful
                // renderbuffer binding to remove. Apple-style soft failure.
                log!(
                    "[EAGLContext renderbufferStorage:{:#x} fromDrawable:nil] \
                     called with no current GL context for thread {}; \
                     treating as no-op.",
                    target,
                    env.current_thread
                );
                return true;
            };
            let mut renderbuffer: gles11::types::GLint = 0;
            unsafe {
                gles.GetIntegerv(gles11::RENDERBUFFER_BINDING_OES, &mut renderbuffer);
            }
            renderbuffer as gles11::types::GLuint
        };

        let removed = {
            let host_obj = env.objc.borrow_mut::<EAGLContextHostObject>(this);
            host_obj
                .renderbuffer_drawable_bindings
                .borrow_mut()
                .remove(&current_renderbuffer)
        };
        if let Some(old_drawable) = removed {
            release(env, old_drawable);
        } else {
            log_dbg!(
                "[EAGLContext renderbufferStorage:{:#x} fromDrawable:nil]: \
                 no drawable was bound to renderbuffer {} — nothing to unbind.",
                target,
                current_renderbuffer
            );
        }
        return true;
    }

    let props: id = msg![env; drawable drawableProperties];

    let format_key = get_static_str(env, kEAGLDrawablePropertyColorFormat);
    let format_rgba8 = get_static_str(env, kEAGLColorFormatRGBA8);
    let format_rgb565 = get_static_str(env, kEAGLColorFormatRGB565);

    let format: id = msg![env; props objectForKey:format_key];
    // Theoretically this should map formats like:
    // - kColorFormatRGBA8 => RGBA8_OES
    // - kColorFormatRGB565 => RGB565_OES
    // However, the specification of EXT_framebuffer_object allows the
    // implementation to arbitrarily restrict which formats can be rendered to,
    // and it seems like RGB565 isn't supported, at least on a machine with
    // Intel HD Graphics 615 running macOS Monterey. I don't think RGBA8 is
    // guaranteed either, but it at least seems to work.
    if !msg![env; format isEqual:format_rgba8] && !msg![env; format isEqual:format_rgb565] {
        log!("[renderbufferStorage:{:?} fromDrawable:{:?}] Warning: unhandled format {:?}, using RGBA8", target, drawable, format);
    }
    let internalformat = gles11::RGBA8_OES;

    let (width, height) = {
        // ULTRAHLE_MINIONJUMP_RENDERBUFFER_BEGIN
        if matches!(
            env.bundle.bundle_identifier(),
            "com.apprisetec9.minionjump" | "com.risinghighapps.kingdomprincepro"
        ) {
            log!("UltraHLE MinionJump: forcing EAGL renderbuffer storage to 1024x768");
            (1024, 768)
        } else {
            let bounds: CGRect = msg![env; drawable bounds];
            let CGSize { width, height } = bounds.size;

            // Apple's `-renderbufferStorage:fromDrawable:` derives the
            // renderbuffer size from the CAEAGLayer's bounds. Some apps (e.g.
            // Beyond Gravity — HyperHLE log #1) momentarily present a layer
            // whose `bounds.size` is bogus (non-finite, negative, or zero)
            // during an unbind/rebind cycle while tearing down and recreating
            // their EAGL surface. touchHLE used to `assert!` the size was in a
            // sane range, which aborted the whole emulator. Real iOS never
            // crashes here — it simply allocates a renderbuffer sized to the
            // (screen-sized) drawable. Match that by falling back to the main
            // screen's bounds when the drawable reports an invalid size.
            let size_is_valid = |v: f32| v.is_finite() && (1.0..(u32::MAX as f32)).contains(&v);
            let (width, height) = if size_is_valid(width) && size_is_valid(height) {
                (width, height)
            } else {
                let screen: id = msg_class![env; UIScreen mainScreen];
                let screen_bounds: CGRect = msg![env; screen bounds];
                // Copy the fields out of the packed CGSize before using them
                // (taking a reference to a packed struct field is UB / a
                // compile error).
                let fallback_width = screen_bounds.size.width;
                let fallback_height = screen_bounds.size.height;
                log!(
                    "[renderbufferStorage:{:?} fromDrawable:{:?}] Warning: drawable \
                     reported invalid bounds size {}x{}; falling back to main screen \
                     bounds {}x{}",
                    target,
                    drawable,
                    width,
                    height,
                    fallback_width,
                    fallback_height
                );
                (fallback_width, fallback_height)
            };
            let scale_hack = env.options.scale_hack.get();

            let mut width = width.round() as u32 * scale_hack;
            let mut height = height.round() as u32 * scale_hack;

            // If even the fallback produced a degenerate size, clamp to a
            // minimum 1x1 so the GL call below cannot receive a zero extent.
            width = width.max(1);
            height = height.max(1);

            if std::env::var_os("TOUCHHLE_FORCE_LANDSCAPE_RENDERBUFFER").is_some() {
                let is_landscape = env
                    .window
                    .as_ref()
                    .map(|window| {
                        !matches!(
                            window.current_rotation(),
                            crate::window::DeviceOrientation::Portrait
                        )
                    })
                    .unwrap_or(false);

                if is_landscape && height > width {
                    log!(
                        "TOUCHHLE_FORCE_LANDSCAPE_RENDERBUFFER=1: swapping EAGL renderbuffer storage from {}x{} to {}x{}",
                        width,
                        height,
                        height,
                        width
                    );
                    std::mem::swap(&mut width, &mut height);
                } else {
                    log!(
                        "TOUCHHLE_FORCE_LANDSCAPE_RENDERBUFFER=1: keeping EAGL renderbuffer storage at {}x{} (is_landscape={})",
                        width,
                        height,
                        is_landscape
                    );
                }
            }

            (width, height)
        }
        // ULTRAHLE_MINIONJUMP_RENDERBUFFER_END
    };

    // Apple's documentation states that the receiver must be the current
    // context when calling `renderbufferStorage:fromDrawable:`.  If no
    // context is current for this thread but `this` has a valid backing GLES
    // context, temporarily make `this` the current context so the GL call
    // succeeds, then restore the previous state.  This matches the behaviour
    // observed on real iOS where calling the method on a non-current context
    // still works as long as the receiver has been initialised.
    //
    // Note: `window` must be borrowed from `env` AFTER any calls to
    // `retain`/`release` because those also borrow `env` mutably.
    let prior_ctx = *env.framework_state.opengles.current_ctx_for_thread(env.current_thread);
    let needs_temp_current = prior_ctx.is_none() && {
        env.objc.borrow::<EAGLContextHostObject>(this).gles_ctx.is_some()
    };
    if needs_temp_current {
        log_dbg!(
            "[EAGLContext renderbufferStorage:{:#x} fromDrawable:{:?}] \
             no current context for thread {}; temporarily making this context current.",
            target, drawable, env.current_thread
        );
        retain(env, this);
        *env.framework_state.opengles.current_ctx_for_thread(env.current_thread) = Some(this);
    }

    // Run the actual GL storage allocation inside a nested scope so that
    // `window` (which mutably borrows `env.window`) is dropped before we need
    // to call `retain`/`release` with a full `&mut env` borrow below.
    let renderbuffer_result: Option<u32> = {
        let window = env.window.as_mut().expect("OpenGL ES is not supported in headless mode");
        match super::sync_context(
            &mut env.framework_state.opengles,
            &mut env.objc,
            window,
            env.current_thread,
        ) {
            None => {
                log!(
                    "[EAGLContext renderbufferStorage:{:#x} fromDrawable:{:?}] \
                     called with no current GL context for thread {}; failing \
                     the call instead of crashing.",
                    target,
                    drawable,
                    env.current_thread
                );
                None
            }
            Some(mut gles) => {
                // Clear any pre-existing error so we can detect failure of the
                // storage allocation reliably.
                unsafe { while gles.GetError() != gles11::NO_ERROR {} }
                unsafe { gles.RenderbufferStorageOES(target, internalformat, width.try_into().unwrap(), height.try_into().unwrap()); }
                let needs_fallback = unsafe { gles.GetError() != gles11::NO_ERROR };
                let alloc_ok = if needs_fallback {
                    // RGBA8 is optional in OpenGL ES 1.1 Common Profile (requires
                    // OES_rgb8_rgba8). Fall back to RGBA4 (0x8056) which is
                    // required by OES_framebuffer_object.
                    const GL_RGBA4: gles11::types::GLenum = 0x8056;
                    unsafe { gles.RenderbufferStorageOES(target, GL_RGBA4, width.try_into().unwrap(), height.try_into().unwrap()); }
                    unsafe { gles.GetError() == gles11::NO_ERROR }
                } else {
                    true
                };
                if !alloc_ok {
                    log!(
                        "[EAGLContext renderbufferStorage:{:#x} fromDrawable:{:?}] \
                         failed to allocate renderbuffer storage (tried RGBA8 and RGBA4)",
                        target,
                        drawable
                    );
                    None
                } else {
                    let mut renderbuffer: gles11::types::GLint = 0;
                    unsafe { gles.GetIntegerv(gles11::RENDERBUFFER_BINDING_OES, &mut renderbuffer); }
                    if crate::ds_startup_report::enabled() {
                        let description = unsafe { gles.driver_description() };
                        crate::ds_startup_report::renderer(&description, width as u32, height as u32);
                    }
                    Some(renderbuffer as u32)
                }
            }
        }
    };

    // `window` borrow dropped here — safe to use `retain`/`release` again.
    // `None` means either no GL context was available, or storage allocation failed.
    let renderbuffer = match renderbuffer_result {
        None => {
            if needs_temp_current {
                *env.framework_state.opengles.current_ctx_for_thread(env.current_thread) = None;
                release(env, this);
            }
            return false;
        }
        Some(rb) => rb,
    };

    // Restore the previous thread-local context (if we temporarily set `this`
    // as the current context earlier in this call).
    if needs_temp_current {
        *env.framework_state.opengles.current_ctx_for_thread(env.current_thread) = None;
        release(env, this);
    }

    // Create (or reuse) the framebuffer that aliases the EAGL drawable, with
    // this renderbuffer attached as the colour attachment. Guest framebuffer 0
    // is redirected to it (see `EAGLContextHostObject::drawable_fbo`).
    {
        let prior_fbo = env.objc.borrow::<EAGLContextHostObject>(this).drawable_fbo;
        let mut new_fbo: Option<GLuint> = None;
        {
            let window = env
                .window
                .as_mut()
                .expect("OpenGL ES is not supported in headless mode");
            if let Some(mut gles) = super::sync_context(
                &mut env.framework_state.opengles,
                &mut env.objc,
                window,
                env.current_thread,
            ) {
                unsafe {
                    let mut fbo: GLuint = prior_fbo.unwrap_or(0);
                    if fbo == 0 {
                        gles.GenFramebuffersOES(1, &mut fbo);
                    }
                    let mut old_fbo: GLint = 0;
                    gles.GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut old_fbo);
                    gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, fbo);
                    gles.FramebufferRenderbufferOES(
                        gles11::FRAMEBUFFER_OES,
                        gles11::COLOR_ATTACHMENT0_OES,
                        gles11::RENDERBUFFER_OES,
                        renderbuffer,
                    );
                    gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, old_fbo as GLuint);
                    if fbo != 0 {
                        new_fbo = Some(fbo);
                    }
                }
            }
        }
        if let Some(fbo) = new_fbo {
            env.objc
                .borrow_mut::<EAGLContextHostObject>(this)
                .drawable_fbo = Some(fbo);
        }
    }

    // Optional MSAA target for the EAGL drawable. SDL's multisample attributes
    // only cover the default window framebuffer, while Dead Space renders into
    // this EAGL FBO. Build a real multisample FBO here and resolve it into the
    // single-sample drawable renderbuffer at present time.
    let mut startup_msaa_actual: GLsizei = 1;
    let requested_samples: GLsizei = std::env::var("TOUCHHLE_DS_MSAA")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(1)
        .clamp(1, 16) as GLsizei;
    if requested_samples > 1 {
        let drawable_fbo = env.objc.borrow::<EAGLContextHostObject>(this).drawable_fbo;
        let mut created = false;
        let mut msaa_values: Option<(GLuint, GLuint, Option<GLuint>, GLsizei, GLsizei)> = None;
        if let Some(draw_fbo) = drawable_fbo {
            let window = env
                .window
                .as_mut()
                .expect("OpenGL ES is not supported in headless mode");
            if let Some(mut gles) = super::sync_context(
                &mut env.framework_state.opengles,
                &mut env.objc,
                window,
                env.current_thread,
            ) {
                unsafe {
                    while gles.GetError() != gles11::NO_ERROR {}
                    let mut old_fbo: GLint = 0;
                    let mut old_rb: GLint = 0;
                    gles.GetIntegerv(gles11::FRAMEBUFFER_BINDING_OES, &mut old_fbo);
                    gles.GetIntegerv(gles11::RENDERBUFFER_BINDING_OES, &mut old_rb);
                    let mut max_samples: GLint = 1;
                    // GL_MAX_SAMPLES_EXT.
                    gles.GetIntegerv(0x8d57, &mut max_samples);
                    let effective_samples =
                        requested_samples.min(max_samples.max(1) as GLsizei).max(1);

                    let mut color_rb: GLuint = 0;
                    let mut depth_rb: GLuint = 0;
                    let mut msaa_fbo: GLuint = 0;
                    gles.GenRenderbuffersOES(1, &mut color_rb);
                    gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, color_rb);
                    gles.RenderbufferStorageMultisampleAPPLE(
                        gles11::RENDERBUFFER_OES,
                        effective_samples,
                        gles11::RGBA8_OES,
                        width as GLsizei,
                        height as GLsizei,
                    );
                    let color_err = gles.GetError();

                    if color_err == gles11::NO_ERROR {
                        gles.GenRenderbuffersOES(1, &mut depth_rb);
                        gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, depth_rb);
                        // GL_DEPTH_COMPONENT24_OES.
                        gles.RenderbufferStorageMultisampleAPPLE(
                            gles11::RENDERBUFFER_OES,
                            effective_samples,
                            0x81a6,
                            width as GLsizei,
                            height as GLsizei,
                        );
                        if gles.GetError() != gles11::NO_ERROR {
                            gles.DeleteRenderbuffersOES(1, &depth_rb);
                            depth_rb = 0;
                        }

                        gles.GenFramebuffersOES(1, &mut msaa_fbo);
                        gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, msaa_fbo);
                        gles.FramebufferRenderbufferOES(
                            gles11::FRAMEBUFFER_OES,
                            gles11::COLOR_ATTACHMENT0_OES,
                            gles11::RENDERBUFFER_OES,
                            color_rb,
                        );
                        if depth_rb != 0 {
                            gles.FramebufferRenderbufferOES(
                                gles11::FRAMEBUFFER_OES,
                                gles11::DEPTH_ATTACHMENT_OES,
                                gles11::RENDERBUFFER_OES,
                                depth_rb,
                            );
                        }
                        if gles.CheckFramebufferStatusOES(gles11::FRAMEBUFFER_OES)
                            == gles11::FRAMEBUFFER_COMPLETE_OES
                        {
                            let samples: GLint = ({
                                let mut value: GLint = 0;
                                gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, color_rb);
                                // GL_RENDERBUFFER_SAMPLES_APPLE.
                                gles.GetRenderbufferParameterivOES(
                                    gles11::RENDERBUFFER_OES,
                                    0x8cab,
                                    &mut value,
                                );
                                value
                            });
                            // A driver may accept the FBO but silently keep one
                            // sample. Commit only a real multisample target;
                            // otherwise the ordinary drawable remains intact.
                            if samples > 1 {
                                msaa_values = Some((
                                    msaa_fbo,
                                    color_rb,
                                    if depth_rb != 0 { Some(depth_rb) } else { None },
                                    effective_samples,
                                    samples,
                                ));
                                created = true;
                            }
                        }
                    }

                    if !created {
                        if msaa_fbo != 0 {
                            gles.DeleteFramebuffersOES(1, &msaa_fbo);
                        }
                        if color_rb != 0 {
                            gles.DeleteRenderbuffersOES(1, &color_rb);
                        }
                        if depth_rb != 0 {
                            gles.DeleteRenderbuffersOES(1, &depth_rb);
                        }
                        log!(
                            "[DSmsaa] requested {}x but driver/FBO setup failed; falling back to 1x",
                            requested_samples
                        );
                    }
                    gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, old_fbo as GLuint);
                    gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, old_rb as GLuint);
                    while gles.GetError() != gles11::NO_ERROR {}
                }
            }
        }
        if let Some((msaa_fbo, color_rb, depth_rb, effective_samples, samples)) = msaa_values {
            startup_msaa_actual = samples;
            let host = env.objc.borrow_mut::<EAGLContextHostObject>(this);
            host.msaa_fbo = Some(msaa_fbo);
            host.msaa_color_renderbuffer = Some(color_rb);
            host.msaa_depth_renderbuffer = depth_rb;
            host.msaa_samples = samples;
            crate::ds_monitor::actual_msaa(samples);
            log!(
                "[DSmsaa] EAGL FBO multisample enabled: requested={} clamped={} actual={} resolve_fbo={}",
                requested_samples,
                effective_samples,
                samples,
                drawable_fbo.unwrap_or(0)
            );
        }
    }

    crate::ds_startup_report::msaa(requested_samples as u32, startup_msaa_actual as u32);
    retain(env, drawable);
    let host_obj = env.objc.borrow_mut::<EAGLContextHostObject>(this);
    let maybe_old_drawable = host_obj.renderbuffer_drawable_bindings.borrow_mut().insert(
        renderbuffer,
        drawable
    );
    if let Some(old_drawable) = maybe_old_drawable {
        release(env, old_drawable);
    }

    true
}

- (bool)presentRenderbuffer:(NSUInteger)target {
    let _stutter_span=crate::ds_stutter_probe::Span::new(11,target as u64);
    crate::ds_frontend_trace::present(env);
    // Some games (e.g. Angry Birds 1.0) run their main loop without going
    // through the NSRunLoop, so handle_events() in the run loop never fires.
    // Poll and dispatch pending input events here, at the natural per-frame
    // boundary, so touches always reach the game.
    if env.current_thread == 0 {
        env.on_parent_stack_in_coroutine(|window, options| {
            window.poll_for_events(options);
        });
        uikit::handle_events(env);
    }

    // First-frame breadcrumb. presentRenderbuffer is called every frame, so a
    // plain log!() would flood, but the very first call is a key signal that
    // the app actually got past splash/init and is rendering.
    log_once!("[EAGLContext presentRenderbuffer:] first call (app reached first frame)");

    // Frame-count milestones. presentRenderbuffer is called every frame, so we
    // want a small, fixed number of log lines that prove the render loop is
    // still progressing (useful for distinguishing "actually hung" from
    // "running but invisible because the app's stdout doesn't reach this log
    // sink"). The milestones are roughly logarithmic so they cover the range
    // from sub-second to ~10 minutes at 60 FPS without flooding.
    {
        use std::sync::atomic::Ordering;
        let n = crate::window::DS_FRAME_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
        crate::ds_startup_report::present(n);
        crate::ds_diagnostics::present(n);
        crate::window::ds_record_present_frame();
        if matches!(n, 10 | 60 | 300 | 1800 | 3600 | 7200 | 18000 | 36000) {
            log!(
                "[EAGLContext presentRenderbuffer:] frame {} reached (render loop is alive)",
                n
            );
        }
    }

    // Frame stepping: `TOUCHHLE_DS_FRAME_STEP=<file>` holds the guest at the
    // frame boundary while the file's number is lower than the current frame.
    // Put a small number in the file to stop there, then raise it by one to
    // advance a single frame. This deliberately blocks the guest thread (and
    // therefore the window) instead of touching the SDL event pump, so it can't
    // steal the user's input.
    if let Ok(path) = std::env::var("TOUCHHLE_DS_FRAME_STEP") {
        use std::sync::atomic::Ordering;
        loop {
            let target = std::fs::read_to_string(&path)
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(u64::MAX);
            let current = crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed);
            if current <= target {
                break;
            }
            log_once!(
                "TOUCHHLE_DS_FRAME_STEP: holding the guest at a frame boundary"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    assert!(target == gles11::RENDERBUFFER_OES);

    // Resolve the optional multisample EAGL FBO into the normal drawable
    // renderbuffer before frame dumping/compositing/presentation.
    let msaa_resolve = {
        let host = env.objc.borrow::<EAGLContextHostObject>(this);
        match (host.msaa_fbo, host.drawable_fbo) {
            (Some(msaa), Some(resolve)) if host.msaa_samples > 1 => {
                Some((msaa, resolve))
            }
            _ => None,
        }
    };
    if let Some((msaa_fbo, resolve_fbo)) = msaa_resolve {
        let window = env
            .window
            .as_mut()
            .expect("OpenGL ES is not supported in headless mode");
        if let Some(mut gles) = super::sync_context(
            &mut env.framework_state.opengles,
            &mut env.objc,
            window,
            env.current_thread,
        ) {
            unsafe {
                // READ_FRAMEBUFFER_APPLE / DRAW_FRAMEBUFFER_APPLE.
                gles.gpu_profile_poll();
                gles.gpu_profile_begin(1);
                gles.BindFramebufferOES(0x8ca8, msaa_fbo);
                gles.BindFramebufferOES(0x8ca9, resolve_fbo);
                gles.ResolveMultisampleFramebufferAPPLE();
                gles.gpu_profile_end(1);
                if std::env::var_os("TOUCHHLE_DS_MSAA_CLEAR_DEPTH").is_some()
                    && !crate::ds_msaa::guest_depth_cleared(
                        crate::window::DS_FRAME_COUNTER
                            .load(std::sync::atomic::Ordering::Relaxed),
                    )
                {
                    gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, msaa_fbo);
                    let mut old_mask: GLboolean = 0;
                    gles.GetBooleanv(gles11::DEPTH_WRITEMASK, &mut old_mask);
                    gles.DepthMask(gles11::TRUE);
                    gles.ClearDepthf(1.0);
                    gles.Clear(gles11::DEPTH_BUFFER_BIT);
                    gles.DepthMask(old_mask);
                }
                // Leave the resolved target bound for ReadPixels/dump paths.
                gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, resolve_fbo);
                if std::env::var_os("TOUCHHLE_DS_MSAA_PROBE").is_some() {
                    use std::sync::atomic::{AtomicU32, Ordering};
                    static N: AtomicU32 = AtomicU32::new(0);
                    let n = N.fetch_add(1, Ordering::Relaxed);
                    // Detailed readback is limited to the first few frames.
                    if n < 8 {
                        let mut read_attachment: GLint = 0;
                        let mut draw_attachment: GLint = 0;
                        gles.BindFramebufferOES(0x8ca8, msaa_fbo);
                        gles.GetFramebufferAttachmentParameterivOES(
                            0x8ca8,
                            gles11::COLOR_ATTACHMENT0_OES,
                            gles11::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME_OES,
                            &mut read_attachment,
                        );
                        gles.BindFramebufferOES(0x8ca9, resolve_fbo);
                        gles.GetFramebufferAttachmentParameterivOES(
                            0x8ca9,
                            gles11::COLOR_ATTACHMENT0_OES,
                            gles11::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME_OES,
                            &mut draw_attachment,
                        );
                        gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, resolve_fbo);
                        let mut viewport = [0i32; 4];
                        gles.GetIntegerv(gles11::VIEWPORT, viewport.as_mut_ptr());
                        let width = viewport[2].max(1);
                        let height = viewport[3].max(1);
                        let xmid = width / 2;
                        let ymid = height / 2;
                        let probes = [
                            (0, 0),
                            (width.saturating_sub(1), 0),
                            (0, height.saturating_sub(1)),
                            (width.saturating_sub(1), height.saturating_sub(1)),
                            (xmid, ymid),
                        ];
                        let mut values = [[0u8; 4]; 5];
                        for ((x, y), px) in probes.into_iter().zip(values.iter_mut()) {
                            gles.ReadPixels(
                                x,
                                y,
                                1,
                                1,
                                gles11::RGBA,
                                gles11::UNSIGNED_BYTE,
                                px.as_mut_ptr() as *mut _,
                            );
                        }
                        let err = gles.GetError();
                        log!(
                            "[DSmsaa probe] n={} resolve={} msaa={} read_attachment={} draw_attachment={} readback={:?} glerr={:#x}",
                            n,
                            resolve_fbo,
                            msaa_fbo,
                            read_attachment,
                            draw_attachment,
                            values,
                            err
                        );
                        gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, resolve_fbo);
                    }
                }
            }
        }
    }

    super::gles_guest::ds_trace_present_frame(env);

    // Opt-in PC flicker diagnostics. This samples the completed drawable only
    // and emits through the existing asynchronous diagnostics writer; it does
    // not hold, filter, or otherwise alter the presented frame.
    if crate::ds_flicker_trace::requested() {
        let window = env
            .window
            .as_mut()
            .expect("OpenGL ES is not supported in headless mode");
        if let Some(mut gles) = super::sync_context(
            &mut env.framework_state.opengles,
            &mut env.objc,
            window,
            env.current_thread,
        ) {
            unsafe { crate::ds_flicker_trace::sample(gles.as_mut()); }
        }
    }

    let suppress_black_frame = if std::env::var_os("TOUCHHLE_DS_HOLD_LAST_ON_BLACK").is_some() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static LAST_MAX: AtomicU32 = AtomicU32::new(0);
        static HELD: AtomicU32 = AtomicU32::new(0);
        let window = env
            .window
            .as_mut()
            .expect("OpenGL ES is not supported in headless mode");
        super::sync_context(
            &mut env.framework_state.opengles,
            &mut env.objc,
            window,
            env.current_thread,
        )
        .map(|mut gles| unsafe {
            let mut viewport = [0i32; 4];
            gles.GetIntegerv(gles11::VIEWPORT, viewport.as_mut_ptr());
            let x0 = viewport[0];
            let y0 = viewport[1];
            let width = viewport[2];
            let height = viewport[3];
            if width <= 0 || height <= 0 {
                return false;
            }
            let mut max_luma = 0u8;
            for gy in 1..=3 {
                for gx in 1..=3 {
                    let x = x0 + width * gx / 4;
                    let y = y0 + height * gy / 4;
                    let mut pixel = [0u8; 4];
                    gles.ReadPixels(
                        x,
                        y,
                        1,
                        1,
                        gles11::RGBA,
                        gles11::UNSIGNED_BYTE,
                        pixel.as_mut_ptr().cast(),
                    );
                    let luma = pixel[0]
                        .max(pixel[1])
                        .max(pixel[2]);
                    max_luma = max_luma.max(luma);
                }
            }
            let previous = LAST_MAX.swap(max_luma as u32, Ordering::Relaxed);
            if max_luma <= 2 && previous >= 16 && HELD.load(Ordering::Relaxed) < 120 {
                LAST_MAX.store(previous, Ordering::Relaxed);
                HELD.fetch_add(1, Ordering::Relaxed);
                true
            } else {
                HELD.store(0, Ordering::Relaxed);
                false
            }
        })
        .unwrap_or(false)
    } else {
        false
    };

    // ★ Marked frames (M / F7) must be captured at EVERY resolution. The
    // clear-based dumper in `glClear` only runs on frames where the guest clears
    // the colour buffer; measured 2026-10-17 at --scale-hack=4 the level never
    // did, so every F7 shot was silently lost. Dump here instead - the drawable
    // is complete at the present boundary.
    let marked = crate::window::DS_MARK.swap(false, std::sync::atomic::Ordering::Relaxed);
    let periodic_dump = if std::env::var_os("TOUCHHLE_DS_DUMP_AT_PRESENT").is_some() {
        use std::sync::atomic::Ordering;
        let frame = crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed) as usize;
        let every: usize = std::env::var("TOUCHHLE_DS_DUMP_FRAME_EVERY")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(120);
        let from: usize = std::env::var("TOUCHHLE_DS_DUMP_FRAME_FROM")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let to: usize = std::env::var("TOUCHHLE_DS_DUMP_FRAME_TO")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(usize::MAX);
        frame >= from && frame <= to && every > 0 && frame % every == 0
    } else {
        false
    };
    if marked || periodic_dump {
        crate::frameworks::opengles::gles_guest::ds_dump_presented_frame(env, marked);
    }

    // The presented frame should be displayed ASAP, but the next one must be
    // delayed, so this needs to be checked before returning.
    let present_deadline = limit_framerate(&mut env.objc.borrow_mut::<EAGLContextHostObject>(this).next_frame_due, &env.options);

    if env.options.print_fps {
        env
            .objc
            .borrow_mut::<EAGLContextHostObject>(this)
            .fps_counter
            .get_or_insert_with(FpsCounter::start)
            .count_frame(format_args!("EAGLContext {this:?}"));
    }

    let fullscreen_layer = find_fullscreen_eagl_layer(env);
    let text_owner=env.framework_state.opengles.effective_ctx_id(env.current_thread).unwrap_or(this);
    let text_state=Rc::clone(&env.objc.borrow::<EAGLContextHostObject>(text_owner).gpu_text_overlay);


    // Unclear from documentation if this method requires the context to be
    // current, but it would be weird if it didn't?
    let window = env.window.as_mut().expect("OpenGL ES is not supported in headless mode");
    let Some(mut gles) = super::sync_context(&mut env.framework_state.opengles, &mut env.objc, window, env.current_thread) else {
        // No current EAGL context. Apple's docs require the receiver to be
        // the current context for `presentRenderbuffer:` to succeed; the
        // canonical iOS behaviour is to silently return NO in this state
        // rather than abort. Returning here also keeps the framerate
        // limiter from leaking a sleep if we somehow get here without a
        // context.
        log!(
            "[EAGLContext presentRenderbuffer:{:#x}] called with no current \
             GL context for thread {}; returning NO.",
            target,
            env.current_thread
        );
        finish_present_pacing(env, present_deadline);
        return false;
    };

    // Decide fallback before selecting direct presentation. The ES2/ES3
    // present path has no fixed-function text-card draw and must retain CPU
    // captions when the opt-in GPU renderer is unsupported.
    unsafe {text_state.borrow_mut().prepare_backend(gles.as_mut());}
    let bound_renderbuffer: GLuint = unsafe {
        let mut renderbuffer = 0;
        gles.GetIntegerv(gles11::RENDERBUFFER_BINDING_OES, &mut renderbuffer);
        renderbuffer as _
    };

    std::mem::drop(gles);

    // Apple's `-[EAGLContext presentRenderbuffer:]` presents the renderbuffer
    // that a previous `renderbufferStorage:fromDrawable:` call set up for THIS
    // context. It does not depend on `GL_RENDERBUFFER_BINDING_OES` at call
    // time: real iOS does not require the drawable renderbuffer to be bound.
    //
    // Dead Space 1.3.34 explicitly binds renderbuffer 0 right before every
    // `presentRenderbuffer:` call, so trusting the current binding made
    // touchHLE reject every frame (log: "Can't present a renderbuffer 0 not
    // bound to a drawable!"), which left the window permanently black. Fall
    // back to the context's registered drawable renderbuffer when the current
    // binding is not one this context knows about.
    // Refresh the receiving context's actual allocation, including zero on a
    // new non-MSAA context. A previous context's global value is not current.
    crate::ds_monitor::actual_msaa(env.objc.borrow::<EAGLContextHostObject>(this).msaa_samples);
    let renderbuffer = {
        let bindings = env
            .objc
            .borrow::<EAGLContextHostObject>(this)
            .renderbuffer_drawable_bindings
            .borrow();
        if bindings.contains_key(&bound_renderbuffer) {
            bound_renderbuffer
        } else if bindings.len() == 1 {
            *bindings.keys().next().unwrap()
        } else {
            bound_renderbuffer
        }
    };

    let Some(&drawable) = env
        .objc
        .borrow::<EAGLContextHostObject>(this)
        .renderbuffer_drawable_bindings
        .borrow()
        .get(&renderbuffer) else {
        log_dbg!("Can't present a renderbuffer {:?} not bound to a drawable!", renderbuffer);
        return false;
    };

    // Optional debug dump of the drawable renderbuffer that is being
    // presented. Set TOUCHHLE_DUMP_FRAMES=<dir> and every 120th presented
    // frame is written out as a PPM so the rendered content can be inspected
    // outside the emulator.
    if let Ok(dump_dir) = std::env::var("TOUCHHLE_DUMP_FRAMES") {
        use std::sync::atomic::{AtomicU64, Ordering};
        static DUMP_N: AtomicU64 = AtomicU64::new(0);
        let n = DUMP_N.fetch_add(1, Ordering::Relaxed);
        if n % 120 == 0 {
            let cached_fbo = env
                .objc
                .borrow::<EAGLContextHostObject>(this)
                .readback_fbo;
            let dump_result = if let Some(mut dump_gles) = super::sync_context(
                &mut env.framework_state.opengles,
                &mut env.objc,
                env.window.as_mut().expect("OpenGL ES is not supported in headless mode"),
                env.current_thread,
            ) {
                Some(unsafe {
                    read_renderbuffer(dump_gles.as_mut(), renderbuffer, Vec::new(), cached_fbo)
                })
            } else {
                None
            };
            if let Some((pixels, w, h, readback_fbo)) = dump_result {
                env.objc
                    .borrow_mut::<EAGLContextHostObject>(this)
                    .readback_fbo = Some(readback_fbo);
                dump_rgba_ppm(&dump_dir, n, w, h, &pixels);
            }
        }
    }

    // We're presenting to the opaque CAEAGLLayer that covers the screen.
    // We can use the fast path where we skip composition and present directly.
    // NOTE: the direct (fast) present path draws straight to the window's
    // back buffer, but touchHLE's Core Animation compositor owns the window and
    // repaints it from the layer tree, which erases that frame again. Route
    // through the compositing (slow) path instead, which stores the frame in
    // the CAEAGLLayer's `presented_pixels` and lets the compositor draw it.
    // (Set TOUCHHLE_USE_FAST_PRESENT=1 to get the old behaviour back.)
    // NOTE: `recomposite_if_necessary` deliberately skips composition while a
    // fullscreen CAEAGLLayer exists (it assumes the EAGL context presents
    // directly), so the direct present path is the one that reaches the window.
    // TOUCHHLE_FORCE_COMPOSITE=1 forces the copy-to-RAM + compositor path for
    // experiments (it will *not* be drawn unless that check is also relaxed).
    let force_composite = crate::ds_host_overlay::requires_cpu_composition(&env.options);
    if !suppress_black_frame
        && drawable == fullscreen_layer
        && !force_composite
    {
        log_dbg!(
            "Layer {:?} is the fullscreen layer, presenting renderbuffer {:?} directly (fast path).",
            drawable,
            renderbuffer,
        );
        // re-borrow
        if env.options.fast_present {
            log_once!("[present] fast path forced by --fast-present");
        } else {
            log_once!("[present] fast path (drawable == fullscreen layer)");
        }
        invalidate_presented_pixels(env, drawable);
        unsafe {
            let _profile = crate::ds_perf::Timer::new(crate::ds_perf::Stage::FastPresent);
            present_renderbuffer(env, renderbuffer, this);
        }
    } else if !suppress_black_frame {
        if fullscreen_layer != nil && drawable != fullscreen_layer {
            // If there's a single layer that covers the screen, and this isn't
            // it, there's no point in presenting the output because it won't be
            // seen. Using a noisy log because it's a weird scenario and might
            // indicate a bug.
            log!(
                "Layer {:?} is not the fullscreen layer {:?}, skipping presentation of renderbuffer {:?}!",
                drawable,
                fullscreen_layer,
                renderbuffer,
            );
            finish_present_pacing(env, present_deadline);
            return true;
        }

        // The very slow and inefficient path: not only does glReadPixels()
        // block the thread until rendering finishes, but the result has to be
        // copied back to system RAM, and then will have to be copied to VRAM
        // again during composition. find_fullscreen_eagl_layer() exists to
        // avoid this.
        log_dbg!(
            "There is no fullscreen layer, presenting renderbuffer {:?} to layer {:?} by copying to RAM (slow path).",
            renderbuffer,
            drawable,
        );
        log_once!("[present] slow path (copy renderbuffer to RAM)");
        let pixels_vec = get_pixels_vec_for_presenting(env, drawable);
        let cached_readback_fbo = env
            .objc
            .borrow::<EAGLContextHostObject>(this)
            .readback_fbo;
        // re-borrow
        let read_profile = crate::ds_perf::Timer::new(crate::ds_perf::Stage::Readback);
        let read_result = {
            let maybe_gles = super::sync_context(
                &mut env.framework_state.opengles,
                &mut env.objc,
                env.window.as_mut().unwrap(),
                env.current_thread,
            );
            match maybe_gles {
                Some(mut gles) => Some(unsafe {
                    read_renderbuffer(
                        gles.as_mut(),
                        renderbuffer,
                        pixels_vec,
                        cached_readback_fbo,
                    )
                }),
                None => {
                    log!(
                        "[EAGLContext presentRenderbuffer:{:#x}] lost GL \
                         context for thread {} between fast-path and \
                         slow-path; skipping copy-back.",
                        target,
                        env.current_thread
                    );
                    None
                }
            }
        };
        drop(read_profile);
        let Some((mut pixels_vec, width, height, readback_fbo)) = read_result else {
            finish_present_pacing(env, present_deadline);
            return false;
        };
        env.objc
            .borrow_mut::<EAGLContextHostObject>(this)
            .readback_fbo = Some(readback_fbo);
        {
            let _profile = crate::ds_perf::Timer::new(crate::ds_perf::Stage::Overlay);
            crate::ds_host_overlay::compose(&mut pixels_vec, width, height);
        }
        present_pixels(env, drawable, pixels_vec, width, height);

        // The slow path stores the freshly rendered frame in `presented_pixels`
        // on the drawable, but the *screen* is only updated when the Core
        // Animation compositor runs. The compositor is normally driven from
        // NSRunLoop iterations, but some games (notably Temple Run) drive
        // their own render loop and only spin NSRunLoop very rarely, so the
        // screen would otherwise update at well below 2 FPS even though the
        // app is rendering at 60 FPS.
        //
        // Apple's documentation for `-[EAGLContext presentRenderbuffer:]`
        // states that the contents of the renderbuffer are displayed when
        // this method returns. To honour that contract — and to keep the
        // composited overlay (UIKit controls drawn on top of the EAGL view)
        // in step with the rendered frame — drive the compositor here
        // explicitly when we are on this slow path. We pass `force: true`
        // so the compositor's 60Hz throttle doesn't suppress this tick;
        // recomposite_if_necessary still updates its internal scheduler so
        // NSRunLoop-driven ticks remain rate-limited.
        {
            let _profile = crate::ds_perf::Timer::new(crate::ds_perf::Stage::Composition);
            crate::frameworks::core_animation::recomposite_if_necessary(env, true);
        }
    }

    // Restore the multisample target so the next frame continues rendering
    // into the MSAA FBO rather than the resolved single-sample image.
    if let Some((msaa_fbo, _)) = msaa_resolve {
        let window = env
            .window
            .as_mut()
            .expect("OpenGL ES is not supported in headless mode");
        if let Some(mut gles) = super::sync_context(
            &mut env.framework_state.opengles,
            &mut env.objc,
            window,
            env.current_thread,
        ) {
            unsafe {
                gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, msaa_fbo);
            }
        }
    }

    finish_present_pacing(env, present_deadline);

    true
}

@end

};

#[path = "frame_pacing.rs"]
mod frame_pacing;

fn finish_present_pacing(env: &mut Environment, deadline: Option<Instant>) {
    if let Some(remaining) = frame_pacing::remaining(deadline, Instant::now()) {
        env.sleep(remaining);
    }
}

/// Choose an absolute deadline before presenting; only its remaining time is
/// slept after the GPU work and swap. Preserve at most one interval of slop.
fn limit_framerate(next_frame_due: &mut Option<Instant>, options: &Options) -> Option<Instant> {
    frame_pacing::deadline(next_frame_due, crate::ds_perf_control::fps_limit(options.fps_limit), Instant::now())
}

// These helper functions make the state backup code easier to read, but
// more importantly, they make it free of mutable variables that wouldn't
// get caught by Rust's unused variable warnings, which are useful to check
// we actually restore the stuff we back up.

unsafe fn get_ptr(gles: &mut dyn GLES, pname: GLenum) -> *const GLvoid {
    let mut ptr = std::ptr::null();
    gles.GetPointerv(pname, &mut ptr);
    ptr
}
// Safety: caller's responsibility to use appropriate N.
unsafe fn get_ints<const N: usize>(gles: &mut dyn GLES, pname: GLenum) -> [GLint; N] {
    let mut res = [0; N];
    gles.GetIntegerv(pname, res.as_mut_ptr());
    res
}
// Safety: caller's responsibility to only use this for scalars.
unsafe fn get_int(gles: &mut dyn GLES, pname: GLenum) -> GLint {
    get_ints::<1>(gles, pname)[0]
}
// Safety: caller's responsibility to use appropriate N.
unsafe fn get_tex_env_ints<const N: usize>(
    gles: &mut dyn GLES,
    target: GLenum,
    pname: GLenum,
) -> [GLint; N] {
    let mut res = [0; N];
    gles.GetTexEnviv(target, pname, res.as_mut_ptr());
    res
}
// Safety: caller's responsibility to only use this for scalars.
unsafe fn get_tex_env_int(gles: &mut dyn GLES, target: GLenum, pname: GLenum) -> GLint {
    get_tex_env_ints::<1>(gles, target, pname)[0]
}
// Safety: caller's responsibility to use appropriate N.
unsafe fn get_floats<const N: usize>(gles: &mut dyn GLES, pname: GLenum) -> [GLfloat; N] {
    let mut res = [0.0; N];
    gles.GetFloatv(pname, res.as_mut_ptr());
    res
}
unsafe fn get_renderbuffer_size(gles: &mut dyn GLES, renderbuffer: GLuint) -> (GLsizei, GLsizei) {
    // The guest may have unbound the drawable renderbuffer (e.g. Dead Space
    // binds renderbuffer 0 before `presentRenderbuffer:`), so bind the one we
    // actually want to measure and restore the previous binding afterwards.
    let old: GLuint = get_int(gles, gles11::RENDERBUFFER_BINDING_OES) as _;
    if old != renderbuffer {
        gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, renderbuffer);
    }
    let mut width: GLint = 0;
    let mut height: GLint = 0;
    gles.GetRenderbufferParameterivOES(
        gles11::RENDERBUFFER_OES,
        gles11::RENDERBUFFER_WIDTH_OES,
        &mut width,
    );
    gles.GetRenderbufferParameterivOES(
        gles11::RENDERBUFFER_OES,
        gles11::RENDERBUFFER_HEIGHT_OES,
        &mut height,
    );
    if old != renderbuffer {
        gles.BindRenderbufferOES(gles11::RENDERBUFFER_OES, old);
    }
    (width, height)
}

/// Debug helper: write an RGBA8 buffer out as a binary PPM (P6), flipping the
/// rows because OpenGL's origin is bottom-left while PPM's is top-left.
fn dump_rgba_ppm(dir: &str, n: u64, width: u32, height: u32, rgba: &[u8]) {
    use std::io::Write;
    if width == 0 || height == 0 || rgba.len() < (width as usize) * (height as usize) * 4 {
        return;
    }
    let dir = dir.trim_end_matches('/').trim_end_matches('\\');
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = format!("{}/frame_{:05}.ppm", dir, n);
    let Ok(mut f) = std::fs::File::create(&path) else {
        return;
    };
    let _ = write!(f, "P6\n{} {}\n255\n", width, height);
    let w = width as usize;
    let mut row = vec![0u8; w * 3];
    for y in (0..height as usize).rev() {
        let src = &rgba[y * w * 4..];
        for x in 0..w {
            row[x * 3] = src[x * 4];
            row[x * 3 + 1] = src[x * 4 + 1];
            row[x * 3 + 2] = src[x * 4 + 2];
        }
        let _ = f.write_all(&row);
    }
}

/// Copies the pixels in a renderbuffer bound to `GL_RENDERBUFFER_BINDING_OES`
/// (which should be provided by the app) to a provided [Vec], trying to avoid
/// noticeably modifying OpenGL ES state while doing so.
///
/// This uses `glReadPixels()`, with all the associated performance risks. Any
/// existing content in the [Vec] will bereplaced. The format is RGBA8.
/// The returned values are the [Vec], the width and height.
///
/// The provided context must be current.
unsafe fn read_renderbuffer(
    gles: &mut dyn GLES,
    renderbuffer: GLuint,
    mut pixel_buffer: Vec<u8>,
    cached_framebuffer: Option<GLuint>,
) -> (Vec<u8>, u32, u32, GLuint) {
    let (width, height) = get_renderbuffer_size(gles, renderbuffer);
    let width_u32: u32 = width.try_into().unwrap();
    let height_u32: u32 = height.try_into().unwrap();

    // To avoid confusing the guest app, we need to be able to undo any
    // state changes we make.
    let old_framebuffer: GLuint = get_int(gles, gles11::FRAMEBUFFER_BINDING_OES) as _;

    // Reuse the context-owned source FBO, but reattach the renderbuffer every
    // time. GL renderbuffer names can be recycled, so caching the attachment
    // itself can silently read a deleted/replacement buffer.
    let mut src_framebuffer = cached_framebuffer.unwrap_or(0);
    if src_framebuffer == 0 {
        gles.GenFramebuffersOES(1, &mut src_framebuffer);
    }
    gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, src_framebuffer);
    gles.FramebufferRenderbufferOES(
        gles11::FRAMEBUFFER_OES,
        gles11::COLOR_ATTACHMENT0_OES,
        gles11::RENDERBUFFER_OES,
        renderbuffer,
    );

    // Tile-based drivers normally need an explicit resolve before readback.
    // Keep Finish as the safe default; allow a device-validated diagnostic
    // override because glReadPixels already implies a flush on some drivers.
    if !crate::ds_perf_control::readback_finish_enabled() {
        log_once!("[present] skipping readback glFinish (diagnostic)");
    } else {
        gles.Finish();
    }

    // Read the pixels
    let size = (width_u32 as usize)
        .checked_mul(height_u32 as usize)
        .unwrap()
        .checked_mul(4)
        .unwrap();
    pixel_buffer.clear();
    pixel_buffer.reserve_exact(size);
    let before = Instant::now();
    gles.ReadPixels(
        0,
        0,
        width,
        height,
        gles11::RGBA,
        gles11::UNSIGNED_BYTE,
        pixel_buffer.as_mut_ptr() as *mut _,
    );
    log_dbg!(
        "glReadPixels(0, 0, {}, {}, …) took {:?}",
        width,
        height,
        Instant::now().saturating_duration_since(before)
    );
    pixel_buffer.set_len(size);

    // Restore the framebuffer binding
    gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, old_framebuffer);

    (pixel_buffer, width_u32, height_u32, src_framebuffer)
}

/// Shader-based variant of the renderbuffer presenter, used when the
/// underlying driver is a real OpenGL ES 2.0 driver (no fixed-function
/// pipeline available).
///
/// This is intentionally simpler than the fixed-function version: we save the
/// minimum amount of ES 2.0 state, draw the textured quad with a small
/// dedicated shader program, and restore. The app's matrices, vertex pointers
/// etc. are not part of ES 2.0 state and thus need no save/restore.
unsafe fn present_renderbuffer_es2(
    gles: &mut dyn GLES,
    state: &mut TransferState,
    present_state: &mut crate::gles::present::PresentState,
    text_overlay: &mut crate::ds_host_overlay::GpuState,
    renderbuffer: GLuint,
    viewport: (u32, u32, u32, u32),
    rotation_matrix: crate::matrix::Matrix<2>,
    virtual_cursor_visible_at: Option<(f32, f32, bool)>,
) {
    use crate::gles::gles2_raw as gles2;

    // Guest error flags were already consumed at the end of this host pass.
    // Separate incoming errors before attributing uploads/uniform checks.
    while gles.GetError()!=0 {}
    // Save state we are about to clobber
    let mut old_program: GLint = 0;
    gles.GetIntegerv(gles2::CURRENT_PROGRAM, &mut old_program);
    let mut old_array_buffer: GLint = 0;
    gles.GetIntegerv(gles2::ARRAY_BUFFER_BINDING, &mut old_array_buffer);
    let mut old_elem_buffer: GLint = 0;
    gles.GetIntegerv(gles2::ELEMENT_ARRAY_BUFFER_BINDING, &mut old_elem_buffer);
    let mut old_active_texture: GLint = 0;
    gles.GetIntegerv(gles2::ACTIVE_TEXTURE, &mut old_active_texture);
    gles.ActiveTexture(gles2::TEXTURE0);
    let mut old_texture: GLint = 0;
    gles.GetIntegerv(gles2::TEXTURE_BINDING_2D, &mut old_texture);
    let mut old_framebuffer: GLint = 0;
    gles.GetIntegerv(gles2::FRAMEBUFFER_BINDING, &mut old_framebuffer);
    let mut old_viewport = [0i32; 4];
    gles.GetIntegerv(gles2::VIEWPORT, old_viewport.as_mut_ptr());
    let mut old_clear_color = [0.0f32; 4];
    gles.GetFloatv(gles2::COLOR_CLEAR_VALUE, old_clear_color.as_mut_ptr());
    let depth_test_was_on = gles.IsEnabled(gles2::DEPTH_TEST) != 0;
    let cull_was_on = gles.IsEnabled(gles2::CULL_FACE) != 0;
    let blend_was_on = gles.IsEnabled(gles2::BLEND) != 0;
    let mut old_blend=[0i32;4];
    for (key,value) in [gles2::BLEND_SRC_RGB,gles2::BLEND_DST_RGB,gles2::BLEND_SRC_ALPHA,gles2::BLEND_DST_ALPHA].into_iter().zip(old_blend.iter_mut()) {gles.GetIntegerv(key,value);}

    let scissor_was_on = gles.IsEnabled(gles2::SCISSOR_TEST) != 0;
    let stencil_was_on = gles.IsEnabled(gles2::STENCIL_TEST) != 0;
    let mut old_color_mask=[0u8;4];
    gles.GetBooleanv(gles2::COLOR_WRITEMASK,old_color_mask.as_mut_ptr());
    let mut old_renderbuffer=0;
    gles.GetIntegerv(gles2::RENDERBUFFER_BINDING,&mut old_renderbuffer);
    gles.BindRenderbuffer(gles2::RENDERBUFFER,renderbuffer);

    // Resolve renderbuffer → texture with a cached FBO + the ES 2.0 entry
    // points.
    let (width, height) = {
        let mut w: GLint = 0;
        let mut h: GLint = 0;
        gles.GetRenderbufferParameteriv(gles2::RENDERBUFFER, gles2::RENDERBUFFER_WIDTH, &mut w);
        gles.GetRenderbufferParameteriv(gles2::RENDERBUFFER, gles2::RENDERBUFFER_HEIGHT, &mut h);
        (w, h)
    };

    let present_objects = ensure_present_objects(gles, state);
    if width > 0 && height > 0 {
        // Reuse one FBO as the copy source. Reattach only when the app
        // creates a new EAGL renderbuffer.
        let source_renderbuffer = state.source_renderbuffer;
        gles.BindFramebuffer(gles2::FRAMEBUFFER, present_objects.framebuffer);
        if source_renderbuffer != renderbuffer as GLuint {
            gles.FramebufferRenderbuffer(
                gles2::FRAMEBUFFER,
                gles2::COLOR_ATTACHMENT0,
                gles2::RENDERBUFFER,
                renderbuffer as GLuint,
            );
            state.source_renderbuffer = renderbuffer as GLuint;
        }

        // Copy into preallocated texture storage. CopyTexImage2D reallocates
        // that storage on every frame, while CopyTexSubImage2D does not.
        gles.ActiveTexture(gles2::TEXTURE0);
        gles.BindTexture(gles2::TEXTURE_2D, present_objects.texture);
        let texture_size = state.texture_size;
        if texture_size != Some((width, height)) {
            gles.TexImage2D(
                gles2::TEXTURE_2D,
                0,
                gles2::RGBA as GLint,
                width,
                height,
                0,
                gles2::RGBA,
                gles2::UNSIGNED_BYTE,
                std::ptr::null(),
            );
            state.texture_size = Some((width, height));
        }
        gles.CopyTexSubImage2D(gles2::TEXTURE_2D, 0, 0, 0, 0, 0, width, height);
        gles.BindFramebuffer(gles2::FRAMEBUFFER, old_framebuffer as _);
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| log!("GLES2 presenter: cached FBO and glCopyTexSubImage2D fast path"));
    }

    gles.ActiveTexture(gles2::TEXTURE0);
    gles.BindTexture(gles2::TEXTURE_2D, present_objects.texture);
    gles.BindBuffer(gles2::ARRAY_BUFFER, present_objects.quad_vbo);
    gles.BindFramebuffer(gles2::FRAMEBUFFER, 0);

    // Configure the destination viewport (the window) and clear.
    gles.Viewport(
        viewport.0 as _,
        viewport.1 as _,
        viewport.2 as _,
        viewport.3 as _,
    );
    gles.ClearColor(0.0, 0.0, 0.0, 1.0);
    gles.Disable(gles2::DEPTH_TEST);
    gles.Disable(gles2::CULL_FACE);
    gles.Disable(gles2::BLEND);
    gles.Disable(gles2::SCISSOR_TEST);
    gles.Disable(gles2::STENCIL_TEST);
    gles.ColorMask(1,1,1,1);
    gles.Clear(gles2::COLOR_BUFFER_BIT);

    // Compile the present shader program once and cache it. If the shader
    // fails to compile/link (e.g. on a host with a buggy GLSL ES driver),
    // skip the present pass instead of crashing — the previous frame
    // remains on screen and the app continues to run.
    let Some(program) = ensure_present_program(gles, state) else {
        log!("Warning: present_renderbuffer_es2: present shader unavailable, skipping frame.");
        gles.UseProgram(if old_program > 0 {
            old_program as GLuint
        } else {
            0
        });
        gles.BindBuffer(gles2::ARRAY_BUFFER, old_array_buffer as GLuint);
        gles.BindBuffer(gles2::ELEMENT_ARRAY_BUFFER, old_elem_buffer as GLuint);
        gles.BindFramebuffer(gles2::FRAMEBUFFER, old_framebuffer as GLuint);
        gles.BindTexture(gles2::TEXTURE_2D, old_texture as GLuint);
        gles.ActiveTexture(old_active_texture as GLenum);
        gles.Viewport(
            old_viewport[0],
            old_viewport[1],
            old_viewport[2],
            old_viewport[3],
        );
        gles.ClearColor(
            old_clear_color[0],
            old_clear_color[1],
            old_clear_color[2],
            old_clear_color[3],
        );
        if depth_test_was_on {
            gles.Enable(gles2::DEPTH_TEST);
        }
        if cull_was_on {
            gles.Enable(gles2::CULL_FACE);
        }
        if blend_was_on {
            gles.Enable(gles2::BLEND);
        }
        if scissor_was_on {
            gles.Enable(gles2::SCISSOR_TEST);
        }
        gles.BindRenderbuffer(gles2::RENDERBUFFER,old_renderbuffer as u32);
    gles.BlendFuncSeparate(old_blend[0] as u32,old_blend[1] as u32,old_blend[2] as u32,old_blend[3] as u32);
        gles.ColorMask(old_color_mask[0],old_color_mask[1],old_color_mask[2],old_color_mask[3]);
        if stencil_was_on { gles.Enable(gles2::STENCIL_TEST); }
        return;
    };

    // The presenter only changes the two attributes it owns. Querying all 16
    // slots on every frame is unnecessary driver traffic.
    let saved_attributes=[
        crate::gles::present_attrib::SavedAttrib::capture(gles,program.a_pos as u32),
        crate::gles::present_attrib::SavedAttrib::capture(gles,program.a_uv as u32),
    ];
    gles.UseProgram(program.program);
    gles.Uniform1i(program.u_tex, 0);
    gles.Uniform4f(program.u_solid,0.0,0.0,0.0,0.0);
    let r = crate::matrix::Matrix::<4>::from(&rotation_matrix);
    let m = crate::matrix::Matrix::<4>::translate_3d(-0.5,-0.5,0.0)
        .multiply(&r).multiply(&crate::matrix::Matrix::<4>::translate_3d(0.5,0.5,0.0));
    let cols = m.columns();
    gles.UniformMatrix4fv(
        program.u_tex_mat,
        1,
        gles2::FALSE,
        cols.as_ptr() as *const _,
    );

    // The full-screen quad lives in `present_objects.quad_vbo`, uploaded
    // above. Core-profile desktop GL (and the GL-on-Vulkan Zink driver used
    // by Winlator on Android) reject client-side vertex arrays with
    // GL_INVALID_OPERATION, so we must source the quad from a real buffer
    // object — never from a host-memory pointer.
    gles.BindBuffer(gles2::ARRAY_BUFFER, present_objects.quad_vbo);
    gles.EnableVertexAttribArray(program.a_pos as _);
    gles.EnableVertexAttribArray(program.a_uv as _);
    gles.VertexAttribPointer(
        program.a_pos as _,
        2,
        gles2::FLOAT,
        gles2::FALSE,
        16,
        std::ptr::null(),
    );
    gles.VertexAttribPointer(
        program.a_uv as _,
        2,
        gles2::FLOAT,
        gles2::FALSE,
        16,
        8usize as *const _,
    );
    let source_size=(width.max(0) as u32,height.max(0) as u32);
    let processed=crate::gles::present::draw_present_processed_quad(
        gles,present_state,&m,Some(source_size),viewport,false).drawn;
    if !processed { gles.DrawArrays(gles2::TRIANGLES,0,6); }
    text_overlay.draw(gles,source_size,rotation_matrix);
    crate::gles::present::maybe_capture_present_backbuffer(gles,viewport);


    // Draw the desktop pointer after all scaling, in drawable coordinates.
    if crate::window::ds_native_pointer_enabled() {
        if let Some(at)=virtual_cursor_visible_at {
            gles.BindBuffer(gles2::ARRAY_BUFFER,present_objects.cursor_vbo);
            gles.DisableVertexAttribArray(program.a_uv as _);
            gles.VertexAttribPointer(program.a_pos as _,2,gles2::FLOAT,gles2::FALSE,0,std::ptr::null());
            for (vertices,color) in crate::ds_pointer::layers(viewport,at) {
                gles.BufferData(gles2::ARRAY_BUFFER,std::mem::size_of_val(&vertices) as isize,vertices.as_ptr().cast(),gles2::STREAM_DRAW);
                gles.Uniform4f(program.u_solid,color[0],color[1],color[2],color[3]);
                gles.DrawArrays(gles2::TRIANGLES,0,12);
            }
            gles.Uniform4f(program.u_solid,0.0,0.0,0.0,0.0);
        }
    }
    // Optional: virtual cursor.
    if let Some((cx, cy, pressed)) = virtual_cursor_visible_at {
        let (vx, vy, vw, vh) = viewport;
        let x = cx - vx as f32;
        let y = cy - vy as f32;
        let radius = 10.0_f32;
        // Build quad in NDC.
        let mut q: [f32; 24] = [
            -1.0, -1.0, 0.0, 0.0, 1.0, -1.0, 1.0, 0.0, -1.0, 1.0, 0.0, 1.0, 1.0, -1.0, 1.0, 0.0,
            1.0, 1.0, 1.0, 1.0, -1.0, 1.0, 0.0, 1.0,
        ];
        for i in (0..q.len()).step_by(4) {
            q[i] = (q[i] * radius + x) / (vw as f32 / 2.0) - 1.0;
            q[i + 1] = 1.0 - (q[i + 1] * radius + y) / (vh as f32 / 2.0);
        }
        // Use a solid black quasi-shadow via a separate program, but to keep
        // things simple just sample our present texture with very low alpha
        // — skip for now if no separate cursor shader.
        let _ = pressed;
    }

    for value in saved_attributes { value.restore(gles); }

    // Restore state we touched
    gles.UseProgram(if old_program > 0 {
        old_program as GLuint
    } else {
        0
    });
    gles.BindBuffer(gles2::ARRAY_BUFFER, old_array_buffer as GLuint);
    gles.BindBuffer(gles2::ELEMENT_ARRAY_BUFFER, old_elem_buffer as GLuint);
    gles.BindFramebuffer(gles2::FRAMEBUFFER, old_framebuffer as GLuint);
    gles.BindTexture(gles2::TEXTURE_2D, old_texture as GLuint);
    gles.ActiveTexture(old_active_texture as GLenum);
    gles.Viewport(
        old_viewport[0],
        old_viewport[1],
        old_viewport[2] as _,
        old_viewport[3] as _,
    );
    gles.ClearColor(
        old_clear_color[0],
        old_clear_color[1],
        old_clear_color[2],
        old_clear_color[3],
    );
    if depth_test_was_on {
        gles.Enable(gles2::DEPTH_TEST);
    }
    if cull_was_on {
        gles.Enable(gles2::CULL_FACE);
    }
    if blend_was_on {
        gles.Enable(gles2::BLEND);
    }
    if scissor_was_on {
        gles.Enable(gles2::SCISSOR_TEST);
    }

    gles.BindRenderbuffer(gles2::RENDERBUFFER,old_renderbuffer as u32);
    gles.BlendFuncSeparate(old_blend[0] as u32,old_blend[1] as u32,old_blend[2] as u32,old_blend[3] as u32);
    gles.ColorMask(old_color_mask[0],old_color_mask[1],old_color_mask[2],old_color_mask[3]);
    if stencil_was_on { gles.Enable(gles2::STENCIL_TEST); }

    // The renderbuffer present is performed by us (the emulator), not by the
    // guest app. On a real iPhone OS device the equivalent work happens
    // inside `-[EAGLContext presentRenderbuffer:]` / the windowserver, behind
    // a context the app never error-checks. Our present path runs in the
    // *same* host GL context and therefore shares a single `glGetError` error
    // queue with the guest. Any error our own present operations might queue
    // (e.g. a benign INVALID_OPERATION from a strict desktop GL / Zink core
    // profile that the guest's lenient iOS PowerVR/Adreno driver would never
    // raise) would otherwise be read back by the guest's *next* GL call and
    // misattributed to the app — this is exactly the per-frame
    // `OpenGLES error 0x0502 in .../GlesHelper.mm` flood seen on Unity titles.
    // Drain the queue so the guest only ever observes errors it actually
    // caused, matching the real-device contract.
    while gles.GetError() != 0 {}
}

unsafe fn prepare_es1_source_framebuffer(gl: &mut dyn GLES, state: &mut TransferState) -> GLuint {
    if state.es1_source_fbo == 0 { gl.GenFramebuffersOES(1, &mut state.es1_source_fbo); }
    state.es1_source_fbo
}

// Kept separate so real-context regressions exercise the production cache path.
unsafe fn prepare_es1_present_texture(gles: &mut dyn GLES, state: &mut TransferState, width: GLint, height: GLint) -> (GLuint, bool) {
    let mut texture: GLuint;
    let mut storage_valid = false;
    let cached_size = state.es1_texture;
    if let Some((tex, w, h)) = cached_size {
        texture = tex;
        gles.BindTexture(gles11::TEXTURE_2D, texture);
        storage_valid = w == width && h == height;
    } else {
        // Set the texture parameters BEFORE the copy: the renderbuffer is
        // typically non-power-of-two (e.g. 640x1136), and on strict ES 1.1
        // drivers (ANGLE's GLES1 front-end, Mali) a CopyTexImage2D into a
        // texture whose wrap mode is still the GL_REPEAT default fails with
        // GL_INVALID_ENUM, leaving an incomplete texture and a black frame.
        texture = 0;
        gles.GenTextures(1, &mut texture);
        gles.BindTexture(gles11::TEXTURE_2D, texture);
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
    }
    (texture, storage_valid)
}

#[derive(Copy, Clone)]
struct PresentProgram {
    program: GLuint,
    a_pos: GLint,
    a_uv: GLint,
    u_tex: GLint,
    u_tex_mat: GLint,
    u_solid: GLint,
}

/// Reusable GL objects for the ES 2.0 present path. Allocating and freeing a
/// framebuffer object and a texture on *every* presented frame is very
/// expensive on tile-based mobile GPUs (e.g. ARM Mali, Qualcomm Adreno):
/// object creation/teardown forces driver-side synchronisation and breaks
/// the driver's ability to pipeline frames, which shows up as severe stutter
/// in 60 FPS games (notably Unity titles, which present through this path).
/// Caching the objects and reusing them across frames removes that per-frame
/// churn entirely. The texture's storage is redefined each frame by
/// `glCopyTexImage2D`, so a resolution change needs no special handling.
#[derive(Copy, Clone)]
struct PresentObjects {
    framebuffer: GLuint,
    texture: GLuint,
    quad_vbo: GLuint,
    cursor_vbo: GLuint,
}

#[path = "present_buffer.rs"]
mod present_buffer;

// CPU-only scratch may be reused on the one host thread. It has no GL names.
thread_local! {
    static PRESENT_ES1_READBACK: std::cell::RefCell<present_buffer::RgbaBuffer> =
        const { std::cell::RefCell::new(present_buffer::RgbaBuffer::new()) };
}

// This state belongs to one actual EAGL backing GL context. A shared object
// namespace does not justify sharing helper attachment/storage history.
#[derive(Default)]
struct TransferState {
    program: Option<PresentProgram>,
    objects: Option<PresentObjects>,
    source_renderbuffer: GLuint,
    texture_size: Option<(GLint, GLint)>,
    es1_texture: Option<(GLuint, GLint, GLint)>,
    es1_source_fbo: GLuint,
}

impl TransferState {
    /// Only call while this state's backing GL context is current.
    unsafe fn release(&mut self, gl: &mut dyn GLES) {
        if let Some(objects) = self.objects.take() {
            gl.DeleteFramebuffers(1, &objects.framebuffer);
            gl.DeleteTextures(1, &objects.texture);
            gl.DeleteBuffers(1, &objects.quad_vbo);
            gl.DeleteBuffers(1, &objects.cursor_vbo);
        }
        if let Some(program) = self.program.take() { gl.DeleteProgram(program.program); }
        if let Some((texture, _, _)) = self.es1_texture.take() { gl.DeleteTextures(1, &texture); }
        if self.es1_source_fbo != 0 {
            gl.DeleteFramebuffersOES(1, &self.es1_source_fbo);
            self.es1_source_fbo = 0;
        }
        self.source_renderbuffer = 0;
        self.texture_size = None;
    }
}

unsafe fn ensure_present_program(gles: &mut dyn GLES, state: &mut TransferState) -> Option<PresentProgram> {
    use crate::gles::gles2_raw as gles2;
    if let Some(p) = state.program {
        return Some(p);
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
        uniform vec4 uSolid;\n\
        void main() {\n\
            gl_FragColor = mix(texture2D(uTex, vUV), uSolid, uSolid.a);\n\
        }\0";

    let vs = gles.CreateShader(gles2::VERTEX_SHADER);
    let vs_ptr = vs_src.as_ptr() as *const _;
    let vs_len = (vs_src.len() - 1) as GLint;
    gles.ShaderSource(vs, 1, &vs_ptr, &vs_len);
    gles.CompileShader(vs);
    let mut ok: GLint = 0;
    gles.GetShaderiv(vs, gles2::COMPILE_STATUS, &mut ok);
    if ok == 0 {
        let mut buf = [0u8; 1024];
        let mut len: GLsizei = 0;
        gles.GetShaderInfoLog(vs, 1024, &mut len, buf.as_mut_ptr() as *mut _);
        let s = std::str::from_utf8(std::slice::from_raw_parts(
            buf.as_ptr() as *const u8,
            len as _,
        ))
        .unwrap_or("?");
        log!("Warning: present_es2 vertex shader compile failed: {s}");
        gles.DeleteShader(vs);
        return None;
    }

    let fs = gles.CreateShader(gles2::FRAGMENT_SHADER);
    let fs_ptr = fs_src.as_ptr() as *const _;
    let fs_len = (fs_src.len() - 1) as GLint;
    gles.ShaderSource(fs, 1, &fs_ptr, &fs_len);
    gles.CompileShader(fs);
    gles.GetShaderiv(fs, gles2::COMPILE_STATUS, &mut ok);
    if ok == 0 {
        let mut buf = [0u8; 1024];
        let mut len: GLsizei = 0;
        gles.GetShaderInfoLog(fs, 1024, &mut len, buf.as_mut_ptr() as *mut _);
        let s = std::str::from_utf8(std::slice::from_raw_parts(
            buf.as_ptr() as *const u8,
            len as _,
        ))
        .unwrap_or("?");
        log!("Warning: present_es2 fragment shader compile failed: {s}");
        gles.DeleteShader(vs);
        gles.DeleteShader(fs);
        return None;
    }

    let prog = gles.CreateProgram();
    gles.AttachShader(prog, vs);
    gles.AttachShader(prog, fs);
    // Bind to high attribute slots so we never collide with the app's
    // attribute layout (which typically starts at 0).
    gles.BindAttribLocation(prog, 6, b"aPos\0".as_ptr() as *const _);
    gles.BindAttribLocation(prog, 7, b"aUV\0".as_ptr() as *const _);
    gles.LinkProgram(prog);
    gles.GetProgramiv(prog, gles2::LINK_STATUS, &mut ok);
    if ok == 0 {
        let mut buf = [0u8; 1024];
        let mut len: GLsizei = 0;
        gles.GetProgramInfoLog(prog, 1024, &mut len, buf.as_mut_ptr() as *mut _);
        let s = std::str::from_utf8(std::slice::from_raw_parts(
            buf.as_ptr() as *const u8,
            len as _,
        ))
        .unwrap_or("?");
        log!("Warning: present_es2 program link failed: {s}");
        gles.DeleteShader(vs);
        gles.DeleteShader(fs);
        gles.DeleteProgram(prog);
        return None;
    }

    let a_pos = gles.GetAttribLocation(prog, b"aPos\0".as_ptr() as *const _);
    let a_uv = gles.GetAttribLocation(prog, b"aUV\0".as_ptr() as *const _);
    let u_tex = gles.GetUniformLocation(prog, b"uTex\0".as_ptr() as *const _);
    let u_tex_mat = gles.GetUniformLocation(prog, b"uTexMat\0".as_ptr() as *const _);

    let result = PresentProgram {
        program: prog,
        a_pos,
        a_uv,
        u_tex,
        u_tex_mat,
        u_solid: gles.GetUniformLocation(prog,b"uSolid\0".as_ptr().cast()),
    };
    state.program = Some(result);
    Some(result)
}

/// Returns the reusable framebuffer + texture used by [present_renderbuffer_es2]
/// to resolve the guest renderbuffer into a presentable texture.
///
/// These objects used to be created with `glGenFramebuffers` / `glGenTextures`
/// and destroyed with `glDeleteFramebuffers` / `glDeleteTextures` on *every*
/// presented frame. On tile-based mobile GPUs (ARM Mali, Qualcomm Adreno,
/// PowerVR) allocating and freeing driver objects every frame is surprisingly
/// expensive — it forces driver-side bookkeeping and synchronization that can
/// dominate frame time, which is why a powerful Helio G99 (Mali-G57) could run
/// a Unity game far more slowly than a real iPhone 4S, where iOS forces 60Hz
/// v-sync and never does this churn. Caching the objects per GL context and reusing
/// them every frame removes that churn entirely.
unsafe fn ensure_present_objects(gles: &mut dyn GLES, state: &mut TransferState) -> PresentObjects {
    if let Some(o) = state.objects {
        return o;
    }

    let mut framebuffer: GLuint = 0;
    gles.GenFramebuffers(1, &mut framebuffer);
    let mut texture: GLuint = 0;
    gles.GenTextures(1, &mut texture);
    let mut quad_vbo: GLuint = 0;
    gles.GenBuffers(1, &mut quad_vbo);
    let mut cursor_vbo: GLuint = 0;
    gles.GenBuffers(1, &mut cursor_vbo);
    gles.BindBuffer(crate::gles::gles2_raw::ARRAY_BUFFER, quad_vbo);
    #[rustfmt::skip]
    let verts: [f32; 24] = [
        -1.0, -1.0, 0.0, 0.0,
         1.0, -1.0, 1.0, 0.0,
        -1.0,  1.0, 0.0, 1.0,
         1.0, -1.0, 1.0, 0.0,
         1.0,  1.0, 1.0, 1.0,
        -1.0,  1.0, 0.0, 1.0,
    ];
    gles.BufferData(
        crate::gles::gles2_raw::ARRAY_BUFFER,
        std::mem::size_of_val(&verts) as isize,
        verts.as_ptr().cast(),
        crate::gles::gles2_raw::STATIC_DRAW,
    );
    gles.ActiveTexture(crate::gles::gles2_raw::TEXTURE0);
    gles.BindTexture(crate::gles::gles2_raw::TEXTURE_2D, texture);
    gles.TexParameteri(
        crate::gles::gles2_raw::TEXTURE_2D,
        crate::gles::gles2_raw::TEXTURE_MIN_FILTER,
        crate::gles::gles2_raw::LINEAR as _,
    );
    gles.TexParameteri(
        crate::gles::gles2_raw::TEXTURE_2D,
        crate::gles::gles2_raw::TEXTURE_MAG_FILTER,
        crate::gles::gles2_raw::LINEAR as _,
    );
    gles.TexParameteri(
        crate::gles::gles2_raw::TEXTURE_2D,
        crate::gles::gles2_raw::TEXTURE_WRAP_S,
        crate::gles::gles2_raw::CLAMP_TO_EDGE as _,
    );
    gles.TexParameteri(
        crate::gles::gles2_raw::TEXTURE_2D,
        crate::gles::gles2_raw::TEXTURE_WRAP_T,
        crate::gles::gles2_raw::CLAMP_TO_EDGE as _,
    );

    let result = PresentObjects {
        framebuffer,
        texture,
        quad_vbo,
        cursor_vbo,
    };
    state.source_renderbuffer = 0;
    state.texture_size = None;
    state.objects = Some(result);
    result
}

/// Copies the pixels in a renderbuffer bound to `GL_RENDERBUFFER_BINDING_OES`
/// (which should be provided by the app) to a texture and presents it with
/// [present_frame], trying to avoid noticeably modifying OpenGL ES state while
/// doing so. The front and back buffers are then swapped.
/// Presents `renderbuffer`.
///
/// The caller passes the renderbuffer resolved from the EAGL context's
/// `renderbuffer_drawable_bindings`, because the guest often unbinds it
/// (binds renderbuffer 0) shortly before calling `presentRenderbuffer:` —
/// Dead Space 1.3.34 does exactly that, so re-reading
/// `GL_RENDERBUFFER_BINDING_OES` here would present the wrong (empty) buffer.
unsafe fn present_renderbuffer(env: &mut Environment, renderbuffer: GLuint, this: id) {
    // Match the actual backing context selected by get_thread_context,
    // including the existing main-thread compatibility fallback.
    let owner = env.framework_state.opengles.effective_ctx_id(env.current_thread).unwrap_or(this);
    let transfer_state = Rc::clone(&env.objc.borrow::<EAGLContextHostObject>(owner).transfer_state);
    let gpu_text_overlay = Rc::clone(&env.objc.borrow::<EAGLContextHostObject>(owner).gpu_text_overlay);
    // Capture this up front because the env borrow is moved into the GL
    // context machinery below.
    let trace_gl_errors = env.options.trace_gl_errors;

    // Save these for when we need to draw the frame
    let viewport = env.window.as_mut().unwrap().viewport();
    let device_family = env.window.as_mut().unwrap().device_family();
    let present_state = Rc::clone(&env.objc.borrow::<EAGLContextHostObject>(owner).present_state);
    let device_orientation = env.window.as_mut().unwrap().current_rotation();
    // For iPad apps in a non-portrait orientation, the UIKit auto-rotation
    // path (`UIWindow addSubview:` in ui_window.rs) applies a rotation
    // transform to the rootViewController's view so that the app, which
    // typically draws content "upright" inside the EAGL layer's portrait
    // bounds, ends up rotated for landscape display when Core Animation
    // composites it. touchHLE bypasses CA composition for EAGL apps that
    // call `presentRenderbuffer:` directly, so we have to replicate that
    // additional rotation here. Without it, iPad landscape games (e.g.
    // Plants vs. Zombies HD) render upside-down. iPhone-only landscape
    // games (e.g. Plants vs. Zombies, the iPhone version) typically rotate
    // their drawing themselves, so we must NOT apply the extra rotation
    // for them.
    // FIXME: A cleaner solution would be to read the actual transform from
    //        the EAGL layer's view hierarchy and apply it here, instead of
    //        using a device-family heuristic.
    let needs_autorotation_compensation = device_family.is_ipad()
        && !matches!(
            device_orientation,
            crate::window::DeviceOrientation::Portrait
        );
    let rotation_matrix = if std::env::var_os("TOUCHHLE_DISABLE_PRESENT_ROTATION").is_some() {
        log_once!(
            "TOUCHHLE_DISABLE_PRESENT_ROTATION=1: presenting EAGL renderbuffer without texture rotation"
        );
        crate::matrix::Matrix::<2>::identity()
    } else if needs_autorotation_compensation {
        env.window
            .as_mut()
            .unwrap()
            .rotation_matrix()
            .multiply(&crate::matrix::Matrix::z_rotation(std::f32::consts::PI))
    } else {
        env.window.as_mut().unwrap().rotation_matrix()
    };
    let virtual_cursor_visible_at = env.window.as_mut().unwrap().virtual_cursor_visible_at();
    // The GL context borrow below keeps `env.window` mutably borrowed for the
    // whole present function. Keep a raw pointer to the stable boxed Window so
    // the FSR3 double-present path can issue its mid-function swap without
    // fighting that borrow.
    let window_ptr = env.window.as_deref().unwrap() as *const crate::window::Window;

    let Some(gles_ctx) = super::get_thread_context(
        &mut env.framework_state.opengles,
        &mut env.objc,
        env.current_thread,
    ) else {
        // No current EAGL context for this thread. The caller already
        // returned `true` from `presentRenderbuffer:` (since the renderbuffer
        // existed in `renderbuffer_drawable_bindings`), so the only thing
        // left to do here is skip the host-side composition step and pump
        // a single SDL swap so the window keeps animating. Without this
        // guard the emulator used to abort with the panic visible in
        // HyperHLE log #5 (`opengles.rs:56` — Option::unwrap on a None
        // current_ctx).
        log!(
            "present_renderbuffer: no current GL context for thread {}; \
             skipping host present and swapping window only.",
            env.current_thread
        );
        if let Some(window) = env.window.as_ref() {
            window.swap_window();
        }
        return;
    };

    let mut gles_boxed = gles_ctx.make_current(env.window.as_mut().unwrap());
    let gles = gles_boxed.as_mut();
    let mut transfer = transfer_state.borrow_mut();

    // Per-section diagnostic checkpoint helper. When --trace-gl-errors is
    // on, this drains GL errors after each named section of
    // present_renderbuffer and logs the *first* time each named section
    // produces an error. Without an explicit per-section split, all
    // errors generated by our host-side present logic accumulate into
    // the GL error queue and are either silently drained at the end
    // (see below) or get incorrectly attributed to whatever guest gl*
    // call happens next, which makes it impossible to tell which of our
    // own state queries / FBO operations / texture uploads is the actual
    // culprit on a strict native ES 1.1 driver (e.g. ARM Mali, Qualcomm
    // Adreno's ES 1.1 surface). The caller-provided AtomicBool means
    // each unique checkpoint logs at most once for the entire app run,
    // so this stays out of normal logs even when an error reproduces
    // every frame. Implemented as a free function (not a macro) so each
    // call site is a clean re-borrow of `gles` for NLL.
    unsafe fn present_check(
        gles: &mut dyn GLES,
        trace: bool,
        seen: &std::sync::atomic::AtomicBool,
        section: &'static str,
    ) {
        if !trace {
            return;
        }
        let err = gles.GetError();
        if err == 0 {
            return;
        }
        if !seen.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log!(
                "[--trace-gl-errors] present_renderbuffer: section {:?} produced GL error {:#x} [this log will only be shown once]",
                section,
                err
            );
        }
        while gles.GetError() != 0 {}
    }

    // Drain anything the guest might have left in the queue so we can
    // attribute new errors below to our own code, not to whatever
    // happened before presentRenderbuffer:.
    if trace_gl_errors {
        while gles.GetError() != 0 {}
    }
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        present_check(gles, trace_gl_errors, &SEEN, "after make_current");
    }

    // On a real OpenGL ES 2.0 driver (Android etc.) the fixed-function code
    // path below cannot be used — there is no glMatrixMode / glColor4f /
    // glEnableClientState / glVertexPointer. Use a small dedicated
    // shader-based presenter instead.
    if gles.is_es2() {
        present_renderbuffer_es2(gles, &mut transfer, &mut present_state.borrow_mut(), &mut gpu_text_overlay.borrow_mut(), renderbuffer, viewport, rotation_matrix, virtual_cursor_visible_at);
        std::mem::drop(gles_boxed);
        env.window.as_ref().unwrap().swap_window();
        return;
    }

    // We can't directly copy the content of the renderbuffer to the default
    // framebuffer (the window), but if we attach it to a framebuffer object, we
    // can use glCopyTexImage2D() to copy it to a texture, which we can then
    // draw to the default framebuffer via a textured quad, which can be
    // rotated, scaled or letterboxed as appropriate.

    gles.gpu_profile_begin(4);
    let (width, height) = get_renderbuffer_size(gles, renderbuffer);
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after renderbuffer-size queries",
        );
    }

    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static SEEN: AtomicBool = AtomicBool::new(false);
        if !SEEN.swap(true, Ordering::Relaxed) {
            log!(
                "First present_renderbuffer ES1.1 path: renderbuffer={} size={}x{} (npot_w={} npot_h={}) [this log will only be shown once]",
                renderbuffer,
                width,
                height,
                !(width as u32).is_power_of_two(),
                !(height as u32).is_power_of_two(),
            );
        }
    }

    // To avoid confusing the guest app, we need to be able to undo any
    // state changes we make.
    let old_framebuffer: GLuint = get_int(gles, gles11::FRAMEBUFFER_BINDING_OES) as _;
    let old_texture_2d: GLuint = get_int(gles, gles11::TEXTURE_BINDING_2D) as _;
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after old_framebuffer/old_texture queries",
        );
    }

    // Tile-based GPUs (e.g. Qualcomm Adreno, ARM Mali) defer rasterisation:
    // the draws the app issued into the renderbuffer may still live in
    // fast tile-local memory at the moment the app calls
    // -presentRenderbuffer:. They are only "resolved" to the
    // renderbuffer's actual main-memory storage at well-defined points,
    // typically eglSwapBuffers, FBO unbinding, glReadPixels, or
    // CopyTexImage2D from the bound FBO.
    //
    // We *used* to create our own throwaway FBO (`src_framebuffer`),
    // attach the app's renderbuffer to it, and CopyTexImage2D from
    // there. That worked on lenient drivers (Mesa / llvmpipe / Apple
    // PowerVR / Adreno ES 3.x layer) but failed on ARM Mali r32p1
    // (Mali-G57 MC2 in OpenGL ES-CM 1.1 mode), which on this code path
    // produces an all-zero renderbuffer. The renderbuffer-content probe
    // we added in the previous PR confirms this on real hardware:
    // BL=BR=TL=TR=(0,0,0,255) at every first frame.
    //
    // Why did it fail on Mali? The act of `BindFramebufferOES(..,
    // src_framebuffer)` unbinds the FBO the app rendered into, and on
    // Mali r32p1 that unbind *discards* the tile data instead of
    // resolving it to the renderbuffer's main memory. By the time
    // CopyTexImage2D runs against `src_framebuffer`, the renderbuffer
    // is empty. Forcing a 1-pixel glReadPixels + glFinish *before* the
    // unbind also turned out not to be enough — the driver's resolve
    // heuristics on that path still produce zeros (verified by user
    // log on Mali-G57 MC2 with the previous attempt).
    //
    // The robust fix is to skip the FBO switch entirely: the app's own
    // FBO already has the renderbuffer attached at color attachment 0
    // (that's how the app rendered into it in the first place), so we
    // can CopyTexImage2D directly from the currently-bound FBO. That
    // way Mali never has a reason to discard the tile data: the same
    // FBO that the draws went into is the FBO we're now reading from.
    //
    // The standard iPhone EAGL pattern guarantees old_framebuffer != 0
    // at this point (the app must bind its own FBO before drawing into
    // a renderbuffer-attached attachment, since FBO 0 has no such
    // attachment). Out of paranoia we still keep a fallback for
    // old_framebuffer == 0 that creates a temporary FBO and attaches
    // the renderbuffer to it — this matches the pre-fix behaviour and
    // lets weird non-iOS-pattern apps still present *something*.
    let mut src_framebuffer: GLuint = 0;
    // Always copy from a temporary FBO with `renderbuffer` attached, instead of
    // trusting whatever framebuffer the guest happens to have bound.
    //
    // Dead Space 1.3.34 binds its own FBO (2) before calling
    // `presentRenderbuffer:`, and that FBO is *not* the one the drawable
    // renderbuffer is attached to. Copying from it produced an all-black
    // texture, so `present_frame` drew a black quad over the window and the
    // window stayed black even though the renderbuffer had the real frame.
    //
    // (The "use the app's FBO" fast path exists as a workaround for Mali
    // tile-cache resolve issues on Android; llvmpipe/Mesa needs the explicit
    // temp FBO instead.)
    let used_app_fbo = false;
    // Read depth through the shared ES2 context while the game's own FBO is
    // still current. The ES1.1 Adreno path rejects depth readback formats, but
    // the ES3.2 shared context can read the same FBO objects.
    #[cfg(target_os = "android")]
    let (fsr3_depth, fsr3_depth_ahb) = if std::env::var("TOUCHHLE_DS_FSR3_AHB").as_deref()
        == Ok("1")
        && std::env::var("TOUCHHLE_DS_FSR3_REAL_DEPTH").as_deref() == Ok("1")
    {
        let mut depth_rb: GLint = 0;
        gles.GetFramebufferAttachmentParameterivOES(
            gles11::FRAMEBUFFER_OES,
            gles11::DEPTH_ATTACHMENT_OES,
            0x8CD1, // GL_FRAMEBUFFER_ATTACHMENT_OBJECT_NAME_OES
            &mut depth_rb,
        );
        unsafe {
            if std::env::var("TOUCHHLE_DS_FSR3_GPU_DEPTH").as_deref() == Ok("1") {
                let depth_image = crate::gles::fsr_es2::depth_image_from_renderbuffer(
                    depth_rb as u32,
                    width as u32,
                    height as u32,
                );
                if !depth_image.is_null() {
                    (None, Some(depth_image))
                } else {
                    (
                        crate::gles::fsr_es2::capture_depth_from_renderbuffer(
                            depth_rb as u32,
                            width as u32,
                            height as u32,
                        ),
                        None,
                    )
                }
            } else {
                (
                    crate::gles::fsr_es2::capture_depth_from_renderbuffer(
                        depth_rb as u32,
                        width as u32,
                        height as u32,
                    ),
                    None,
                )
            }
        }
    } else {
        (None, None)
    };
    #[cfg(not(target_os = "android"))]
    let fsr3_depth: Option<Vec<f32>> = None;
    #[cfg(not(target_os = "android"))]
    let fsr3_depth_ahb: Option<*mut std::ffi::c_void> = None;

    let _ = old_framebuffer;
    let gpu_present_copy = crate::ds_perf_control::gpu_present_copy();
    {
        use std::sync::atomic::{AtomicU64, Ordering};
        static BRANCH_COUNT: AtomicU64 = AtomicU64::new(0);
        let branch_frame = BRANCH_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if branch_frame % 120 == 0 {
            log!(
                "[DSpresent] branch frame={} gpu_copy={} fsr3_ahb={:?} renderbuffer={} size={}x{}",
                branch_frame,
                gpu_present_copy,
                std::env::var("TOUCHHLE_DS_FSR3_AHB").ok(),
                renderbuffer,
                width,
                height
            );
        }
    }
    if !used_app_fbo {
        if gpu_present_copy {
            src_framebuffer = prepare_es1_source_framebuffer(gles, &mut transfer);
        } else {
            gles.GenFramebuffersOES(1, &mut src_framebuffer);
        }
        gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, src_framebuffer);
        gles.FramebufferRenderbufferOES(
            gles11::FRAMEBUFFER_OES,
            gles11::COLOR_ATTACHMENT0_OES,
            gles11::RENDERBUFFER_OES,
            renderbuffer,
        );
    }
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            if used_app_fbo {
                "after using app's bound FBO as copy source (no FBO switch)"
            } else {
                "after fallback FBO create+bind+attach (old_framebuffer==0)"
            },
        );
    }

    // Create a texture with a copy of the pixels in the framebuffer.
    //
    // PERF: the texture is cached across frames (see [TransferState::es1_texture]).
    // We used to GenTextures + CopyTexImage2D + DeleteTextures on *every*
    // presented frame; CopyTexImage2D redefines (reallocates) the texture
    // storage each time, which at scale-hack 4x means allocating and filling
    // a multi-megabyte RGBA surface per frame. Now the texture persists and
    // steady-state frames only run glCopyTexSubImage2D into preallocated
    // storage (no reallocation, no object churn). Parameters are set once at
    // creation; storage is only redefined when the renderbuffer size changes.
    // The present quad's shader always samples texture unit 0, but the guest may
    // have left a different unit active (its own ES2-style code paths use
    // glActiveTexture). Select unit 0 for our own texture work and restore the
    // guest's selection afterwards — otherwise the quad samples an empty unit
    // and the presented frame comes out black even though the texture contains
    // the right pixels (verified via the ReadPixels probe above).
    let old_active_texture: GLenum = get_int(gles, crate::gles::gles2_raw::ACTIVE_TEXTURE) as _;
    gles.ActiveTexture(crate::gles::gles2_raw::TEXTURE0);

    let (texture, storage_valid) = prepare_es1_present_texture(gles, &mut transfer, width, height);
    // These draws/copies/samples execute in the same GL context. The driver
    // orders their dependencies; a global CPU wait needlessly drains the GPU
    // queue before every copy. Preserve the CPU path and diagnostic rollback.
    let skip_present_finish = !crate::ds_perf_control::readback_finish_enabled();
    let force_gpu_finish = std::env::var("TOUCHHLE_DS_PRESENT_COPY_FINISH")
        .as_deref() == Ok("1");
    let present_finish = present_buffer::needs_finish(gpu_present_copy, skip_present_finish, force_gpu_finish);
    // On the CPU compatibility path, Finish + ReadPixels + Tex(Sub)Image2D
    // are the per-frame transfer cost. Keep this nested timer opt-in so normal
    // releases add no clock reads or GL work; the enclosing gpu_present timer
    // remains the end-to-end present cost.
    let _cpu_transfer_profile = if gpu_present_copy {
        None
    } else {
        Some(crate::ds_perf::Timer::new(crate::ds_perf::Stage::Readback))
    };
    if present_finish {
        gles.Finish();
    } else if gpu_present_copy {
        log_once!("[present] GPU copy remains queued; no CPU glFinish before copy");
    } else {
        log_once!("[present] skipping present-copy glFinish (diagnostic)");
    }

    gles.gpu_profile_end(4);
    if gpu_present_copy {
        gles.gpu_profile_poll();
        gles.gpu_profile_begin(2);
        if !storage_valid {
            gles.TexImage2D(
                gles11::TEXTURE_2D,
                0,
                gles11::RGBA as _,
                width,
                height,
                0,
                gles11::RGBA,
                gles11::UNSIGNED_BYTE,
                std::ptr::null(),
            );
            transfer.es1_texture = Some((texture, width, height));
        }
        gles.CopyTexSubImage2D(gles11::TEXTURE_2D, 0, 0, 0, 0, 0, width, height);
        gles.gpu_profile_end(2);
        crate::ds_perf::present_transfer_submission(true);
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static SEEN: AtomicBool = AtomicBool::new(false);
            if !SEEN.swap(true, Ordering::Relaxed) {
                log!(
                    "[DSpresent] GPU copy path active: {}x{} fbo={} texture={} finish={}",
                    width,
                    height,
                    src_framebuffer,
                    texture,
                    present_finish
                );
            }
        }
    } else {
        // Windows/Mesa compatibility path: CopyTexSubImage2D can produce an
        // all-black texture on llvmpipe, so retain the proven CPU readback
        // implementation there and as the Android fallback.
        let w = width as usize;
        let h = height as usize;
        PRESENT_ES1_READBACK.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            let pixels = scratch.prepare(width, height);
            gles.ReadPixels(
                0,
                0,
                width,
                height,
                gles11::RGBA,
                gles11::UNSIGNED_BYTE,
                pixels.as_mut_ptr() as *mut _,
            );
            if trace_gl_errors {
                use std::sync::atomic::{AtomicU32, Ordering};
                static N: AtomicU32 = AtomicU32::new(0);
                let n = N.fetch_add(1, Ordering::Relaxed);
                if n % 600 == 0 {
                    let mid = ((h / 2) * w + (w / 2)) * 4;
                    log!(
                        "[--trace-gl-errors] present upload src {}x{}: origin={:?} centre={:?}",
                        width,
                        height,
                        &pixels[0..4],
                        &pixels[mid..mid + 4]
                    );
                }
            }
            if storage_valid {
                gles.TexSubImage2D(
                    gles11::TEXTURE_2D, 0, 0, 0, width, height,
                    gles11::RGBA, gles11::UNSIGNED_BYTE, pixels.as_ptr().cast(),
                );
            } else {
                gles.TexImage2D(
                    gles11::TEXTURE_2D,
                    0,
                    gles11::RGBA as _,
                    width,
                    height,
                    0,
                    gles11::RGBA,
                    gles11::UNSIGNED_BYTE,
                    pixels.as_ptr() as *const _,
                );
                transfer.es1_texture = Some((texture, width, height));
            }
            crate::ds_perf::present_transfer_submission(false);
        });
        drop(_cpu_transfer_profile);
    }
    gles.gpu_profile_begin(5);
    // Re-assert the texture parameters on every present. If anything leaves
    // this NPOT texture with mipmapped filtering, the present quad is black.
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
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after Finish + CopyTexImage2D",
        );
    }
    // Diagnostic probe: read a few pixels of the renderbuffer the guest
    // just rendered into, so we can tell apart "renderbuffer is empty /
    // all-black" (a guest-side or attach-side / tile-resolve bug) from
    // "renderbuffer has content but present_frame is mis-displaying it"
    // (a present-side bug). At this point the read source FBO is
    // whichever copy source we used above: the app's own FBO
    // (used_app_fbo, normal iOS pattern) or the throwaway src_framebuffer
    // (fallback for old_framebuffer == 0). Either way, ReadPixels reads
    // from FRAMEBUFFER_BINDING, so the values logged here describe the
    // pixels CopyTexImage2D just copied.
    //
    // We sample several frames (the very first frame is often just a
    // glClear and shows zeros even on healthy drivers — we need to also
    // see what later "real game" frames look like) and we sample the
    // image centre as well as the corners (the corners on UI screens
    // are often legitimately black, while the centre is where the
    // actual artwork lives, so that's a much better "did anything
    // render?" signal). Gated on --trace-gl-errors.
    if trace_gl_errors {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static PROBE_COUNT: AtomicUsize = AtomicUsize::new(0);
        let count = PROBE_COUNT.fetch_add(1, Ordering::Relaxed);
        // Probe at frames 0, 1, 5, 30, 120, 600 — covers "first frame
        // before app drew anything", "second frame", a couple of early
        // splash frames, ~half-second-in, ~2s-in, ~10s-in. After that
        // every 600 frames so a long-running session still gives us
        // periodic snapshots without flooding the log.
        let should_log = matches!(count, 0 | 1 | 5 | 30 | 120 | 600) || count.is_multiple_of(600);
        if should_log {
            // 5 single-pixel reads: 4 corners + centre.
            let mut tl: [u8; 4] = [0; 4];
            let mut tr: [u8; 4] = [0; 4];
            let mut bl: [u8; 4] = [0; 4];
            let mut br: [u8; 4] = [0; 4];
            let mut cc: [u8; 4] = [0; 4];
            let xmax = width.saturating_sub(1).max(0);
            let ymax = height.saturating_sub(1).max(0);
            let xmid = width / 2;
            let ymid = height / 2;
            for (x, y, buf) in [
                (0, 0, &mut bl),
                (xmax, 0, &mut br),
                (0, ymax, &mut tl),
                (xmax, ymax, &mut tr),
                (xmid, ymid, &mut cc),
            ] {
                gles.ReadPixels(
                    x,
                    y,
                    1,
                    1,
                    gles11::RGBA,
                    gles11::UNSIGNED_BYTE,
                    buf.as_mut_ptr() as *mut _,
                );
            }
            // Drain any GL error this probe may have generated; it must
            // not leak into the guest-visible error queue.
            while gles.GetError() != 0 {}
            // Snapshot the guest's current GL state — this tells us how
            // the *app* was set up at presentRenderbuffer entry, which
            // is the moment the previous frame's draws were issued.
            // Useful for diagnosing "renderbuffer is uniform colour"
            // bugs that look like draws no-op'd: does the app even
            // have a vertex array enabled? Was a texture bound? Is
            // alpha-test or depth-test rejecting fragments? etc.
            //
            // Each query is wrapped with an error-drain so a strict
            // driver that rejects an enum just shows up as 0 in the
            // log without poisoning the queue for the next query.
            let qb = |gles: &mut dyn GLES, name: GLenum| -> GLboolean {
                while gles.GetError() != 0 {}
                let mut v: GLboolean = gles11::FALSE;
                gles.GetBooleanv(name, &mut v);
                while gles.GetError() != 0 {}
                v
            };
            let qi = |gles: &mut dyn GLES, name: GLenum| -> GLint {
                while gles.GetError() != 0 {}
                let mut v: GLint = 0;
                gles.GetIntegerv(name, &mut v);
                while gles.GetError() != 0 {}
                v
            };
            let qf4 = |gles: &mut dyn GLES, name: GLenum| -> [GLfloat; 4] {
                while gles.GetError() != 0 {}
                let mut v: [GLfloat; 4] = [0.0; 4];
                gles.GetFloatv(name, v.as_mut_ptr());
                while gles.GetError() != 0 {}
                v
            };
            let blend_on = qb(gles, gles11::BLEND);
            let blend_src = qi(gles, gles11::BLEND_SRC) as u32;
            let blend_dst = qi(gles, gles11::BLEND_DST) as u32;
            let depth_on = qb(gles, gles11::DEPTH_TEST);
            let alpha_on = qb(gles, gles11::ALPHA_TEST);
            let cull_on = qb(gles, gles11::CULL_FACE);
            let texture_2d_on = qb(gles, gles11::TEXTURE_2D);
            let varr_on = qb(gles, gles11::VERTEX_ARRAY);
            let carr_on = qb(gles, gles11::COLOR_ARRAY);
            let tarr_on = qb(gles, gles11::TEXTURE_COORD_ARRAY);
            let array_buffer = qi(gles, gles11::ARRAY_BUFFER_BINDING);
            let elem_buffer = qi(gles, gles11::ELEMENT_ARRAY_BUFFER_BINDING);
            let clear_color = qf4(gles, gles11::COLOR_CLEAR_VALUE);
            let current_color = qf4(gles, gles11::CURRENT_COLOR);
            log!(
                "[--trace-gl-errors] present_renderbuffer renderbuffer-content probe \
                 (frame={}, used_app_fbo={}, viewport={:?}, renderbuffer={}x{}): \
                 BL=({},{},{},{}) BR=({},{},{},{}) TL=({},{},{},{}) TR=({},{},{},{}) \
                 CENTER=({},{},{},{}) \
                 | guest GL state: app_fbo={} app_tex2d={} \
                 array_buffer={} element_buffer={} \
                 BLEND={} (src=0x{:04x} dst=0x{:04x}) DEPTH_TEST={} ALPHA_TEST={} \
                 CULL_FACE={} TEXTURE_2D={} \
                 vertex_arr={} color_arr={} texcoord_arr={} \
                 clear_color=({:.3},{:.3},{:.3},{:.3}) current_color=({:.3},{:.3},{:.3},{:.3})",
                count,
                used_app_fbo,
                viewport,
                width,
                height,
                bl[0],
                bl[1],
                bl[2],
                bl[3],
                br[0],
                br[1],
                br[2],
                br[3],
                tl[0],
                tl[1],
                tl[2],
                tl[3],
                tr[0],
                tr[1],
                tr[2],
                tr[3],
                cc[0],
                cc[1],
                cc[2],
                cc[3],
                old_framebuffer,
                old_texture_2d,
                array_buffer,
                elem_buffer,
                blend_on,
                blend_src,
                blend_dst,
                depth_on,
                alpha_on,
                cull_on,
                texture_2d_on,
                varr_on,
                carr_on,
                tarr_on,
                clear_color[0],
                clear_color[1],
                clear_color[2],
                clear_color[3],
                current_color[0],
                current_color[1],
                current_color[2],
                current_color[3],
            );
        }
    }
    // Texture filter/wrap parameters were set once at creation (see the
    // comment in the cold path above); re-issuing them every frame is
    // unnecessary driver work.
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after TexParameteri (filter+wrap)",
        );
    }

    // Stop using the source FBO so the present_frame quad below renders
    // to the default framebuffer (the SDL window) instead of the
    // renderbuffer / our throwaway FBO. In the no-FBO-switch path we
    // simply unbind the app's FBO; we'll restore it again at the end of
    // this function. In the fallback path, deleting the throwaway FBO
    // also implicitly unbinds it, leaving FRAMEBUFFER_BINDING == 0.
    if used_app_fbo || gpu_present_copy {
        gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, 0);
    } else {
        gles.DeleteFramebuffersOES(1, &src_framebuffer);
    }
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            if used_app_fbo || gpu_present_copy {
                "after BindFramebufferOES(0) (release source FBO for present)"
            } else {
                "after DeleteFramebuffersOES (fallback path)"
            },
        );
    }

    // Reset various things that could affect the quad or virtual cursor we're
    // going to draw. Back up the old state while doing so, so it can be
    // restored later. The app's subsequent drawing will be messed up if we
    // don't restore it.

    // We *used* to query GL_CURRENT_PROGRAM (0x8B8D) here and call
    // glUseProgram(0) to clear any program before drawing the fixed-function
    // present quad. The original assumption was: "ES 1.x contexts won't have
    // any program bound, so this is a harmless no-op on ES 1.1 backends; ES
    // 2.0 apps that nevertheless landed in this path needed the clear so the
    // fixed-function quad would work."
    //
    // That assumption is wrong on real-world Android ES 1.1 drivers. On
    // Adreno (and similar GPUs whose ES 1.1 surface is implemented on top of
    // an underlying ES 3.x engine), querying GL_CURRENT_PROGRAM returns a
    // non-zero handle pointing at the driver's own internal program (used
    // for fixed-function emulation). Calling our generic gles.UseProgram(0)
    // on a GLES1Native backend then no-ops (gles_generic stub) — the program
    // is *not* actually unbound, but our fixed-function quad below now runs
    // with that internal program intercepting the draws, which produces a
    // black screen.
    //
    // The reverse path (a GLES2 app that somehow landed here) is now
    // impossible: the `if gles.is_es2()` branch above returns early for any
    // ES 2.0 backend. So just drop the clear/restore entirely on the
    // remaining (non-ES2) path. See LEGO Ninjago: Spinjitzu Scavenger Hunt.

    let old_arrays = {
        let mut old_arrays = [gles11::FALSE; gles1_on_gl2::ARRAYS.len()];
        for (is_enabled, info) in old_arrays.iter_mut().zip(gles1_on_gl2::ARRAYS.iter()) {
            gles.GetBooleanv(info.name, is_enabled);
            gles.DisableClientState(info.name);
        }
        old_arrays
    };
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after old_arrays save+disable",
        );
    }
    // On a strict native ES 1.1 driver (e.g. ARM Mali on Android), some
    // entries in CAPABILITIES are spec-invalid for ES 1.1 even though they
    // are valid in desktop GL 2.1 (where the gles1_on_gl2 emulator runs).
    // Querying / disabling those would yield GL_INVALID_ENUM. The hard-coded
    // CAPABILITIES_GL21_ONLY list catches the *known* offenders, but real
    // Android drivers have plenty of further per-vendor / per-extension
    // quirks (e.g. r32p1 Mali-G57 in OpenGL ES-CM 1.1 mode rejects more caps
    // than the spec strictly says it should). Rather than maintain a growing
    // allow-list per driver, we drain GL errors *per cap* here: any cap that
    // GetBooleanv rejects is recorded as "skip" (None) so the matching
    // restore loop below also leaves it alone instead of trying to Enable /
    // Disable it and producing yet another error. This keeps the GL error
    // queue clean for the subsequent present_check sections — without it,
    // the very first cap that errors would poison the queue and make every
    // later checkpoint look like *it* failed.
    let is_native_es1 = gles.is_native_es1();
    let old_capabilities: [Option<GLboolean>; gles1_on_gl2::CAPABILITIES.len()] = {
        let mut old_capabilities: [Option<GLboolean>; gles1_on_gl2::CAPABILITIES.len()] =
            [None; gles1_on_gl2::CAPABILITIES.len()];
        // Collect rejected caps so we can log them as a single line the
        // first time present runs. Useful for diagnosing "title menu
        // black on Mali but logo works" style bugs: maybe Mali rejected
        // the very cap the title menu relies on (e.g. ALPHA_TEST,
        // POINT_SPRITE_OES, ...).
        static FIRST_PRESENT: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(true);
        let log_rejects =
            trace_gl_errors && FIRST_PRESENT.swap(false, std::sync::atomic::Ordering::Relaxed);
        let mut rejected_caps: Vec<GLenum> = Vec::new();
        for (slot, &name) in old_capabilities
            .iter_mut()
            .zip(gles1_on_gl2::CAPABILITIES.iter())
        {
            if is_native_es1 && gles1_on_gl2::CAPABILITIES_GL21_ONLY.contains(&name) {
                continue;
            }
            // Drain anything that leaked in from earlier so we can attribute
            // a fresh error to *this* cap.
            while gles.GetError() != 0 {}
            let mut value: GLboolean = gles11::FALSE;
            gles.GetBooleanv(name, &mut value);
            let mut probe_failed = false;
            while gles.GetError() != 0 {
                probe_failed = true;
            }
            if probe_failed {
                // Driver doesn't accept this cap. Leave slot as None so we
                // also skip it on restore, and don't try to Disable it.
                if log_rejects {
                    rejected_caps.push(name);
                }
                continue;
            }
            *slot = Some(value);
            gles.Disable(name);
            // Disable on a valid cap shouldn't error, but a few drivers are
            // looser on Get than on Enable/Disable; drain to be safe.
            while gles.GetError() != 0 {}
        }
        if log_rejects && !rejected_caps.is_empty() {
            let names: Vec<String> = rejected_caps
                .iter()
                .map(|c| format!("0x{:04x}", c))
                .collect();
            log!(
                "[--trace-gl-errors] present_renderbuffer driver rejected {} ES1.1 caps \
                 (will be skipped on save/restore): [{}]. \
                 Hard-coded CAPABILITIES_GL21_ONLY allow-list missed these — they \
                 may also be rejected when the *guest* tries to use them, which \
                 could explain why some screens render black.",
                rejected_caps.len(),
                names.join(", "),
            );
        }
        old_capabilities
    };
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after old_capabilities save+disable",
        );
    }
    let old_matrix_mode: GLenum = get_int(gles, gles11::MATRIX_MODE) as _;
    for mode in [gles11::MODELVIEW, gles11::PROJECTION, gles11::TEXTURE] {
        gles.MatrixMode(mode);
        gles.PushMatrix();
        gles.LoadIdentity();
    }
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after matrix push+identity for all 3 stacks",
        );
    }
    let old_color: [GLfloat; 4] = get_floats(gles, gles11::CURRENT_COLOR);
    gles.Color4f(1.0, 1.0, 1.0, 1.0);
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after old_color save + Color4f white",
        );
    }

    // Back up other things that will be modified while drawing.
    let old_viewport: (GLint, GLint, GLsizei, GLsizei) = {
        let [x, y, width, height] = get_ints(gles, gles11::VIEWPORT);
        (x, y, width as _, height as _)
    };
    let old_clear_color: [GLfloat; 4] = get_floats(gles, gles11::COLOR_CLEAR_VALUE);
    let old_array_buffer: GLuint = get_int(gles, gles11::ARRAY_BUFFER_BINDING) as _;
    let old_vertex_array_binding: GLuint = get_int(gles, gles11::VERTEX_ARRAY_BUFFER_BINDING) as _;
    let old_vertex_array_size: GLint = get_int(gles, gles11::VERTEX_ARRAY_SIZE);
    let old_vertex_array_type: GLenum = get_int(gles, gles11::VERTEX_ARRAY_TYPE) as _;
    let old_vertex_array_stride: GLsizei = get_int(gles, gles11::VERTEX_ARRAY_STRIDE) as _;
    let old_vertex_array_pointer = get_ptr(gles, gles11::VERTEX_ARRAY_POINTER);
    let old_tex_coord_array_binding: GLuint =
        get_int(gles, gles11::TEXTURE_COORD_ARRAY_BUFFER_BINDING) as _;
    let old_tex_coord_array_size: GLint = get_int(gles, gles11::TEXTURE_COORD_ARRAY_SIZE);
    let old_tex_coord_array_type: GLenum = get_int(gles, gles11::TEXTURE_COORD_ARRAY_TYPE) as _;
    let old_tex_coord_array_stride: GLsizei =
        get_int(gles, gles11::TEXTURE_COORD_ARRAY_STRIDE) as _;
    let old_tex_coord_array_pointer = get_ptr(gles, gles11::TEXTURE_COORD_ARRAY_POINTER);
    let old_blend_sfactor: GLenum = get_int(gles, gles11::BLEND_SRC) as _;
    let old_blend_dfactor: GLenum = get_int(gles, gles11::BLEND_DST) as _;
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after viewport/clear/blend/array-pointer state save",
        );
    }

    let old_tex_env_mode = get_tex_env_int(gles, gles11::TEXTURE_ENV, gles11::TEXTURE_ENV_MODE);
    // if the mode is REPLACE, we don't have to reset the other texture
    // environment values
    let tex_env_mode_arr = [gles11::REPLACE; 1];
    gles.TexEnviv(
        gles11::TEXTURE_ENV,
        gles11::TEXTURE_ENV_MODE,
        tex_env_mode_arr.as_ptr().cast(),
    );
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(gles, trace_gl_errors, &SEEN, "after TexEnviv setup");
    }

    // Draw the quad. `(width, height)` is the renderbuffer we just uploaded, so
    // the optional present-layer sharpen gets the right texel size even though
    // this path presents through a window that may be a different size.
    #[cfg(target_os = "android")]
    let android_shader_present_done = if std::env::var("TOUCHHLE_DS_FSR3_AHB").as_deref() == Ok("1")
    {
        unsafe {
            let fsr3_output = crate::gles::fsr_es2::try_ahb_copy(
                texture,
                width as u32,
                height as u32,
                fsr3_depth.as_deref(),
                fsr3_depth_ahb.unwrap_or(std::ptr::null_mut()),
                viewport.2,
                viewport.3,
                viewport,
                rotation_matrix,
            );
            if let Some((_output_texture, output_width, output_height)) = fsr3_output {
                if crate::gles::ds_ahb_bridge::fsr3_copy_fg() {
                    // Preserve the explicit synthetic FG diagnostic path. The
                    // real FSR output was already presented inside try_ahb_copy.
                    (*window_ptr).swap_window();
                    let fg_texture = crate::gles::ds_ahb_bridge::output_texture();
                    let _fg_presented = crate::gles::fsr_es2::try_present(
                        fg_texture,
                        (output_width, output_height),
                        viewport,
                        rotation_matrix,
                    );
                }
                true
            } else {
                false
            }
        }
    } else if is_native_es1 {
        unsafe {
            crate::gles::fsr_es2::try_present(
                texture,
                (width as u32, height as u32),
                viewport,
                rotation_matrix,
            )
        }
    } else {
        false
    };
    #[cfg(not(target_os = "android"))]
    let android_shader_present_done = false;

    if !android_shader_present_done {
        gles.gpu_profile_end(5);
        present_frame(
            gles,
            &mut present_state.borrow_mut(),
            viewport,
            rotation_matrix,
            virtual_cursor_visible_at,
            Some((width as u32, height as u32)),
            Some(&mut gpu_text_overlay.borrow_mut()),
        );
    }
    gles.gpu_profile_begin(6);
    // DIAGNOSTIC: read the window back-buffer's centre pixel to check whether
    // the present quad actually produced visible content.
    if trace_gl_errors {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        if n % 600 == 599 {
            let mut px = [0u8; 4];
            unsafe {
                gles.ReadPixels(
                    (viewport.0 + viewport.2 / 2) as i32,
                    (viewport.1 + viewport.3 / 2) as i32,
                    1,
                    1,
                    gles11::RGBA,
                    gles11::UNSIGNED_BYTE,
                    px.as_mut_ptr() as *mut _,
                );
            }
            log!(
                "[--trace-gl-errors] window back-buffer centre pixel after present_frame = {:?}",
                px
            );
        }
    }
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after present_frame (textured quad draw)",
        );
    }

    // PERF: the present texture is cached across frames
    // (TransferState::es1_texture); do not delete it here. The restore below binds
    // the app's previous texture, which also unbinds ours.

    // Restore all the state saved before rendering
    for (&is_enabled, info) in old_arrays.iter().zip(gles1_on_gl2::ARRAYS.iter()) {
        match is_enabled {
            gles11::TRUE => gles.EnableClientState(info.name),
            gles11::FALSE => gles.DisableClientState(info.name),
            _ => unreachable!(),
        }
    }
    for (&saved, &name) in old_capabilities
        .iter()
        .zip(gles1_on_gl2::CAPABILITIES.iter())
    {
        if is_native_es1 && gles1_on_gl2::CAPABILITIES_GL21_ONLY.contains(&name) {
            continue;
        }
        // None means the save loop above couldn't query this cap (the
        // driver rejected it with INVALID_ENUM). Don't try to Enable /
        // Disable it here either — that would just produce another error.
        let Some(is_enabled) = saved else {
            continue;
        };
        match is_enabled {
            gles11::TRUE => gles.Enable(name),
            gles11::FALSE => gles.Disable(name),
            _ => unreachable!(),
        }
    }
    for mode in [gles11::MODELVIEW, gles11::PROJECTION, gles11::TEXTURE] {
        gles.MatrixMode(mode);
        gles.PopMatrix();
    }
    gles.MatrixMode(old_matrix_mode);
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(gles, trace_gl_errors, &SEEN, "after matrix pop+restore");
    }
    gles.Color4f(old_color[0], old_color[1], old_color[2], old_color[3]);
    gles.Viewport(
        old_viewport.0,
        old_viewport.1,
        old_viewport.2,
        old_viewport.3,
    );
    gles.ClearColor(
        old_clear_color[0],
        old_clear_color[1],
        old_clear_color[2],
        old_clear_color[3],
    );
    // GL_ARRAY_BUFFER is implicitly used by the Pointer functions but is also
    // an independent binding.
    gles.BindBuffer(gles11::ARRAY_BUFFER, old_vertex_array_binding);
    gles.VertexPointer(
        old_vertex_array_size,
        old_vertex_array_type,
        old_vertex_array_stride,
        old_vertex_array_pointer,
    );
    gles.BindBuffer(gles11::ARRAY_BUFFER, old_tex_coord_array_binding);
    gles.TexCoordPointer(
        old_tex_coord_array_size,
        old_tex_coord_array_type,
        old_tex_coord_array_stride,
        old_tex_coord_array_pointer,
    );
    gles.BindBuffer(gles11::ARRAY_BUFFER, old_array_buffer);
    gles.BlendFunc(old_blend_sfactor, old_blend_dfactor);
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after vertex/texcoord/buffer/blend restore",
        );
    }

    let old_tex_env_mode_arr = [old_tex_env_mode; 1];
    gles.TexEnviv(
        gles11::TEXTURE_ENV,
        gles11::TEXTURE_ENV_MODE,
        old_tex_env_mode_arr.as_ptr().cast(),
    );
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(gles, trace_gl_errors, &SEEN, "after TexEnviv restore");
    }

    gles.ActiveTexture(old_active_texture);
    gles.gpu_profile_end(6);
    gles.gpu_profile_begin(7);
    std::mem::drop(gles_boxed);

    // SDL2's documentation warns 0 should be bound to the draw framebuffer
    // when swapping the window, so this is the perfect moment.
    env.window.as_ref().unwrap().swap_window();

    let mut gles_boxed = gles_ctx.make_current(env.window.as_mut().unwrap());
    let gles = gles_boxed.as_mut();
    gles.gpu_profile_end(7);
    gles.gpu_profile_begin(8);
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after swap_window + re-make-current",
        );
    }

    // Restore the other bindings
    gles.BindTexture(gles11::TEXTURE_2D, old_texture_2d);
    gles.BindFramebufferOES(gles11::FRAMEBUFFER_OES, old_framebuffer);
    {
        static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        present_check(
            gles,
            trace_gl_errors,
            &SEEN,
            "after BindTexture + BindFramebufferOES restore",
        );
    }

    // (See the long comment above for why we no longer save/restore
    // GL_CURRENT_PROGRAM on this ES 1.1 present path.)

    // Drain any GL errors generated by our own host-side present logic so
    // they don't leak into the guest's GL error queue and get attributed
    // to whatever guest call happens next (which previously confused the
    // --trace-gl-errors output and could perturb apps that poll
    // glGetError themselves). On strict ES 1.1 drivers (Mali, Adreno
    // ES1.1 surface) some of the wide state save/restore queries above
    // can return GL_INVALID_ENUM for state variables the driver doesn't
    // recognise; that's a host-side issue with our save list, not
    // anything the guest did. Log the first error once per app run for
    // diagnostics, then silently drain the rest.
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        static REPORTED_ERR: AtomicBool = AtomicBool::new(false);
        let first = gles.GetError();
        if first != 0 && !REPORTED_ERR.swap(true, Ordering::Relaxed) {
            log!(
                "Note: present_renderbuffer left GL error {:#x} in queue; \
                 draining. Further errors from the present path will be \
                 silently consumed [this log will only be shown once]",
                first
            );
        }
        // Drain any further pending errors (GL keeps them queued one at
        // a time per error code; spec says implementations may keep an
        // unbounded number).
        while gles.GetError() != 0 {}
    }
    gles.gpu_profile_end(8);
    std::mem::drop(gles_boxed);
    crate::frameworks::store_kit::maybe_unlock_mtx(env);
}

pub fn EAGLGetVersion(env: &mut Environment, major: MutPtr<u32>, minor: MutPtr<u32>) {
    let version_major: u32 = 1;
    let version_minor: u32 = 1;

    if !major.is_null() {
        env.mem.write(major, version_major);
    }
    if !minor.is_null() {
        env.mem.write(minor, version_minor);
    }

    log!(
        "EAGLGetVersion called: major={}, minor={}",
        version_major,
        version_minor
    );
}

pub const FUNCTIONS: FunctionExports = &[export_c_func!(EAGLGetVersion(_, _))];

#[cfg(test)]
#[path = "eagl_transfer_context_tests.rs"]
mod transfer_context_tests;

#[cfg(all(test,target_os="windows"))]
#[path="eagl_r4_tests.rs"]
mod r4_tests;

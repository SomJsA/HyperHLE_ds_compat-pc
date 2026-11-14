/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! OpenGL ES and EAGL.
//!
//! This module is specific to OpenGL ES's role as a part of the iPhone OS API
//! surface. See [crate::gles] for other uses and a discussion of the broader
//! topic.

mod eagl;
pub(crate) use eagl::shader_opening_fog_range;
mod gles_guest;
pub(crate) use gles_guest::ds_test_assist;
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) use gles_guest::ds_current_hud_player_public;

use touchHLE_gl_bindings::gles11::types::GLenum;

use crate::mem::ConstPtr;

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/System/Library/Frameworks/OpenGLES.framework/OpenGLES",
    aliases: &[],
    class_exports: &[eagl::CLASSES],
    constant_exports: &[eagl::CONSTANTS],
    function_exports: &[gles_guest::FUNCTIONS, eagl::FUNCTIONS],
};

#[derive(Default)]
pub struct State {
    pub(crate) ds_chapter: crate::ds_chapter_overlay::State,
    pub(crate) ds_frontend_trace: crate::ds_frontend_trace::State,
    pub(crate) ds_font_roles: crate::ds_font_roles::State,
    pub(crate) ds_game_clock: crate::ds_game_clock::State,
    pub(crate) ds_shader_opening_fog: crate::ds_shader_opening_fog::State,
    pub(crate) ds_shader_projection: crate::ds_shader_projection::State,
    pub(crate) ds_guest_skin_probe: crate::ds_guest_skin_probe::State,
    pub(crate) ds_test_assist: gles_guest::ds_test_assist::State,
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) ds_pc_skin_shadow: crate::ds_pc_skin_shadow::State,
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) ds_pc_transform: crate::ds_pc_transform::State,
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) ds_pc_inverse_cache: crate::ds_pc_inverse_cache::State,
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) ds_pc_animation_position_cache:
        crate::ds_pc_animation_position_cache::State,
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) ds_pc_keyframe_cache: crate::ds_pc_keyframe_cache::State,
    pub(crate) ds_qte_geometry: crate::ds_qte_geometry::State,
    pub(crate) ds_subtitle: crate::ds_subtitle_overlay::State,
    pub(crate) ds_objective: crate::ds_objective_overlay::State,
    pub(crate) ds_tutorial: crate::ds_tutorial_overlay::State,
    /// Current EAGLContext for each thread. Thread IDs are indices into the
    /// emulator's contiguous `Environment::threads` vector, so a direct
    /// table avoids hashing on every guest GL call.
    current_ctxs: Vec<Option<crate::objc::id>>,
    /// `glGetString()` results cache, keyed by `(is_es2, name)` so that an
    /// ES 2.0 context does not return the ES 1.1 `OpenGL ES-CM 1.1` version
    /// string (Bad Piggies / Unity 3.5 checks the version string to decide
    /// which renderer code path to use — mismatching the API leaves Unity in
    /// a half-initialised ES 2.0 path that emits torn / overlapping frames).
    strings_cache: std::collections::HashMap<(bool, GLenum), ConstPtr<u8>>,
    ds_aim_hold_requested: bool,
    ds_aim_hold_player: u32,
    ds_aim_timeout_lease: Option<(u32, u32)>,
    /// A physical LT release can arrive while a native control/QTE has
    /// temporarily hidden the player object. Keep the release alive until
    /// the guest exposes the player again; otherwise the old aim bit leaks
    /// through the control state and the weapon returns up after release.
    ds_aim_release_pending: bool,
    ds_pc_move_player: u32,
    ds_pc_move_active: bool,
    ds_pc_forward_limited: bool,
    /// Optional per-present-frame cache for the native HUD/player lookup.
    /// The guest callback remains the source of truth whenever the cache is
    /// disabled or a new frame begins.
    ds_hud_player_cache_frame: u64,
    ds_hud_player_cache_thread: crate::ThreadId,
    ds_hud_player_cache_sampled: bool,
    ds_hud_player_cache_value: Option<(u32, u32)>,
    /// Check the full native GetHud entry once before any fixed-layout call.
    ds_native_hud_layout_verified: Option<bool>,
    ds_pc_look: crate::window::ds_pc_input::NativeLook,
    ds_player_action_cache: crate::ds_player_action_cache::Cache,
}
/// Restore the temporary native idle timeout before guest shutdown/prefs sync.
pub(crate) fn release_pc_aim_timeout(env: &mut crate::Environment) {
    gles_guest::ds_release_aim_timeout(env);
}

impl State {
    fn current_ctx_for_thread(&mut self, thread: crate::ThreadId) -> &mut Option<crate::objc::id> {
        if thread >= self.current_ctxs.len() {
            self.current_ctxs.resize(thread + 1, None);
        }
        &mut self.current_ctxs[thread]
    }

    /// The EAGL object whose actual GL context backs this thread's calls.
    fn effective_ctx_id(&self, thread: crate::ThreadId) -> Option<crate::objc::id> {
        self.current_ctxs
            .get(thread)
            .copied()
            .flatten()
            .or_else(|| {
                self.current_ctxs
                    .first()
                    .copied()
                    .flatten()
                    .filter(|&context| context != crate::objc::nil)
            })
    }

    /// True when GL calls on `thread` have no usable context *and* the
    /// main-thread fallback is also empty.
    ///
    /// Some apps (e.g. Dead Space 1.3.34) do all their rendering and
    /// `presentRenderbuffer:` on a worker thread but only ever call
    /// `[EAGLContext setCurrentContext:]` on the main thread. Strictly
    /// per-thread lookup would silently drop every one of those GL calls,
    /// producing a black window, so we fall back to the main thread's
    /// context.
    fn effective_ctx_missing(&self, thread: crate::ThreadId) -> bool {
        self.current_ctxs
            .get(thread)
            .copied()
            .flatten()
            .is_none()
            && self.current_ctxs.first().copied().flatten().is_none()
    }
}

/// Bind the calling thread's current EAGL context to the host window and
/// return a [`crate::gles::GLES`] wrapper for it.
///
/// Returns `None` when the calling thread has no current EAGL context (i.e.
/// `[EAGLContext setCurrentContext:]` was never called with a non-`nil`
/// argument on this thread, or it was reset to `nil`). Apple's
/// implementation simply ignores GL calls in that state instead of crashing
/// the process — see Apple's "OpenGL ES Programming Guide for iOS",
/// section "Context Lifecycle Management": *"If the current context is
/// `nil`, OpenGL ES commands silently fail"*.
///
/// Previously this used `.unwrap()` on the thread-local context, which
/// turned a perfectly recoverable guest-side state error into an emulator
/// panic. HyperHLE log #5 reproduced this when a worker thread issued GL
/// calls between `[EAGLContext setCurrentContext:nil]` and a later
/// `setCurrentContext:` on the main thread.
fn sync_context<'objc, 'win: 'objc>(
    state: &mut State,
    objc: &'objc mut crate::objc::ObjC,
    window: &'win mut crate::window::Window,
    current_thread: crate::ThreadId,
) -> Option<crate::gles::GLESHandle<'objc>> {
    let Some(current_ctx) = state.effective_ctx_id(current_thread) else {
        log!("[gl-debug] thread {} has no current EAGL ctx; bindings={:?}", current_thread, state.current_ctxs);
        return None;
    };
    sync_context_with_id(state, objc, window, current_thread, current_ctx)
}

/// Bind a context whose ID has already been resolved. The normal wrapper above
/// keeps the existing call sites unchanged; the guest GL hot path uses this
/// form to avoid resolving the same ThreadId twice in one call.
fn sync_context_with_id<'objc, 'win: 'objc>(
    state: &mut State,
    objc: &'objc mut crate::objc::ObjC,
    window: &'win mut crate::window::Window,
    current_thread: crate::ThreadId,
    current_ctx: crate::objc::id,
) -> Option<crate::gles::GLESHandle<'objc>> {
    let gles_ctx = get_thread_context_with_id(state, objc, current_thread, current_ctx)?;
    Some(gles_ctx.make_current_handle(window))
}

/// Look up the current [`crate::gles::GLESContext`] for the given thread.
///
/// Returns `None` when the thread has no current EAGL context bound, or
/// when that context was created without a backing GLES driver (e.g.
/// headless / unsupported `EAGLRenderingAPI`). Both are recoverable —
/// callers should skip the GL operation rather than abort the emulator.
fn get_thread_context<'objc>(
    state: &mut State,
    objc: &'objc mut crate::objc::ObjC,
    current_thread: crate::ThreadId,
) -> Option<&'objc mut dyn crate::gles::GLESContext> {
    // Note: `Box<dyn GLESContext>` carries the default `'static` trait-object
    // bound, so the returned mutable reference re-borrows from the
    // (`'objc`) ObjC store without imposing a tighter lifetime.
    let Some(current_ctx) = state.effective_ctx_id(current_thread) else {
        log!("[gl-debug] thread {} has no current EAGL ctx; bindings={:?}", current_thread, state.current_ctxs);
        return None;
    };
    get_thread_context_with_id(state, objc, current_thread, current_ctx)
}

fn get_thread_context_with_id<'objc>(
    state: &mut State,
    objc: &'objc mut crate::objc::ObjC,
    current_thread: crate::ThreadId,
    current_ctx: crate::objc::id,
) -> Option<&'objc mut dyn crate::gles::GLESContext> {
    let host_obj = objc.borrow_mut::<eagl::EAGLContextHostObject>(current_ctx);
    let gles_ctx: Option<&mut (dyn crate::gles::GLESContext + 'static)> =
        host_obj.gles_ctx.as_deref_mut();
    match gles_ctx {
        Some(g) => Some(g as &mut dyn crate::gles::GLESContext),
        None => {
            log!(
                "[gl-debug] thread {} ctx {:?} has no gles_ctx; bindings={:?}",
                current_thread,
                current_ctx,
                state.current_ctxs
            );
            None
        }
    }
}


#[cfg(test)]
mod gles_handle_context_tests {
    use super::*;
    #[test]
    fn gles_handle_context_map_preserves_main_fallback_and_explicit_binding() {
        let mut state=State::default();
        assert!(state.effective_ctx_missing(3));
        let main=crate::objc::id::from_bits(0x1000);
        let worker=crate::objc::id::from_bits(0x2000);
        *state.current_ctx_for_thread(0)=Some(main);
        assert!(!state.effective_ctx_missing(3));
        *state.current_ctx_for_thread(3)=Some(worker);
        assert_eq!(*state.current_ctx_for_thread(3),Some(worker));
        assert_eq!(*state.current_ctx_for_thread(0),Some(main));
        *state.current_ctx_for_thread(3)=None;
        assert!(!state.effective_ctx_missing(3));
        *state.current_ctx_for_thread(0)=None;
        assert!(state.effective_ctx_missing(3));
    }
}

#[cfg(test)]
mod effective_context_tests {
    use super::*;
    #[test]
    fn pc_eagl_effective_context_preserves_worker_and_main_fallback() {
        let mut state=State::default();
        let main=crate::objc::id::from_bits(0x1000);
        let worker=crate::objc::id::from_bits(0x2000);
        assert_eq!(state.effective_ctx_id(3),None);
        *state.current_ctx_for_thread(0)=Some(main);
        assert_eq!(state.effective_ctx_id(3),Some(main));
        *state.current_ctx_for_thread(3)=Some(worker);
        assert_eq!(state.effective_ctx_id(3),Some(worker));
        *state.current_ctx_for_thread(3)=None;
        assert_eq!(state.effective_ctx_id(3),Some(main));
        assert_eq!(*state.current_ctx_for_thread(0),Some(main));
    }
}

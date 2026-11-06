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
mod gles_guest;
pub(crate) use gles_guest::ds_test_assist;

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
    pub(crate) ds_guest_skin_probe: crate::ds_guest_skin_probe::State,
    pub(crate) ds_test_assist: gles_guest::ds_test_assist::State,
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) ds_pc_skin_shadow: crate::ds_pc_skin_shadow::State,
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) ds_pc_transform: crate::ds_pc_transform::State,
    pub(crate) ds_qte_geometry: crate::ds_qte_geometry::State,
    pub(crate) ds_subtitle: crate::ds_subtitle_overlay::State,
    pub(crate) ds_objective: crate::ds_objective_overlay::State,
    pub(crate) ds_tutorial: crate::ds_tutorial_overlay::State,
    /// Current EAGLContext for each thread
    current_ctxs: std::collections::HashMap<crate::ThreadId, Option<crate::objc::id>>,
    /// `glGetString()` results cache, keyed by `(is_es2, name)` so that an
    /// ES 2.0 context does not return the ES 1.1 `OpenGL ES-CM 1.1` version
    /// string (Bad Piggies / Unity 3.5 checks the version string to decide
    /// which renderer code path to use — mismatching the API leaves Unity in
    /// a half-initialised ES 2.0 path that emits torn / overlapping frames).
    strings_cache: std::collections::HashMap<(bool, GLenum), ConstPtr<u8>>,
    ds_aim_hold_requested: bool,
    ds_aim_hold_player: u32,
    ds_aim_timeout_lease: Option<(u32, u32)>,
    ds_pc_move_player: u32,
    ds_pc_move_active: bool,
    ds_pc_forward_limited: bool,
    ds_pc_look: crate::window::ds_pc_input::NativeLook,
    ds_player_action_cache: crate::ds_player_action_cache::Cache,
}
/// Restore the temporary native idle timeout before guest shutdown/prefs sync.
pub(crate) fn release_pc_aim_timeout(env: &mut crate::Environment) {
    gles_guest::ds_release_aim_timeout(env);
}

impl State {
    fn current_ctx_for_thread(&mut self, thread: crate::ThreadId) -> &mut Option<crate::objc::id> {
        self.current_ctxs.entry(thread).or_insert(None)
    }

    /// The EAGL object whose actual GL context backs this thread's calls.
    fn effective_ctx_id(&mut self, thread: crate::ThreadId) -> Option<crate::objc::id> {
        (*self.current_ctx_for_thread(thread)).or_else(||
            (*self.current_ctx_for_thread(0)).filter(|&context| context != crate::objc::nil))
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
    fn effective_ctx_missing(&mut self, thread: crate::ThreadId) -> bool {
        if (*self.current_ctx_for_thread(thread)).is_some() {
            return false;
        }
        (*self.current_ctx_for_thread(0)).is_none()
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
    let gles_ctx = get_thread_context(state, objc, current_thread)?;
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

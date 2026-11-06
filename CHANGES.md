# CHANGES

All changes in this fork relative to its base (HyperHLE `2281c14`, itself a fork
of touchHLE) are Dead Space 1.3.34 compatibility work. They were developed by
running the unmodified retail IPA in the emulator and fixing each failure in turn.

## 1. Build

- `Cargo.toml`: use the vendored `vendor/rust-sdl2` via `[patch]` instead of the
  git dependency, so the build works without network access to
  github.com/touchHLE/rust-sdl2.
- `src/audio/openal_soft_wrapper/build.rs` and `vendor/rust-sdl2/sdl2-sys/build.rs`:
  define `CMAKE_POLICY_VERSION_MINIMUM=3.5` so CMake >= 4 will still configure
  the vendored OpenAL Soft / SDL2 builds.

## 2. Run-time compatibility (the interesting part)

### 2.1 `__dynamic_cast` actually works (root cause of the black screen)

`src/libc/cxxabi.rs` used to stub the Itanium ABI `__dynamic_cast` to always
return NULL ("RTTI vtables are stubbed"). Dead Space bundles its own
`libstdc++.6.dylib`, so its `dynamic_cast` calls resolved to the *guest*
implementation, which relies on RTTI vtables that touchHLE stubs to `bx lr`
(return 0) — every `dynamic_cast` in the app therefore failed.

Chain that produced the original black screen:

```
bl ___dynamic_cast              ; stub returned NULL
ldr ip,[r0]                     ; dereference NULL -> UndefinedInstruction
... r5 = NULL ...
bl m3g::OpenGLES11Renderer::OpenGLES11Renderer(im::gles::ContextBinder*)
                                ; renderer constructed with a NULL ContextBinder
str r4,[r5,#0x2d0]              ; ctor stores that NULL at this+0x2d0
ldr r0,[r5,#0x2d0]              ; NULL every frame
blx r1                          ; virtual call on NULL -> nothing is drawn
```

Implemented a real Itanium RTTI walk (`__dynamic_cast`): classify a `type_info`
by comparing its vptr against the stubbed RTTI vtables (published by
`src/dyld.rs`), read the dynamic type from `*(vptr-4)`, walk public non-virtual
base paths from the dynamic type to both the source and target types, and return
`src + (offset(dyn→dst) - offset(dyn→src))`. `src/dyld.rs` now prefers the host
implementation over the bundled libstdc++ one at lazy-bind time (the symbol name
is `___dynamic_cast`).

### 2.2 EAGL present path

- `presentRenderbuffer:` now resolves the drawable renderbuffer from the EAGL
  context's `renderbuffer_drawable_bindings` instead of re-reading
  `GL_RENDERBUFFER_BINDING_OES` — the app binds renderbuffer 0 immediately before
  presenting, so the binding is not a reliable source.
- Guest framebuffer 0 now aliases the EAGL drawable: `renderbufferStorage:`
  creates an FBO with the drawable renderbuffer attached and
  `glBindFramebufferOES(..., 0)` redirects to it (iOS semantics). Without this
  the app's final composite quad went to the host window instead of the drawable.
- `present_renderbuffer` copies from a temporary FBO with the target renderbuffer
  attached rather than trusting the guest's currently bound FBO.
- The present path saves the guest's `GL_ACTIVE_TEXTURE`, selects
  `GL_TEXTURE0` for its own texture work and restores it afterwards. The app
  leaves a non-zero unit selected, so the present quad sampled an empty unit and
  the window stayed black.
- `libc::cxxabi` / present-path diagnostics are gated behind env vars and off by
  default.

### 2.3 UIKit / CoreGraphics

- Implemented `-[NSString sizeWithFont:minFontSize:actualFontSize:forWidth:lineBreakMode:]`
  (previously unanswered, so callers saw a zero `CGSize`).
- `CGFontGetAscent/Descent/Leading` now scale rusttype's v-metrics by the font's
  `units_per_em` instead of `units_per_em / 1.125`. The old code made every
  metric ~11% too small (EurostileLTStd: ascent 667 instead of 750).

## 3. Default options

- `touchHLE_default_options.txt`: `com.ea.deadspace.bv: --force-composition`,
  matching upstream touchHLE v0.3.0. Forces the compositing present path.

## 4. Diagnostics added (env-gated, off by default)

- `TOUCHHLE_DUMP_FRAMES=<dir>` — dump the presented drawable renderbuffer as PPM.
- `TOUCHHLE_DUMP_CG=<dir>` — dump the app's `CGBitmapContext` glyph atlas.
- `TOUCHHLE_USE_FAST_PRESENT` / `TOUCHHLE_FORCE_COMPOSITE` — choose the present path.
- `[--trace-gl-errors]` present-path probes (window back-buffer readback,
  renderbuffer content probe).

## Known remaining issues

- The in-game UI text still renders as smeared fragments. The app's glyph atlas
  is built correctly (verified by dumping it) and metrics/advances are correct;
  the fault is in the app's own UV maths for its text quads, which is still being
  investigated.
- The app calls `glDrawTex*iOES` (`GL_OES_draw_texture`); the GLES1-on-GL2 backend
  does not implement it (the trait default is a no-op), so those draws are
  currently dropped. Implementing it is the next planned step.

## Licensing

This fork is MPL-2.0, like its upstreams. See LICENSE, NOTICE and the derived-work
statement in NOTICE. No game data is included.
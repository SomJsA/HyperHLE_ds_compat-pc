# EAGL present-transfer cache ownership

This follows the earlier PresentState postprocessing fix. A separate set of EAGL
transfer caches still lived in thread_local storage: ES1 present texture/source
FBO, ES2 program/objects, attached renderbuffer and texture dimensions.

The real RTX4090D/NVIDIA617.14 regression extracted the existing production
texture-cache binding block without changing it. Two independent contexts copied
red/green source pixels. Context0 passed; context1 reused cached32x32 storage and
CopyTexSubImage2D returned GL_INVALID_VALUE0x501. See
report/pc-eagl-transfer-context-20261105/REPRO-SOURCE.json and
repro-texture-cache.log. This is a transfer ownership reproduction; the exact
player10h6x→4x90degree report remains open.

TransferState now belongs to the actual backing EAGL context. Its owner is chosen
by the same effective thread/main fallback resolver used to obtain the GL driver.
The shader/postprocess and GPU-card caches use that owner too. CPU-only RGBA
scratch remains reusable on the host thread. Normal cache hits and resize checks
remain; no per-frame texture allocation is introduced.

During EAGL deallocation, private transfer/postprocess/card resources are released
while that context is current. Afterwards the existing thread/main context map is
restored without modifying the guest mapping. GL calls remain synchronous on the
owning host thread; no GL object or Environment is sent to another thread.

The shared-context fixture must avoid numeric-name assumptions. This backend maps
OES framebuffer calls to EXT; the EXT specification allows shared FBO objects,
while core/ARB binding differs. OpenGL ES2.0.25 also warns that framebuffer sharing
can vary and recommends separately generated objects. Private helper attachments
are therefore maintained per context on every backend. Primary references:
[EXT_framebuffer_object dependencies](https://registry.khronos.org/OpenGL/extensions/EXT/EXT_framebuffer_object.txt),
[OpenGL ES2.0.25 appendixC](https://registry.khronos.org/OpenGL/specs/es/2.0/es_full_spec_2.0.pdf).

Validation: fixed-transfer-tests-03.log passes both hardware regressions:
independent-context red/green texture copies, owner-local storage reuse and release;
shared-context private helper/guest attachments and release preserving the other
context's objects. Existing3postprocess context/FSR/scale-rotation regressions pass
in existing-present-tests.log. Effective thread/main fallback1test and native
close scheduler3tests also pass. These are component tests; native integration is
still required.

All failed records remain: the old-cache0x501 failure is the actual reproduction;
fixed-transfer-tests.log was a fixture compile error on an unavailable generated
RGBA4 constant, corrected to production0x8056. fixed-transfer-tests-02.log passed
the independent texture case but stopped at a wrong shared numeric-overlap
assumption before attachment assertions. The final fixture avoids that assumption.

The offline guard rejects known GL-cache types/identifiers inside thread_local
macros and requires the owned-state anchor. CPU scratch and comments/raw string
history are accepted. Its existing Rust quote stripper was also corrected for
backslash/newline continuation, with a negative test for both cache and QTE rules.
guard-tests-03.log19tests and guard-source-working-02.json24source checks pass;
this structural guard is not a hardware lifetime proof.

Before accepting native integration: normal full release/map, private native
CPU/GPU font/menu/gameplay/close regressions and appropriate performance comparison. GPU copy/text defaults remain off; frozen v10/92ce6316 remains a
separate validated artifact. Android is an independent baseline and has not been
modified by this PC transfer-cache change. Full R15 acceptance remains open.

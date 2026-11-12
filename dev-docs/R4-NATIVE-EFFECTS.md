# R4 native effects and missing subtitles (2026-11-12)

Dead Space defaults to the iPhone 4S native device profile, retaining explicit model overrides and `TOUCHHLE_DS_NATIVE_EFFECTS=0` for diagnostic rollback. This selects original high-tier resources and GLES2 materials, including cube reflections, specular/Fresnel and native weapon/particle light eligibility. Original tier gates, material light limits and fog behavior are retained. It does not force Bloom or synthesize shadow maps, normal maps, SSAO or depth of field.

The GLES2 presenter rotates texture coordinates about the center, uses context-owned VBOs, preserves guest attributes/texture units/renderbuffer/masks, and routes through the existing platform quality policy. Core ES3 multisample storage/resolve is used when the actual native context supports it; older APPLE or single-sample fallback remains. Actual sample counts and effective uniforms are verified separately from requests. Windows text cards use a context-owned GLES2 shader with the existing card cache and CPU fallback.

The immutable game resource `subtitles.sb` is overlaid read-only only for the verified original or already patched bank, in the verified Dead Space bundle. The supplement adds Vill_58 and Van_12 to `banter_01` (6317/2107ms) and disambiguates the preceding duplicate Vill_12 event as `line_mine`. Native FMOD/HUD timing, pause, repeat, interruption and existing caption observation remain authoritative; no persistent voice hook is enabled. Unknown resources and writable nodes are left unchanged. `TOUCHHLE_DS_SUBTITLE_SUPPLEMENT=0` disables the supplement diagnostically.

No new marker, game IPA or save format is created. R1/R2 fixes, Android default-off mode switching, and community links are retained. Reporter IPA/video remain local ignored research and are excluded from source snapshots and releases.

Evidence is recorded under `report/r4-native-effects-release-20261112`. Real isolated GPU pixel tests cover both native GLES2 and desktop core paths, rotations, MSAA resolve, quality uniforms and guest state. Device results, package hashes, build inputs and installed APKs are recorded separately. These checks do not close the existing long-duration, cross-chapter, additional hardware or final player acceptance gates in `R15-RELIABILITY.md`.

## PC dynarmic source availability

The PC release pins dynarmic commit `4c0b798b5380f508649a57f4a4e16b224f885290`, including the existing Win64 ABI/JIT mapping work. Android retains its original dynarmic pin. The PC commit is mirrored under `codex/vendor-dynarmic-20261112-r4` in the PC repository; it is not an upstream touchHLE commit. If ordinary recursive initialization cannot find this revision, initialize the dynarmic clone, fetch that branch from `https://github.com/SomJsA/HyperHLE_ds_compat-pc.git`, then run `git submodule update --init --recursive` again. Release manifests record the exact compiler inputs and submodule revisions.

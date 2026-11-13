# R4 native opening fog recovery

The accepted GLES1 OpeningFog sequence is 10..25, a zero-range waiting phase, then a tiny positive recovery ramp. Native GLES2 zero-range shader handling alone does not cover the recovery ramp.

`ds_shader_opening_fog` validates the complete original linear fog binder, SetFog, SetProjection, getters and Uniform2f wrapper before the projection breakpoint is installed. Only its exact Uniform2f return PC, binding location, original Fog payload, black linear fog and opening perspective shape qualify. Observe location -1 and disabled fog as before, but change upload arguments only for enabled fog and valid location. Guest Fog objects, source matrices and CPU arguments stay unchanged. Unknown code or data uses the original upload.

History belongs to the actual effective EAGL context and renderer, independent of shared texture identity or the scoped texture fog toggle. Renderer history is bounded. Reuse the existing OpeningFog policy; do not replace it with a minimum fog distance or frame-number-only rule. Explicit TOUCHHLE_DS_OPENING_FOG_FIX=0 provides the same-binary control; bounded trace is opt-in via TOUCHHLE_DS_ES2_OPENING_FOG_TRACE=1.

Acceptance requires same JNI complete opening A/B, source and native input trace, actual image comparison after the protagonist turns and before native state 0, preserved geometry/effects, unchanged user saves, independent lab cleanup and final sleep. Static tests alone do not prove the reported flash fixed. Final evidence belongs in the root report and TODO, not inferred from this maintenance contract.

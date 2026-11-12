# Legacy GPU text default

First configuration already selects GPU text. Existing nonempty launcher INIs
previously used a different fallback when `gpu_text_overlay` was absent: CPU
text, while `gpu_present_copy` still defaulted to on. A native objective or
subtitle could therefore take the full CPU readback/composition route with
the GPU-copy checkbox enabled.

`LoadSettings` now uses the first-configuration GPU default for a missing key.
Explicit `gpu_text_overlay=0` remains the CPU compatibility choice; explicit
GPU and GPU-copy choices are preserved. Loading a legacy INI does not rewrite
it. Normal SaveSettings persists the selected value. No internal resolution,
texture, MSAA, renderer code or user file is changed by this fallback fix.

`tools/tests/PcGpuTextDefaultsTest.cs` loads the real launcher assembly and
constructs its hidden form with injected private package/Documents roots and
fixed display hardware. Five cases cover fresh configuration, missing legacy
key, explicit off/on, and independent GPU-copy off with missing text key.
Each checks load/save/reload, private INI location, unchanged manual3x and
existing INI contents before saving. No form is shown or game launched.
Before the change the two missing-key cases failed; afterwards5/5 pass.

Native evidence is in project report `resolution-gpu-20261107/text-replay`:
real locator objective, copy on, original3AFF runtime, CPU/GPU/GPU/CPU8s
segments at6x and3x, same output/checkpoint, actual submitted cards and stage
counts checked. CPU windows have hundreds of readback/composition calls;
GPU windows have zero such calls. GPU-text paired FPS median +60.33% at6x,
+32.28% at3x in this fixture, both native closes0. Cards are transient, so
these percentages are whole8s windows rather than constant full-card load.
They do not predict another CPU/GPU or a complete chapter, and do not prove
the reported player's setting. End-window screenshots confirm the scene,
not pixel equivalence of the transient cards. The renderer itself is unchanged.

Source and experimental launcher stay on the isolated research branch.
The delivered runtime, IPA, user configuration and save files are not replaced.

# Native ES1 vertex array buffer query

The native ES1 projection span and blood-size paths query the buffer captured
with the vertex array pointer before reading client memory. Their local0x889f
constant was a different generic vertex-attribute pname,not the ES1 vertex-array
buffer binding. Use the generated gles11::VERTEX_ARRAY_BUFFER_BINDING (0x8896)
at both existing sites. Do not replace it with the current ARRAY_BUFFER_BINDING:
that binding can change after the pointer captures another buffer.

This is a narrow port of Android10a4a7bf. The actual Adreno ES1 fixture covers
32 client/VBO cases,distinct captured/current buffers and exact draw pixels:
0x889f producesGL_INVALID_ENUM;0x8896 reports the expected captured binding.
Android actual trace-enabled gameplay subsequently removed the wrapper errors.
That evidence establishes the shared native query correction,not extra desktop
hardware game coverage. Windows GL2 implementation already uses its canonical
constants and is unchanged. Rendering/Finish/CPU/JIT defaults remain unchanged;
no FPS claim follows from this fix,particularly on the user's busy PC.

Normal PC release compilation and source/artifact guards must bind this source
separately from older4f binaries. Keep input,weapon,streaming,extraGPU gameplay,
exact10h6x->4x rotation and final human acceptance gates open.

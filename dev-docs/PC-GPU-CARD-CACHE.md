# GPU text card upload reuse

The GPU presenter formerly associated each texture with a draw-list index.
When a subtitle, panel, objective or QTE appeared/disappeared, an unchanged
cached bitmap could move to another index and be uploaded again. Texture
dimensions alone do not identify an image.

The presenter now looks up the existing immutable `Arc<CachedCard>` identity
within the actual GL context's state. It still submits cards in their original
order with unchanged vertices, pixels, alpha, opacity and sampling. No hashing,
bitmap copy or file access occurs on a cache hit. Changed/new images upload in
the owner context; an evicted same-size slot can use TexSubImage2D as before.

Retained slots have a16-image/32MiB RGBA-payload budget. Least-recently-used
inactive cards are recycled/deleted. A frame's currently required cards may
exceed this budget; after drawing, no inactive card can keep it above budget.
This bounds retained data, not exact driver VRAM or temporary allocation peaks.
The owning current context deletes every remaining texture on release.

`TOUCHHLE_DS_GPU_CARD_CACHE=0` selects the former position-indexed behavior for
a new state/process. `TOUCHHLE_DS_GPU_CARD_CACHE_TRACE=1` reports cumulative
upload calls/bytes and retained payload on changes; default tracing is off.
Both policies are read once per context state. CPU compatibility text is intact.

The ignored real-driver test
`card_cache_preserves_pixels_order_rotation_and_context_ownership` compares
the complete GPU framebuffer against the old indexed path for two unshared
contexts, four rotations, alpha/opacity, scale, changing draw order, and slot
eviction. It verifies884736 pixel positions exactly, zero GL errors and resource
release. Per context,108 uploads/163584 bytes become76 uploads/119104 bytes
for this component sequence. Six related card/layout/CPU fallback tests pass.
These are work-count/pixel results, not a whole-game FPS improvement claim.

Project report `resolution-optimization-20261107` binds the build, hardware
test logs and native chapter/subtitle/objective/pause observations. Native
events and final screenshots do not certify a complete chapter or anotherGPU.

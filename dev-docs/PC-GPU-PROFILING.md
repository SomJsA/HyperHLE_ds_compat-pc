# Asynchronous GPU command timing

Experimental opt-in: `TOUCHHLE_DS_GPU_PROFILE=1`. The normal default does not
create, submit or poll GL queries. This is a diagnostic observer, not an FPS
optimization or a default-policy change. Only the desktop GLES1-on-GL2 backend
implements it; other backends use the no-op trait methods.

Each real GL context owns its query state. ARB_timer_query and >=32 usable
timestamp bits are required. Every30th native present serial samples selected
draw commands, optional MSAA resolve, present-copy and final present commands.
The current observer also samples clears outside the final present. Every
profiler context has a distinct local owner tag; it is not an OS TID or GL
handle. Auxiliary-context reports must not be mixed into the app-copy context.
At most256 pairs exist per context. Exhaustion drops samples and is reported;
incomplete coverage must not be called a whole-frame GPU measurement.

Polling waits at least3 native serials, checks QUERY_RESULT_AVAILABLE, and only
then fetches the two timestamps. There is no profiler glFinish, fence wait or
pixel read. EAGL deallocation deletes outstanding query objects with their
owner current, without waiting; unresolved tail pairs are explicitly reported.
Context destruction also reclaims its non-shared query objects.

`[DSgpu-profile]` JSON contains Unix milliseconds, native serial, stage calls,
sum/max GPU nanoseconds, pending/created pairs and cumulative dropped count.
CPU preparation, restoration and swap/current spans report wall nanoseconds
and QueryThreadCycleTime deltas separately. Cycle-read success counts are
explicit. Never convert those cycles to milliseconds or call them GPU work:
https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-querythreadcycletime
One log covers roughly1 wall second. Filter whole report windows inside the
measurement interval; startup/capture/shutdown and observer-on FPS are separate.
The final present includes helper-card rendering; its draws are excluded from
the separate draw/clear buckets. Optional special diagnostic draw helpers are
not all covered by the scene bucket. These command spans can include GPU idle
gaps caused by host submission and query overhead: they are not exclusive GPU
occupancy or proof of a CPU stall. Do not add nested CPU GL wall spans to GPU
timestamps, or infer GPU times from a CPU duration of glCopyTexSubImage2D.

Specification: https://registry.khronos.org/OpenGL/extensions/ARB/ARB_timer_query.txt

Real-driver ignored test `gpu_timestamps_keep_queries_and_pixels_in_their_own_contexts`
uses two independent hidden contexts, checks completed red/green pixels, valid
nonzero timestamps, zero GL errors and deletion in the owning context. Its
explicit test-only pixel synchronization is not part of the production observer.

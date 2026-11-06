# Windows x64 guarded JIT fastmem candidate

Default OFF. Explicit `TOUCHHLE_DS_CPU_FASTMEM=1` enables only when a full4GiB direct guest reservation and a uniform, page-aligned null segment can be guarded. Unknown/mixed shapes keep the original page table. No unsafe Dynarmic accuracy optimizations, scheduler/tick or game-clock changes.

The null range is either newly committed NOACCESS (and decommitted on wrapper destruction), or existing committed READWRITE temporarily protected and exactly restored. The JIT is destroyed first. Windows lazy-commit VEH must handle only MEM_RESERVE faults, leaving committed protection violations to Dynarmic's exception fallback. Original zero-read/absorbed-write null callbacks and page table remain.

`cargo test --offline --locked --release windows_fastmem -- --test-threads=1 --nocapture` exercises the production JIT/wrapper and real lazy-commit handler, alternating six modes with committed/reserved null cases, matching registers/CPSR/data, real null load-store-load, exact guard restoration, no-direct-memory and zero-null fallback. Component timings are not whole-game FPS gains.

ARM fixture at0x2000:
```
outer: mov r0,r6; mov r1,r7
inner: ldr r5,[r0],#4; add r3,r3,r5; eor r5,r5,r4; add r4,r4,#1
       str r5,[r0,#-4]; subs r1,r1,#1; bne inner
       subs r2,r2,#1; bne outer; svc #1
```

Release/gameplay and actual timing validation pending. Source and fixture alone do not certify the PC main-thread bottleneck solved, default enablement, stable60, cross-level or full R15 acceptance. Existing IPA/saves/releases remain immutable; run only private copies via control inbox. Android implementation is separate and unchanged.

## Validated Windows boundary and library pin

The actual production-JIT tests passed2/2 after correcting Win64 callback ABI shadow space. Six ABBAAB memory-loop rounds retained identical registers/CPSR/data; reserved/committed NULL load-store-load and exact restoration passed. Median component14.246ms page-table vs12.522ms fastmem (~12.1% elapsed reduction), not total-game gain. First two failed runs are retained in PC-HOTPATH local evidence.

The original Dynarmic Win64 exception handler reserved only8 bytes before calling C++. Its callback may home the four register arguments, overwriting the handler return address. CDB observed a second-chance attempt to execute a stack address in the real NULL fallback. Reserve0x28 (32bytes shadow +8alignment), restore0x28. Official ABI: https://learn.microsoft.com/en-us/cpp/build/x64-calling-convention?view=msvc-170

PC Dynarmic pin: `4a145d3850488598d6653aa1e57873a0bbda0995`. `tools/project-guard/contract.json` has an explicit PC-only override with rationale; Android/default e0f6bd9d remains unchanged. Do not recopy the platform tree or silently refresh baseline hashes. Guard still rejects unknown paths, malformed commits, mismatched/dirty submodule pointers. Windows flag reads GetEnvironmentVariableA, so Rust runtime/test overrides do not hit the CRT getenv cache.

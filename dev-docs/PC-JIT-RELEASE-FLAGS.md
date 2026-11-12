# Windows JIT compiler release flags

The default MSVC generator path in cmake-rs0.1.58 supplies configuration flags
to select the CRT. It replaces `CMAKE_C_FLAGS_RELEASE` and
`CMAKE_CXX_FLAGS_RELEASE`, losing CMake's `/O2 /Ob2 /DNDEBUG` defaults.
Rust's optimized release profile and an optimized C++ wrapper do not fix the
separate Dynarmic CMake library. The observed Release project had an empty
Optimization property and no NDEBUG; saved-IP observations also reached
out-of-line decoder/container helpers while cold weapon code was translated.

`src/cpu/dynarmic_wrapper/build.rs` explicitly supplies
`/O2 /Ob2 /DNDEBUG /MD` for Windows/MSVC non-debug builds. Debug and other
targets retain their existing settings. No unsafe Dynarmic optimizations,
fast-math, guest instruction changes, extra game execution or host skin
acceleration are enabled. Original IPA and saved progress remain immutable.

After a normal release build, inspect the actual generated CMake cache and
Dynarmic Release vcxproj/CL command record: Optimization must be MaxSpeed,
NDEBUG must be present, and the CRT must be MultiThreadedDLL. A cargo
`Finished release [optimized]` line alone does not verify native dependencies.
Keep these build inputs with the executable hash. Project-guard's source and
resource checks do not inspect an external CMake build directory.

The release JIT regression covers real page-table/fastmem/null-page execution
and ARM first-PUSH replay with ARM/Thumb return, registers, flags and stack.
Skin and transform regression outputs are checked separately. Native cold
shot evidence is in `report/cold-first-shot-fix-20261107` in the project root.
Initial instrumented runs reduced one first-shot action112.577ms to49.832ms,
and the following frame194.381ms to61.364ms, but observers and dynamic enemy
scenes prevent interpreting that as a controlled whole-game FPS percentage.
This build correction alone does not certify first-blood latency resolved;
observer-off cold processes and actual hit/effect coverage are still required.

Automatic diagnostics stay disabled by default. An explicit private test
observer does not change that policy or the independent manual F7 path.

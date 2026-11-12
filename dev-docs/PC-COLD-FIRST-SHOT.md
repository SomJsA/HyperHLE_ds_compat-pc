# PC cold first-shot path

The reported hitch was reproduced on an owned Windows child with a copied seed,
the original IPA, GPU copy, 6x internal scale and HD texture pack. The first
enemy hit entered the original `GameObjectNecromorph::OnDamageReceived` path.
Before this change, the frame after the first blood hit reached about 104 ms;
the action also demand-translated hundreds of guest blocks. The long spans were
mostly cooperative guest worker waits (`usleep(10 ms)`, semaphore waits and
Objective-C callbacks), with JIT translation adding a smaller cold component.

Two bounded PC changes are now enabled by default for the Windows x64 Dead
Space profile:

* The Release CMake builds for Dynarmic and OpenAL Soft explicitly retain
  `/O2 /Ob2 /DNDEBUG /MD`. cmake-rs otherwise replaces the Release flag
  variables with CRT/tool flags, leaving these separately built libraries with
  an empty optimization property.
* Once a real gameplay player exists, the runtime translates 1,257 immutable
  ARM/Thumb guest blocks collected from the original first-weapon and first-hit
  paths. This is translation only: no guest code, HLE callback, audio, GL,
  input, ammo, enemy or save state is executed or modified. It runs once before
  the first gameplay action and is skipped when
  `TOUCHHLE_DS_PC_JIT_PREWARM=0`.

During the same controlled cold run, prewarm took about 31 ms before the first
action. The first actual fire action fell from roughly 50 ms to 7–8 ms. The
first hit's following frame fell from roughly 104 ms to 24 ms; the remaining
time is native game worker/audio synchronization rather than translation. A
Windows PC combat sleep policy shortens only exact 10 ms guest polling sleeps
to 2.5 ms while gameplay is active, reducing serialized polling without
changing semaphore waits. Set `TOUCHHLE_DS_PC_COMBAT_LATENCY=0` for the exact
legacy timing comparison.

The prewarm list is tied to the immutable original Dead Space guest code
addresses. It is not a general JIT cache, does not write guest memory, and is
not enabled on Android or other applications. Normal automatic diagnostics and
the independent manual F7 path remain unchanged.

Validation is in `report/cold-first-shot-fix-20261107`: actual weapon-0 fire,
enemy hit records, before/after frame intervals, exact JIT component fixture,
OpenAL loopback fixture, seed/IPA hashes and normal WM_CLOSE cleanup. The
controlled runs are performance evidence for this path, not a claim about all
chapters or every GPU.

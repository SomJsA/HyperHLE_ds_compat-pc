# Native station entry repair (2026-10-31)

A verified live case had Store::CanInteract return 1 while E selected an ammunition pickup behind the store. Generic TapToAction lookup was insufficient for station interaction. The Windows shortcut now considers the current world's native Store and Bench actors before the pickup fallback.

The entry checks current HUD gameplay state, actor vtable, world and enabled byte; executes the original Store::CanInteract (0x9165c) or Bench::CanInteract (0xb5b84); then calls the original OnInteract (0x92f84 / 0xb58fc). Exact entry opcodes are checked. These two overrides ignore their transform argument; this exception must not be generalized to other GameObjectInteractive classes. Native range, facing, station phase and combat restrictions remain authoritative. No player, HUD, inventory or currency field is written, and E does not inject a screen tap.

Actor discovery reads checked memory chunks once per world, caches even an empty result, and rejects invalid/reused object vtables. The diagnostic spatial probe reuses that cache. Do not reintroduce repeated full-heap per-word scans on every rendered frame. Android retains its existing interaction path.

## Validation and scope

`tools/pc-runtime/vm/verify-native-stations.py` executes 23 original ARM entry-gate cases. Store geometry runs unchanged; tweak/world combat providers and Bench IsPlayerInRange are isolated external dependencies. Correct front-facing range accepts; combat, wrong side/facing/distance and invalid phases reject. Both overrides execute with a null unused transform argument.

Candidate R7f SHA256: ECA0E1E84B869E39121AAB6A3F5A75B23093CFEC90047AA6453EC0CFF00EBA74. It passed 116 DS Rust tests. Private cloned checkpoint fixtures reached both real station interfaces via E. Store native sale +1500, native buy -800, and scrolled-row sale +1050 matched displayed credit balances. Workbench current Corer grid contained 16 nodes plus two fixed actions; native Blank1 purchase reduced inventory nodes 3 to 2. Canceling another preview retained its native cart reservation, and backing out restored the display to 2. GPU-copy-on popup/bench/store-return frames were internally captured on RTX4090. These observations do not establish normal walking/story progression, physical keyboard/gamepad acceptance, other GPU correctness or a performance default.

Evidence under workspace `perf-continuation-20261030/R7{E,F}-{STORE,BENCH}-UI-LIVE`, plus `r7-native-stations-original-arm.json`. Fixtures change only their own serialized player transform and DATA checksum, preserve original saves, and retain native currency/state operations. The full runtime also includes pending frontend UI adaptation; this focused commit contains station entry and its contract test, not the entire dirty worktree. Keep the independent stasis acquisition gate e09c52f2 and accepted D24/five alpha-mip repairs intact.

# Original IPA plus patches

The input IPA is immutable. It is not replaced by a newly authored game build.
R15 mounts the native text patch in the guest filesystem at startup; select the
original/previously accepted base IPA directly. No derived IPA is required.

The repository owns these patch layers:

| Layer | Contents |
| --- | --- |
| Native text | `res/overlay/deadspace_iphone.bin`, mounted read-only at `published/stringdata/CHS_CN/deadspace_iphone.bin` |
| Emulator code | input, UI, overlay, chapter rendering, save, and performance changes in `touchHLE.exe` |
| Emulator assets | chapter fonts, overlay JSON, input prompts, launcher defaults |
| Rebuild source | the six-column ledger and deterministic import tools |

## Runtime mount

`src/fs/ds_resource_overlay.rs` embeds the repository SBIN into the PC executable
and Android native library. Both platforms use the same VFS mount, including
extracted `.app` bundles. The patch is available without a writable IPA, sidecar
discovery, APK asset extraction, or launcher-specific installation.

Only `com.ea.deadspace.bv` and an existing, read-only Chinese member are eligible.
The source SHA-256 must match the original, the historical translation baseline,
the known preceding revision, or the
current embedded patch. Unknown or unreadable resources keep their original
node. No directories or game resources are synthesized. App-picker inspection
does not mount patches; gameplay startup does.

The original 1.3.34 backup IPA and unchanged iPad2 asset base both contain native
Chinese SBIN SHA-256 `47864a0c746cf439b9bab93e66aea621ae3896f2e517f710436b6302b0a3db77`.
The old tool's `4732db25...d36f0` "original" was a historical translation base;
it remains accepted for compatibility, but is not the untouched backup resource.
The actual original has the same 996 keys, with 844 changed after the recorded
currency correction. The current count is maintained in `ledger-audit.json`;
`text-corrections.json` preserves verified fixes during subsequent imports.

The mounted node reports the patch length through VFS size/stat and descriptor
length/fstat. Read, seek, EOF, and cloned handles use the same immutable bytes.
Write, append, truncate, rename, and removal remain denied. Other languages,
assets, bundle identity, and save paths are unaffected. Mounting happens once;
there is no per-frame or per-read hashing.

Startup logs `[DSresource-overlay] mounted ...` with source/patch hashes and byte
length. For a diagnostic rollback, set `TOUCHHLE_DS_NATIVE_TEXT_PATCH=0` before
launch; the default is enabled. This switch affects only the native SBIN, not
the separate high-resolution overlays. Updating the ledger requires rebuilding
the runtime to incorporate the new native patch.

## Optional legacy packaging

`Apply-RevisedIpa.py` remains an optional legacy utility. Creating a derived IPA
requires explicit user confirmation; it is not part of R15 release packaging.
It copies every member of an original
or known previous-revision IPA unchanged, replaces only the Chinese SBIN member,
recalculates that ZIP entry, and verifies it by reading the output back.

The output is therefore a derived runnable IPA, not a new game base:

```text
original IPA + one native-string patch -> derived runnable IPA
```

Keep the original IPA and the patch resource. Current R15 PC and Android
runtimes mount the patch without generating this derived artifact.

## Android input storage

`BundledIpaStore` never replaces an existing user IPA. An existing legacy file
with the matching previous bundle marker remains selected, including a user's
customized copy. Missing or stale markers do not authorize deletion. The exact
trusted bundled bytes can instead be copied into a separate hash-addressed
`touchHLE_bundled_cache/<sha256>/DeadSpace.ipa`; content is verified before a new
directory is published. A differing existing cache is retained and rejected.
This is a bit-identical input copy, not a derived/patched IPA. Runtime text
mounting remains the same on both platforms.

See `tools/project-guard/README.md` for offline source/artifact checks and the
versioned save/IPA protection probes. No check command generates an IPA.

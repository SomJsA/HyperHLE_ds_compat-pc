# Reproducible Build Guide

## Target

Windows x64 build of the patched HyperHLE tree, for Dead Space 1.3.34 ARMv7.

## Source versions

Base: HyperHLE commit `2281c14`.

Dependency source archives are stored in `vendor/`:

| Component | Source archive | SHA256 |
|---|---|---|
| SDL | SDL.tar.gz | A2DA78889A98444562C2F917F794E406BC0CAF68EC49CE996D87217E457F1A31 |
| dynarmic | dynarmic.tar.gz | 7C1988D6F0ABB894E6AC2A52AEDD87936858CD83709EE3EFAAB96163D63371D6 |
| libxml2 | libxml2.tar.gz | 332B783F706D9429DE53762F118E918E96B35C2A9E57582A224FC2F6025F1475 |
| OpenAL Soft | openal-soft.tar.gz | 262D3CA3E57C164C0F4E0FA9A9B69BCBEFBBDC5123EBC73526894878B5721D36 |
| stb | stb.tar.gz | 475079FFBB9431F4D9EA6E3518E553B6810A75F0A4354262E83D2FBBFA8C3E31 |
| rust-sdl2 | rust-sdl2.tar.gz | E9FFDF41E57B7A8F31F9BC6406784B196F8F7955EF8BF4E01EB7442DA9920B45 |
| Boost 1.81.0 | boost_1_81_0.tar.gz | 205666DEA9F6A7CFED87C7A6DFBEB52A2C1B9DE55712C9C1A87735D7181452B6 |

## Required patches

`patches/` carries this fork's changes:

```text
patches\0001-deadspace-build-and-compat.patch
patches\0002-rust-sdl2-cmake-policy.patch
```

`0001` contains:

- local rust-sdl2 dependency redirect
- CMake policy compatibility for OpenAL Soft
- Dead Space 1.3.34 LogBuffer `dynamic_cast` compatibility patch

`0002` adds the CMake policy compatibility setting to rust-sdl2's SDL2 build.

`0003` is the full working diff for the compatibility work.

## Toolchain

- Rust stable
- Visual Studio 2022 C++ Build Tools
- CMake 4.x
- Boost 1.81 headers in `vendor/boost`
- Vendored SDL, dynarmic, libxml2, OpenAL Soft, stb and rust-sdl2 sources

## Build

From the repository root:

```powershell
cargo build --release
```

Expected output: `target\release\touchHLE.exe`

## Run

Run your own legally obtained copy of the application directly, for example:

```powershell
.\target\release\touchHLE.exe "path\to\your\app.ipa"
```

The original application bundle is not modified.

---

## Fork-specific notes (Dead Space compatibility fork)

1. **Initialise the submodules** (SDL, dynarmic, openal-soft, stb, PVRTDecompress,
   libxml2):

   ```bash
   git submodule update --init --depth 1
   ```

2. **Boost is required but not committed** (upstream `.gitignore` also ignores
   `/vendor/boost`). Download Boost (1.81 tested) and unpack it so that
   `vendor/boost/` contains the Boost headers, e.g.:

   ```powershell
   # from the repository root
   curl -L -o vendor/boost.tar.gz https://archives.boost.io/release/1.81.0/source/boost_1_81_0.tar.gz
   tar -xzf vendor/boost.tar.gz -C vendor
   Move-Item vendor/boost_1_81_0 vendor/boost
   ```

   `src/cpu/dynarmic_wrapper/build.rs` will otherwise abort with
   "Could not find Boost".

3. `vendor/rust-sdl2` **is** committed (it is patched and used through a
   `[patch]` entry in `Cargo.toml`).

4. Build:

   ```powershell
   cargo build --release
   ```

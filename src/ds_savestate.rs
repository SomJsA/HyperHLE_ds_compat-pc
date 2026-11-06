/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Best-effort runtime save-state ("snapshot") support for the Dead Space
//! compatibility fork.
//!
//! Shift+F5 dumps the current guest memory + CPU registers to a file;
//! Shift+F9 restores the most recent dump. This is a debugging aid meant to
//! capture a bug *in place* so it can be inspected or reproduced later, in the
//! spirit of an emulator save-state.
//!
//! Scope / limitations of v1:
//! - Only the *current* thread's register set is captured. Other threads'
//!   saved contexts are left untouched, and running coroutines/fibers are not
//!   rewound.
//! - Host-side state (GL textures/VBOs, audio, Objective-C caches, open file
//!   descriptors, ...) is *not* captured, so a restore is approximate: it is
//!   often good enough to get back to the same gameplay moment, but it is not
//!   a bit-perfect rewind.
//! - Restoring while the emulator is mid-frame can leave GL state inconsistent;
//!   press Shift+F9 while the game is paused (or on a stable screen) for best
//!   results.
//!
//! File layout (all integers little-endian):
//!
//! ```text
//! magic        [u8; 8]   b"DSSTATE1"
//! version      u32       currently 1
//! region_count u32
//! region[0..n] { base: u32, size: u32, bytes: [u8; size] }
//! regs         [u32; 16]
//! cpsr         u32
//! ```

use std::io::{BufReader, Read, Write};

use crate::mem::{GuestUSize, Ptr};
use crate::Environment;

const MAGIC: &[u8; 8] = b"DSSTATE1";
const VERSION: u32 = 1;

/// Directory the state files live in. Defaults to the current working
/// directory; override with `TOUCHHLE_DS_STATE_DIR`.
fn state_dir() -> std::path::PathBuf {
    match std::env::var_os("TOUCHHLE_DS_STATE_DIR") {
        Some(p) => std::path::PathBuf::from(p),
        None => std::path::PathBuf::from("."),
    }
}

fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Dump the current guest memory + CPU state to a new `state-<unix>.dsstate` file.
///
/// Never panics: any error is reported through `echo!` and the emulator keeps
/// running.
pub fn save(env: &mut Environment) {
    let dir = state_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        echo!("[DSstate] cannot create dir {:?}: {}", dir, e);
        return;
    }
    // Put the presented-frame number in the file name: the frame counter lives
    // on the host, so it is NOT part of the snapshot itself, and without this
    // there is no way to tell afterwards which moment a state came from.
    let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!("state-{}-f{:06}.dsstate", timestamp(), frame));
    // If frame dumping is on, also keep the picture of this exact moment (the
    // dumper burns the shot number into it).
    if std::env::var_os("TOUCHHLE_DS_DUMP_FRAMES").is_some() {
        crate::window::DS_MARK.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    echo!("[DSstate] frame {} -> {}", frame, path.display());

    match write_state(env, &path) {
        Ok((regions, bytes)) => echo!(
            "[DSstate] saved {} region(s), {} bytes of guest memory -> {:?}",
            regions,
            bytes,
            path
        ),
        Err(e) => echo!("[DSstate] save failed: {}", e),
    }
}

/// Restore the most recent state file, or the one named by
/// `TOUCHHLE_DS_STATE_LOAD` if that variable is set.
///
/// Never panics.
pub fn load(env: &mut Environment) {
    let path = match std::env::var_os("TOUCHHLE_DS_STATE_LOAD") {
        Some(p) => std::path::PathBuf::from(p),
        None => match newest_state(&state_dir()) {
            Some(p) => p,
            None => {
                echo!("[DSstate] load: no state file found");
                return;
            }
        },
    };

    match read_state(env, &path) {
        Ok((regions, bytes)) => echo!(
            "[DSstate] restored {} region(s), {} bytes of guest memory from {:?}",
            regions,
            bytes,
            path
        ),
        Err(e) => echo!("[DSstate] load failed: {}", e),
    }
}

fn newest_state(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut best: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("state-") && name.ends_with(".dsstate") {
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().map_or(true, |(t, _)| mtime > *t) {
                best = Some((mtime, entry.path()));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Windows refuses to write from a buffer that touches a guard page
/// (`ERROR_INVALID_USER_BUFFER`, 1784). Guest memory has guard pages between
/// allocations, so a "used range" slice can span one. Write page by page and
/// substitute zeros for any page the OS will not read: a guard page carries no
/// guest data, and the load side skips pages it cannot write anyway.
fn write_guest_region(w: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<u64> {
    const PAGE: usize = 4096;
    let mut written: u64 = 0;
    let zeros = [0u8; PAGE];
    let mut i = 0usize;
    while i < bytes.len() {
        let n = PAGE.min(bytes.len() - i);
        let chunk = &bytes[i..i + n];
        match w.write_all(chunk) {
            Ok(()) => {}
            Err(e) if e.raw_os_error() == Some(1784) => {
                w.write_all(&zeros[..n])?;
            }
            Err(e) => return Err(e),
        }
        written += n as u64;
        i += n;
    }
    Ok(written)
}

fn write_zeros(w: &mut std::fs::File, mut count: u64) -> std::io::Result<()> {
    const PAGE: usize = 4096;
    let zeros = [0u8; PAGE];
    while count > 0 {
        let n = PAGE.min(count as usize);
        w.write_all(&zeros[..n])?;
        count -= n as u64;
    }
    Ok(())
}

fn write_state(env: &mut Environment, path: &std::path::Path) -> std::io::Result<(usize, u64)> {
    let regions: Vec<(GuestUSize, GuestUSize)> = env.mem.iter_used_ranges().collect();

    let file = std::fs::File::create(path)?;
    // Write directly (no BufWriter): the per-chunk failure below must be
    // attributed to the exact buffer we handed to the OS.
    let mut w = file;

    w.write_all(MAGIC)?;
    w.write_all(&VERSION.to_le_bytes())?;
    w.write_all(&(regions.len() as u32).to_le_bytes())?;

    let mut total: u64 = 0;
    for &(base, size) in &regions {
        w.write_all(&base.to_le_bytes())?;
        w.write_all(&size.to_le_bytes())?;
        let ptr = Ptr::<u8, false>::from_bits(base);
        let bytes = env.mem.unchecked_bytes_at(ptr, size);
        let written = write_guest_region(&mut w, bytes)?;
        // `unchecked_bytes_at` may hand back a short slice (null-page case), so
        // pad with zeros to keep the on-disk layout exactly `size` bytes: the
        // load side reads `size` bytes unconditionally.
        if written < size as u64 {
            write_zeros(&mut w, size as u64 - written)?;
        }
        total += size as u64;
    }

    for r in env.cpu.regs().iter() {
        w.write_all(&r.to_le_bytes())?;
    }
    w.write_all(&env.cpu.cpsr().to_le_bytes())?;

    w.flush()?;
    Ok((regions.len(), total))
}

fn read_state(env: &mut Environment, path: &std::path::Path) -> Result<(usize, u64), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut r = BufReader::new(file);

    let mut magic = [0u8; 8];
    r.read_exact(&mut magic).map_err(|e| e.to_string())?;
    if &magic != MAGIC {
        return Err("bad magic (not a .dsstate file)".into());
    }
    let version = read_u32(&mut r)?;
    if version != VERSION {
        return Err(format!("unsupported state version {}", version));
    }
    let count = read_u32(&mut r)? as usize;

    let mut total: u64 = 0;
    for _ in 0..count {
        let base = read_u32(&mut r)?;
        let size = read_u32(&mut r)?;
        let mut buf = vec![0u8; size as usize];
        r.read_exact(&mut buf).map_err(|e| e.to_string())?;
        let ptr = Ptr::<u8, true>::from_bits(base);
        let dst = env.mem.bytes_at_mut(ptr, size);
        // bytes_at_mut redirects null-page writes to a sink, which can return
        // a shorter slice; only copy when the lengths line up.
        if dst.len() == buf.len() {
            dst.copy_from_slice(&buf);
        }
        total += buf.len() as u64;
    }

    let mut regs = [0u32; 16];
    for v in regs.iter_mut() {
        *v = read_u32(&mut r)?;
    }
    let cpsr = read_u32(&mut r)?;

    env.cpu.regs_mut().copy_from_slice(&regs);
    env.cpu.set_cpsr(cpsr);

    Ok((count, total))
}

fn read_u32<R: Read>(r: &mut R) -> Result<u32, String> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u32::from_le_bytes(b))
}

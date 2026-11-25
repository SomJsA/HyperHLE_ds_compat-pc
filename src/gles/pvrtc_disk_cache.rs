/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Persistent, disposable copies of decoded PVRTC textures.

use std::collections::HashMap;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

const DIRECTORY: &str = "touchHLE_pvrtc_cache";
const MAX_FILES: usize = 8192;
const BUDGET: u64 = if cfg!(target_os = "android") {
    1536
} else {
    2048
} * 1024
    * 1024;

struct Entry {
    bytes: u64,
    last_used: SystemTime,
}

struct DiskCache {
    root: PathBuf,
    // Keep the OS lock for the life of the inventory. Another process using
    // this root can still decode textures without the optional disk cache.
    _lock: File,
    entries: HashMap<String, Entry>,
    budget: u64,
    bytes: u64,
}

static CACHE: OnceLock<Option<Mutex<DiskCache>>> = OnceLock::new();

fn cache() -> Option<&'static Mutex<DiskCache>> {
    CACHE
        .get_or_init(|| {
            let root = crate::paths::user_data_base_path().join(DIRECTORY);
            match DiskCache::open(root, BUDGET) {
                Ok(cache) => Some(Mutex::new(cache)),
                Err(error) => {
                    log!("PVRTC disk cache unavailable: {error}");
                    None
                }
            }
        })
        .as_ref()
}

pub(crate) fn maintain_existing() {
    if crate::paths::user_data_base_path().join(DIRECTORY).exists() {
        let _ = cache();
    }
}

fn filename(key: u64, is_2bit: bool, opaque: bool, width: u32, height: u32) -> String {
    let bits = if is_2bit { 2 } else { 4 };
    let alpha = if opaque { 'o' } else { 'a' };
    format!("{key:016x}-{bits}{alpha}-{width}x{height}.rgba")
}

fn entry_size(name: &str) -> Option<(u64, bool)> {
    let (stem, extension) = name.rsplit_once('.')?;
    let temporary = match extension {
        "rgba" => false,
        "tmp" => true,
        _ => return None,
    };
    let (hash, layout) = stem.split_once('-')?;
    let (format, dimensions) = layout.split_once('-')?;
    if hash.len() != 16
        || !hash
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || !matches!(format, "2a" | "2o" | "4a" | "4o")
    {
        return None;
    }
    let (w, h) = dimensions.split_once('x')?;
    let width: u32 = w.parse().ok()?;
    let height: u32 = h.parse().ok()?;
    if width == 0 || height == 0 || w != width.to_string() || h != height.to_string() {
        return None;
    }
    Some((
        u64::from(width)
            .checked_mul(u64::from(height))?
            .checked_mul(4)?,
        temporary,
    ))
}

fn linked(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Also reject junctions and other directory reparse points.
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

impl DiskCache {
    fn open(root: PathBuf, budget: u64) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        let root_metadata = fs::symlink_metadata(&root)?;
        if linked(&root_metadata) || !root_metadata.is_dir() {
            return Err(io::Error::other("unexpected PVRTC cache directory"));
        }
        let lock_path = root.join(".pvrtc-cache-lock");
        if let Ok(metadata) = fs::symlink_metadata(&lock_path) {
            if linked(&metadata) || !metadata.is_file() {
                return Err(io::Error::other("unexpected PVRTC cache lock"));
            }
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock.try_lock().map_err(io::Error::other)?;
        let mut cache = Self {
            root,
            _lock: lock,
            entries: HashMap::new(),
            budget,
            bytes: 0,
        };
        let mut removed = 0;
        for item in fs::read_dir(&cache.root)? {
            let item = item?;
            let Ok(name) = item.file_name().into_string() else {
                continue;
            };
            let Some((expected, temporary)) = entry_size(&name) else {
                continue;
            };
            let Ok(metadata) = fs::symlink_metadata(item.path()) else {
                continue;
            };
            if linked(&metadata) || !metadata.is_file() {
                continue;
            }
            let bytes = metadata.len();
            if temporary || bytes != expected {
                fs::remove_file(item.path())?;
                removed += bytes;
                continue;
            }
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            cache.bytes += bytes;
            cache.entries.insert(
                name,
                Entry {
                    bytes,
                    last_used: modified,
                },
            );
        }
        removed += cache.trim(0, 0)?;
        if removed != 0 {
            log!("PVRTC disk cache: reclaimed {removed} bytes (limit {budget} bytes)");
        }
        Ok(cache)
    }

    fn trim(&mut self, incoming: u64, incoming_files: usize) -> io::Result<u64> {
        let mut count = self.entries.len();
        if self.bytes + incoming <= self.budget && count + incoming_files <= MAX_FILES {
            return Ok(0);
        }
        let mut oldest: Vec<_> = self
            .entries
            .iter()
            .map(|(name, entry)| (entry.last_used, name.clone(), entry.bytes))
            .collect();
        oldest.sort_unstable();
        let mut removed = 0;
        for (_, name, size) in oldest {
            match fs::remove_file(self.root.join(&name)) {
                Ok(()) => removed += size,
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error),
            }
            self.entries.remove(&name);
            self.bytes -= size;
            count -= 1;
            if self.bytes + incoming <= self.budget && count + incoming_files <= MAX_FILES {
                break;
            }
        }
        Ok(removed)
    }

    fn load(&mut self, name: &str) -> Option<Vec<u32>> {
        let entry = self.entries.get_mut(name)?;
        let path = self.root.join(name);
        let metadata = fs::symlink_metadata(&path).ok()?;
        if linked(&metadata) || !metadata.is_file() || metadata.len() != entry.bytes {
            return None;
        }
        let mut bytes = Vec::with_capacity(entry.bytes as usize);
        File::open(&path)
            .ok()?
            .take(entry.bytes)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 != entry.bytes {
            return None;
        }
        entry.last_used = SystemTime::now();
        Some(
            bytes
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
                .collect(),
        )
    }

    fn store(&mut self, name: String, pixels: &[u32]) -> io::Result<()> {
        let bytes = pixels.len() as u64 * 4;
        if bytes == 0 || bytes > self.budget || entry_size(&name) != Some((bytes, false)) {
            return Ok(());
        }
        let path = self.root.join(&name);
        if self.entries.contains_key(&name) {
            match fs::symlink_metadata(&path) {
                Ok(metadata) if !linked(&metadata) && metadata.is_file() => {
                    if metadata.len() == bytes {
                        return Ok(());
                    }
                    fs::remove_file(&path)?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Ok(_) => return Ok(()),
                Err(error) => return Err(error),
            }
            self.bytes -= self.entries.remove(&name).unwrap().bytes;
        } else if fs::symlink_metadata(&path).is_ok() {
            return Ok(());
        }
        self.trim(bytes, 1)?;
        let tmp = path.with_extension("tmp");
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        let mut buffer = Vec::with_capacity(bytes as usize);
        for pixel in pixels {
            buffer.extend_from_slice(&pixel.to_le_bytes());
        }
        let written = file.write_all(&buffer);
        drop(file);
        let result = written.and_then(|_| fs::rename(&tmp, &path));
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result?;
        self.bytes += bytes;
        let now = SystemTime::now();
        self.entries.insert(
            name,
            Entry {
                bytes,
                last_used: now,
            },
        );
        Ok(())
    }
}

pub(super) fn load(
    key: u64,
    is_2bit: bool,
    opaque: bool,
    width: u32,
    height: u32,
) -> Option<Vec<u32>> {
    cache()?
        .lock()
        .ok()?
        .load(&filename(key, is_2bit, opaque, width, height))
}

pub(super) fn store(
    key: u64,
    is_2bit: bool,
    opaque: bool,
    width: u32,
    height: u32,
    pixels: &[u32],
) {
    if let Some(mut cache) = cache().and_then(|cache| cache.lock().ok()) {
        let _ = cache.store(filename(key, is_2bit, opaque, width, height), pixels);
    }
}

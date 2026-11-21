//! Windows-only, fingerprint-checked native HUD resource mount.
use super::{EmbeddedFile, FileLocation, FsNode};
use sha2::{Digest, Sha256};
use std::{io::Read, sync::OnceLock};
#[path = "ds_weapon_layout.rs"]
mod geometry;
const MEMBER: &str = "published/layoutdata/layouts.xml.sb";
const SOURCE: &str = "11a7e8d5f82071a0132baf0675fffcf113d0711c52fb0a45aa8eeddef9a0b0b9";
const WORLD_101_LAYOUT: &str = "2df8d4afe778ebdccfbebb68fbab04991f1a0a77683cd90e7cf45b6bbd9938b3";
static PATCH: OnceLock<Box<[u8]>> = OnceLock::new();
static WORLD_TUTORIAL_PATCH: OnceLock<Box<[u8]>> = OnceLock::new();

pub(super) fn mount(bundle: &mut FsNode, bundle_id: &str) {
    if bundle_id != "com.ea.deadspace.bv" { return; }
    let mut node = bundle;
    for part in MEMBER.split('/') {
        let FsNode::Directory { children, .. } = node else {return;};
        let Some(child) = children.get_mut(part) else {return;}; node = child;
    }
    let FsNode::File { location, writeable: false } = node else {return;};
    let original = match location {
        FileLocation::Path(path) => std::fs::read(path),
        FileLocation::IpaFileRef(file) => { let mut bytes = Vec::new(); file.open().read_to_end(&mut bytes).map(|_| bytes) },
        FileLocation::EmbeddedFile(file) => Ok(file.0.to_vec()),
        FileLocation::ResourceFilePath(_) => return,
    };
    let Ok(original) = original else {return;};
    let source_hash = format!("{:x}", Sha256::digest(&original));
    let (patch, cache) = match source_hash.as_str() {
        SOURCE => (geometry::patch(&original), &PATCH),
        WORLD_101_LAYOUT => (geometry::patch_world_tutorial(&original), &WORLD_TUTORIAL_PATCH),
        _ => {
            log!("[DSweapon-layout] unknown layout resource; native layout preserved");
            return;
        }
    };
    let Some(patched) = patch else {
        log!("[DSweapon-layout] resource validation failed; native layout preserved");
        return;
    };
    let bytes = cache.get_or_init(|| patched.into_boxed_slice());
    *location = FileLocation::EmbeddedFile(EmbeddedFile(bytes));
    if source_hash == WORLD_101_LAYOUT {
        log!("[DStutorial-layout] mounted World 1.0.1 extended tutorial border source_sha256={WORLD_101_LAYOUT} bytes={}", bytes.len());
    } else {
        log!("[DSweapon-layout] mounted Windows story+survival vertical order=plasma,ripper,linegun,corer,burst source_sha256={SOURCE} bytes={}; original IPA unchanged", bytes.len());
    }
}

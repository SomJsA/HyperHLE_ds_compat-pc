//! Windows-only, fingerprint-checked native HUD resource mount.
use super::{EmbeddedFile, FileLocation, FsNode};
use sha2::{Digest, Sha256};
use std::{io::Read, sync::OnceLock};
#[path = "ds_weapon_layout.rs"]
mod geometry;
const MEMBER: &str = "published/layoutdata/layouts.xml.sb";
const SOURCE: &str = "11a7e8d5f82071a0132baf0675fffcf113d0711c52fb0a45aa8eeddef9a0b0b9";
static PATCH: OnceLock<Box<[u8]>> = OnceLock::new();

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
    if format!("{:x}", Sha256::digest(&original)) != SOURCE {
        log!("[DSweapon-layout] unknown layout resource; native layout preserved"); return;
    }
    let Some(patched) = geometry::patch(&original) else {
        log!("[DSweapon-layout] resource validation failed; native layout preserved"); return;
    };
    let bytes = PATCH.get_or_init(|| patched.into_boxed_slice());
    *location = FileLocation::EmbeddedFile(EmbeddedFile(bytes));
    log!("[DSweapon-layout] mounted Windows story+survival vertical order=plasma,ripper,linegun,corer,burst source_sha256={SOURCE} bytes={}; original IPA unchanged", bytes.len());
}

/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Mount the native Chinese string patch without changing the selected IPA.

use super::{EmbeddedFile, FileLocation, FsNode};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;

const BUNDLE_ID: &str = "com.ea.deadspace.bv";
const MEMBER: &str = "published/stringdata/CHS_CN/deadspace_iphone.bin";
const PATCH: &[u8] = include_bytes!("../../res/overlay/deadspace_iphone.bin");
const ORIGINAL: &str = "47864a0c746cf439b9bab93e66aea621ae3896f2e517f710436b6302b0a3db77";
// Historical translation baseline, previously labelled "original" by the IPA tool.
const LEGACY_BASE: &str = "4732db2574300a5407b783c72e0caa6002e961441379cbb86ef5780fa06d36f0";
const PREVIOUS: &str = "e779145bb9c2415d23630bd844c141e7c208090e01f4a8e24193eecff6a2db9c";
const PREVIOUS_R15: &str = "71f989b9cea4ae2dedb58b4e66279513ac06f077eb30e65ef7bfcdcfeada5bce";
const JAPANESE_MEMBER: &str = "published/stringdata/JPN_JP/deadspace_iphone.bin";
const ENGLISH_MEMBER: &str = "published/stringdata/ENG_US/deadspace_iphone.bin";

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn mount(bundle: &mut FsNode, bundle_id: &str) {
    mount_subtitles(bundle,bundle_id,std::env::var("TOUCHHLE_DS_SUBTITLE_SUPPLEMENT").as_deref()!=Ok("0"));
    mount_language_fallback(bundle, bundle_id, std::env::var("TOUCHHLE_DS_NATIVE_TEXT_FALLBACK").as_deref() == Ok("1"));
    let enabled = std::env::var("TOUCHHLE_DS_NATIVE_TEXT_PATCH").as_deref() != Ok("0");
    let mounted = mount_enabled(bundle, bundle_id, enabled);
    if bundle_id == BUNDLE_ID { crate::ds_startup_report::text_patch(mounted); }
}

fn read_member(bundle: &FsNode, member: &str) -> Option<Vec<u8>> {
    let mut node = bundle;
    for part in member.split('/') {
        let FsNode::Directory { children, .. } = node else { return None; };
        node = children.get(part)?;
    }
    let FsNode::File { location, .. } = node else { return None; };
    match location {
        FileLocation::Path(path) => std::fs::read(path).ok(),
        FileLocation::IpaFileRef(file) => {
            let mut bytes = Vec::new(); file.open().read_to_end(&mut bytes).ok().map(|_| bytes)
        }
        FileLocation::EmbeddedFile(file) => Some(file.0.to_vec()),
        FileLocation::ResourceFilePath(_) => None,
    }
}

fn replace_member(bundle: &mut FsNode, member: &str, bytes: &'static [u8]) -> bool {
    let mut node = bundle;
    for part in member.split('/') {
        let FsNode::Directory { children, .. } = node else { return false; };
        let Some(child) = children.get_mut(part) else { return false; };
        node = child;
    }
    let FsNode::File { location, writeable: false } = node else { return false; };
    *location = FileLocation::EmbeddedFile(EmbeddedFile(bytes));
    true
}

fn fnv1(bytes: &[u8]) -> u32 {
    let mut value = 0x811c9dc5u32;
    for byte in bytes { value = value.wrapping_mul(0x01000193) ^ u32::from(*byte); }
    value
}

fn parse_sections(raw: &[u8]) -> Option<Vec<([u8; 4], Vec<u8>)>> {
    if raw.get(..8) != Some(b"SBIN\x02\0\0\0") { return None; }
    let mut cursor = 8usize; let mut sections = Vec::new();
    while cursor < raw.len() {
        if cursor.checked_add(12)? > raw.len() { return None; }
        let tag = raw.get(cursor..cursor + 4)?.try_into().ok()?;
        let length = u32::from_le_bytes(raw[cursor + 4..cursor + 8].try_into().ok()?) as usize;
        let end = cursor.checked_add(12)?.checked_add(length)?;
        let aligned = (end + 3) & !3;
        if aligned > raw.len() || fnv1(&raw[cursor + 12..end]) != u32::from_le_bytes(raw[cursor + 8..cursor + 12].try_into().ok()?) { return None; }
        sections.push((tag, raw[cursor + 12..end].to_vec())); cursor = aligned;
    }
    Some(sections)
}

fn pack_strings(strings: &[Vec<u8>]) -> Option<Vec<u8>> {
    let mut result = Vec::new(); result.extend_from_slice(&(strings.len() as u32).to_le_bytes());
    for value in strings {
        if value.len() > u32::MAX as usize { return None; }
        result.extend_from_slice(&(value.len() as u32).to_le_bytes()); result.extend_from_slice(value);
    }
    Some(result)
}

fn unpack_strings(payload: &[u8]) -> Option<Vec<Vec<u8>>> {
    if payload.len() < 4 { return None; }
    let count = u32::from_le_bytes(payload[..4].try_into().ok()?) as usize; let mut cursor = 4usize; let mut strings = Vec::with_capacity(count);
    for _ in 0..count {
        if cursor.checked_add(4)? > payload.len() { return None; }
        let length = u32::from_le_bytes(payload[cursor..cursor + 4].try_into().ok()?) as usize; cursor += 4;
        let end = cursor.checked_add(length)?; if end > payload.len() { return None; }
        strings.push(payload[cursor..end].to_vec()); cursor = end;
    }
    (cursor == payload.len()).then_some(strings)
}

fn section_payload<'a>(sections: &'a [([u8; 4], Vec<u8>)], tag: &[u8; 4]) -> Option<&'a [u8]> {
    sections.iter().find(|(name, _)| name == tag).map(|(_, payload)| payload.as_slice())
}

fn merge_language_banks(japanese: &[u8], english: &[u8]) -> Option<Vec<u8>> {
    let japanese_sections = parse_sections(japanese)?; let english_sections = parse_sections(english)?;
    let japanese_strings = unpack_strings(section_payload(&japanese_sections, b"STRS")?)?;
    let english_strings = unpack_strings(section_payload(&english_sections, b"STRS")?)?;
    let japanese_ohdr = section_payload(&japanese_sections, b"OHDR")?; let english_ohdr = section_payload(&english_sections, b"OHDR")?;
    if japanese_ohdr.len() != 12 || english_ohdr.len() != 12 { return None; }
    let japanese_data = section_payload(&japanese_sections, b"DATA")?; let english_data = section_payload(&english_sections, b"DATA")?;
    let japanese_list = u32::from_le_bytes(japanese_ohdr[8..12].try_into().ok()?) as usize >> 3;
    let english_list = u32::from_le_bytes(english_ohdr[8..12].try_into().ok()?) as usize >> 3;
    if japanese_list + 8 > japanese_data.len() || english_list + 8 > english_data.len() { return None; }
    let japanese_kind = u32::from_le_bytes(japanese_data[japanese_list..japanese_list + 4].try_into().ok()?);
    let english_kind = u32::from_le_bytes(english_data[english_list..english_list + 4].try_into().ok()?);
    let japanese_count = u32::from_le_bytes(japanese_data[japanese_list + 4..japanese_list + 8].try_into().ok()?) as usize;
    let english_count = u32::from_le_bytes(english_data[english_list + 4..english_list + 8].try_into().ok()?) as usize;
    if japanese_kind != 16 || english_kind != 16 || japanese_list + 8 + japanese_count * 8 != japanese_data.len() || english_list + 8 + english_count * 8 != english_data.len() { return None; }

    let mut strings = japanese_strings.clone(); let mut string_ids = HashMap::<Vec<u8>, u16>::new();
    for (index, value) in strings.iter().enumerate() { string_ids.insert(value.clone(), (index + 1).try_into().ok()?); }
    let mut records = japanese_data[japanese_list + 8..].to_vec(); let mut names = HashMap::<Vec<u8>, ()>::new();
    for index in 0..japanese_count {
        let offset = index * 8; let name_id = u16::from_le_bytes(records[offset..offset + 2].try_into().ok()?);
        if name_id == 0 || name_id as usize > strings.len() { return None; } names.insert(strings[name_id as usize - 1].clone(), ());
    }
    for index in 0..english_count {
        let offset = english_list + 8 + index * 8;
        let name_id = u16::from_le_bytes(english_data[offset..offset + 2].try_into().ok()?);
        let value_id = u16::from_le_bytes(english_data[offset + 2..offset + 4].try_into().ok()?);
        if name_id == 0 || value_id == 0 || name_id as usize > english_strings.len() || value_id as usize > english_strings.len() { return None; }
        let name = english_strings[name_id as usize - 1].clone(); if names.contains_key(&name) { continue; }
        let value = english_strings[value_id as usize - 1].clone();
        let merged_name = if let Some(id) = string_ids.get(&name) { *id } else { strings.push(name.clone()); let id: u16 = strings.len().try_into().ok()?; string_ids.insert(name.clone(), id); id };
        let merged_value = if let Some(id) = string_ids.get(&value) { *id } else { strings.push(value.clone()); let id: u16 = strings.len().try_into().ok()?; string_ids.insert(value.clone(), id); id };
        records.extend_from_slice(&merged_name.to_le_bytes()); records.extend_from_slice(&merged_value.to_le_bytes()); records.extend_from_slice(&english_data[offset + 4..offset + 8]); names.insert(name, ());
    }
    let new_count = names.len(); let mut data = japanese_data.to_vec(); data[japanese_list + 4..japanese_list + 8].copy_from_slice(&(new_count as u32).to_le_bytes()); data.extend_from_slice(&records[japanese_count * 8..]);
    let new_strings = pack_strings(&strings)?; let mut output = Vec::new(); output.extend_from_slice(&japanese[..8]);
    for (tag, original) in japanese_sections {
        let payload: &[u8] = if &tag == b"DATA" { data.as_slice() } else if &tag == b"STRS" { new_strings.as_slice() } else { original.as_slice() };
        output.extend_from_slice(&tag); output.extend_from_slice(&(payload.len() as u32).to_le_bytes()); output.extend_from_slice(&fnv1(payload).to_le_bytes()); output.extend_from_slice(payload); while output.len() % 4 != 0 { output.push(0); }
    }
    Some(output)
}

fn mount_language_fallback(bundle: &mut FsNode, bundle_id: &str, enabled: bool) -> bool {
    if bundle_id != BUNDLE_ID || !enabled { return false; }
    let Some(japanese) = read_member(bundle, JAPANESE_MEMBER) else { return false; };
    let Some(english) = read_member(bundle, ENGLISH_MEMBER) else { return false; };
    let Some(merged) = merge_language_banks(&japanese, &english) else { log!("[DSlanguage-fallback] unable to merge Japanese and English banks; original retained"); return false; };
    if merged.len() > 2 * 1024 * 1024 { return false; }
    let bytes: &'static [u8] = Box::leak(merged.into_boxed_slice());
    let mounted = replace_member(bundle, JAPANESE_MEMBER, bytes);
    if mounted { log!("[DSlanguage-fallback] mounted Japanese-first English fallback bank bytes={} original_japanese={} english={}", bytes.len(), japanese.len(), english.len()); }
    mounted
}

fn mount_enabled(bundle: &mut FsNode, bundle_id: &str, enabled: bool) -> bool {
    if bundle_id != BUNDLE_ID {
        return false;
    }
    if !enabled {
        log!("[DSresource-overlay] native text patch disabled; using selected bundle unchanged");
        return false;
    }
    let mut node = bundle;
    for part in MEMBER.split('/') {
        let FsNode::Directory { children, .. } = node else {
            return false;
        };
        let Some(child) = children.get_mut(part) else {
            return false;
        };
        node = child;
    }
    let FsNode::File {
        location,
        writeable: false,
    } = node
    else {
        return false;
    };
    // Only inspect the one existing Chinese member. Other files, languages and
    // the user's sandbox keep their original nodes and access rules.
    let source = match location {
        FileLocation::Path(path) => std::fs::read(path),
        FileLocation::IpaFileRef(file) => {
            let mut bytes = Vec::new();
            file.open().read_to_end(&mut bytes).map(|_| bytes)
        }
        FileLocation::EmbeddedFile(file) => Ok(file.0.to_vec()),
        FileLocation::ResourceFilePath(_) => return false,
    };
    let source = match source {
        Ok(bytes) => bytes,
        Err(error) => {
            log!("[DSresource-overlay] cannot read {MEMBER}: {error}; keeping original node");
            return false;
        }
    };
    let source_hash = digest(&source);
    let patch_hash = digest(PATCH);
    if source_hash != ORIGINAL
        && source_hash != LEGACY_BASE
        && source_hash != PREVIOUS
        && source_hash != PREVIOUS_R15
        && source_hash != patch_hash
    {
        log!("[DSresource-overlay] unsupported source_sha256={source_hash}; keeping original {MEMBER}");
        return false;
    }
    *location = FileLocation::EmbeddedFile(EmbeddedFile(PATCH));
    crate::ds_diagnostics::feature("native_text_source_sha256",&source_hash);
    crate::ds_diagnostics::feature("native_text_patch_sha256",&patch_hash);
    log!("[DSresource-overlay] mounted {MEMBER} bytes={} source_sha256={source_hash} patch_sha256={patch_hash}; selected IPA unchanged", PATCH.len());
    true
}


const SUBTITLE_MEMBER:&str="published/data/subtitles.sb";
const SUBTITLE_ORIGINAL:&str="8f4e219455148d0acafec5e632f2a762f0d1e60fe375acff787059125bd1d143";
const SUBTITLE_PATCH:&[u8]=include_bytes!("../../res/overlay/deadspace_subtitles.sb");

fn mount_subtitles(bundle:&mut FsNode,bundle_id:&str,enabled:bool)->bool {
    if bundle_id != BUNDLE_ID || !enabled { return false; }
    let mut node=bundle;
    for part in SUBTITLE_MEMBER.split('/') {
        let FsNode::Directory {children,..}=node else {return false;};
        let Some(next)=children.get_mut(part) else {return false;};node=next;
    }
    let FsNode::File {location,writeable:false}=node else {return false;};
    let bytes=match location {
        FileLocation::Path(path)=>std::fs::read(path),
        FileLocation::IpaFileRef(file)=>{let mut data=Vec::new();file.open().read_to_end(&mut data).map(|_|data)},
        FileLocation::EmbeddedFile(file)=>Ok(file.0.to_vec()),
        FileLocation::ResourceFilePath(_)=>return false,
    };
    let Ok(bytes)=bytes else {return false;};
    let source=digest(&bytes);let patch=digest(SUBTITLE_PATCH);
    if source != SUBTITLE_ORIGINAL && source != patch {
        log!("[DSsubtitle-bank] unsupported source_sha256={source}; original table retained");
        return false;
    }
    *location=FileLocation::EmbeddedFile(EmbeddedFile(SUBTITLE_PATCH));
    log!("[DSsubtitle-bank] mounted read-only bytes={} source_sha256={} patch_sha256={}; selected IPA unchanged",SUBTITLE_PATCH.len(),source,patch);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{BundleData, Fs, GuestOpenOptions, GuestPath, GuestPathBuf};
    use std::io::{Seek, SeekFrom, Write};

    fn test_bank(strings: &[&[u8]], records: &[(u16, u16, u32)]) -> Vec<u8> {
        let mut data = Vec::new(); data.extend_from_slice(&16u32.to_le_bytes()); data.extend_from_slice(&(records.len() as u32).to_le_bytes());
        for (name, value, category) in records { data.extend_from_slice(&name.to_le_bytes()); data.extend_from_slice(&value.to_le_bytes()); data.extend_from_slice(&category.to_le_bytes()); }
        let string_bytes = pack_strings(&strings.iter().map(|value| value.to_vec()).collect::<Vec<_>>()).unwrap();
        let sections = vec![(b"OHDR".to_vec(), vec![0u8; 12]), (b"DATA".to_vec(), data), (b"STRS".to_vec(), string_bytes)];
        let mut raw = b"SBIN\x02\0\0\0".to_vec();
        for (tag, payload) in sections { raw.extend_from_slice(&tag); raw.extend_from_slice(&(payload.len() as u32).to_le_bytes()); raw.extend_from_slice(&fnv1(&payload).to_le_bytes()); raw.extend_from_slice(&payload); while raw.len() % 4 != 0 { raw.push(0); } }
        raw
    }

    #[test]
    fn r6_language_bank_merges_missing_english_records_without_replacing_existing() {
        let japanese = test_bank(&[b"jkey", b"japanese"], &[(1, 2, 7)]);
        let english = test_bank(&[b"jkey", b"english", b"ekey", b"fallback"], &[(1, 2, 7), (3, 4, 9)]);
        let merged = merge_language_banks(&japanese, &english).expect("merge");
        let sections = parse_sections(&merged).expect("valid merged SBIN");
        let strings = unpack_strings(section_payload(&sections, b"STRS").unwrap()).unwrap();
        assert_eq!(strings, vec![b"jkey".to_vec(), b"japanese".to_vec(), b"ekey".to_vec(), b"fallback".to_vec()]);
        let data = section_payload(&sections, b"DATA").unwrap(); assert_eq!(u32::from_le_bytes(data[4..8].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(data[16..18].try_into().unwrap()), 3);
        assert_eq!(u16::from_le_bytes(data[18..20].try_into().unwrap()), 4);
    }

    fn bundle(bytes: &'static [u8]) -> FsNode {
        FsNode::dir()
            .with_child(
                "published",
                FsNode::dir().with_child(
                    "stringdata",
                    FsNode::dir()
                        .with_child(
                            "CHS_CN",
                            FsNode::dir().with_child(
                                "deadspace_iphone.bin",
                                FsNode::File {
                                    location: FileLocation::EmbeddedFile(EmbeddedFile(bytes)),
                                    writeable: false,
                                },
                            ),
                        )
                        .with_child(
                            "EN_US",
                            FsNode::dir().with_child(
                                "deadspace_iphone.bin",
                                FsNode::File {
                                    location: FileLocation::EmbeddedFile(EmbeddedFile(
                                        b"other language",
                                    )),
                                    writeable: false,
                                },
                            ),
                        ),
                ),
            )
            .with_child(
                "untouched",
                FsNode::File {
                    location: FileLocation::EmbeddedFile(EmbeddedFile(b"untouched")),
                    writeable: false,
                },
            )
    }

    fn fs(root: FsNode) -> Fs {
        Fs {
            root,
            working_directory: GuestPathBuf::from("/".to_owned()),
            home_directory: GuestPathBuf::from("/".to_owned()),
        }
    }

    fn check_reads(mut fs: Fs) {
        let path = GuestPath::new(MEMBER);
        assert_eq!(fs.size(path).unwrap(), PATCH.len() as u64);
        assert_eq!(fs.access(path), (true, true, false, false));
        assert_eq!(fs.read(path).unwrap(), PATCH);
        assert!(fs
            .enumerate_recursive("/")
            .unwrap()
            .iter()
            .any(|p| p.as_str().ends_with(MEMBER)));
        let mut options = GuestOpenOptions::new();
        options.read();
        let mut file = fs.open_with_options(path, options).unwrap();
        file.seek(SeekFrom::Start(13)).unwrap();
        assert_eq!(file.stream_len().unwrap(), PATCH.len() as u64);
        assert_eq!(file.stream_position().unwrap(), 13);
        let mut cloned = file.try_clone().unwrap();
        let mut bytes = [0; 19];
        cloned.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, PATCH[13..32]);
        assert_eq!(file.stream_position().unwrap(), 13);
        assert_eq!(
            file.seek(SeekFrom::End(-8)).unwrap(),
            (PATCH.len() - 8) as u64
        );
        let mut tail = Vec::new();
        file.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, PATCH[PATCH.len() - 8..]);
        assert_eq!(file.read(&mut bytes).unwrap(), 0);
        assert_eq!(
            file.write(b"no").unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            file.set_len(0).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(fs.write(path, b"no").is_err());
        assert!(fs.remove(path).is_err());
        assert!(fs.rename(path, GuestPath::new("moved.bin")).is_err());
        for mode in 0..4 {
            let mut options = GuestOpenOptions::new();
            match mode {
                0 => {
                    options.write();
                }
                1 => {
                    options.append();
                }
                2 => {
                    options.truncate();
                }
                _ => {
                    options.create();
                }
            }
            assert!(fs.open_with_options(path, options).is_err());
        }
        assert_eq!(fs.read(path).unwrap(), PATCH);
    }

    #[test]
    fn ds_native_patch_vfs_read_seek_length_clone_and_readonly() {
        let mut root = bundle(PATCH);
        assert!(mount_enabled(&mut root, BUNDLE_ID, true));
        let fs = fs(root);
        assert_eq!(
            fs.read("published/stringdata/EN_US/deadspace_iphone.bin")
                .unwrap(),
            b"other language"
        );
        assert_eq!(fs.read("untouched").unwrap(), b"untouched");
        check_reads(fs);
    }

    #[test]
    fn ds_native_patch_unknown_disabled_unrelated_and_missing_keep_original() {
        for (id, enabled, bytes) in [
            (BUNDLE_ID, true, b"unknown".as_slice()),
            (BUNDLE_ID, false, PATCH),
            ("other.app", true, PATCH),
        ] {
            let mut root = bundle(bytes);
            assert!(!mount_enabled(&mut root, id, enabled));
            assert_eq!(fs(root).read(MEMBER).unwrap(), bytes);
        }
        let mut root = FsNode::dir();
        assert!(!mount_enabled(&mut root, BUNDLE_ID, true));
        assert!(fs(root).enumerate_recursive("/").unwrap().is_empty());
    }

    #[test]
    // R2 bank (including reviewed text fixes); its accepted hash is also checked by project-guard.
    fn ds_native_patch_embedded_resource_matches_latest_ledger() {
        assert_eq!(
            digest(PATCH),
            "05713dc09dc6988c60a1d4e3f431c96c7d108f6c3d0cd7be278a3da3b277b81a"
        );
        assert_eq!(&PATCH[..8], b"SBIN\x02\0\0\0");
    }

    #[test]
    #[ignore = "requires TOUCHHLE_TEST_ORIGINAL_IPA pointing to the user's immutable original IPA"]
    fn ds_native_patch_original_ipa_read_only_integration() {
        let path =
            std::env::var_os("TOUCHHLE_TEST_ORIGINAL_IPA").expect("original IPA path required");
        let path = std::path::Path::new(&path);
        let before = file_digest(path);
        let mut filesystem = fs(BundleData::open_ipa(path).unwrap().into_fs_node());
        assert_eq!(digest(&filesystem.read(MEMBER).unwrap()), ORIGINAL);
        assert!(mount_enabled(&mut filesystem.root, BUNDLE_ID, true));
        check_reads(filesystem);
        assert_eq!(before, file_digest(path));
    }

    fn file_digest(path: &std::path::Path) -> String {
        let mut file = std::fs::File::open(path).unwrap();
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let count = file.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        format!("{hash:x}", hash = hash.finalize())
    }
    #[test]
    fn r4_subtitle_mount_is_readonly_idempotent_and_rejects_unknown_nodes() {
        fn make(bytes:&'static [u8],writeable:bool)->FsNode {
            FsNode::dir().with_child("published",FsNode::dir().with_child("data",
                FsNode::dir().with_child("subtitles.sb",FsNode::File {
                    location:FileLocation::EmbeddedFile(EmbeddedFile(bytes)),writeable})))
        }
        let mut known=make(SUBTITLE_PATCH,false);
        assert!(mount_subtitles(&mut known,BUNDLE_ID,true));
        assert!(mount_subtitles(&mut known,BUNDLE_ID,true));
        assert!(!mount_subtitles(&mut known,BUNDLE_ID,false));
        assert!(!mount_subtitles(&mut known,"other.application",true));
        assert!(!mount_subtitles(&mut make(b"unknown",false),BUNDLE_ID,true));
        assert!(!mount_subtitles(&mut make(SUBTITLE_PATCH,true),BUNDLE_ID,true));
        assert!(!mount_subtitles(&mut FsNode::dir(),BUNDLE_ID,true));
    }

}

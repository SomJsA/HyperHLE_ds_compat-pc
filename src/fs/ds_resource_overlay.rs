/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Mount the native Chinese string patch without changing the selected IPA.

use super::{EmbeddedFile, FileLocation, FsNode};
use sha2::{Digest, Sha256};
use std::io::Read;

const BUNDLE_ID: &str = "com.ea.deadspace.bv";
const MEMBER: &str = "published/stringdata/CHS_CN/deadspace_iphone.bin";
const PATCH: &[u8] = include_bytes!("../../res/overlay/deadspace_iphone.bin");
const ORIGINAL: &str = "47864a0c746cf439b9bab93e66aea621ae3896f2e517f710436b6302b0a3db77";
// Historical translation baseline, previously labelled "original" by the IPA tool.
const LEGACY_BASE: &str = "4732db2574300a5407b783c72e0caa6002e961441379cbb86ef5780fa06d36f0";
const PREVIOUS: &str = "e779145bb9c2415d23630bd844c141e7c208090e01f4a8e24193eecff6a2db9c";
const PREVIOUS_R15: &str = "71f989b9cea4ae2dedb58b4e66279513ac06f077eb30e65ef7bfcdcfeada5bce";

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn mount(bundle: &mut FsNode, bundle_id: &str) {
    mount_subtitles(bundle,bundle_id,std::env::var("TOUCHHLE_DS_SUBTITLE_SUPPLEMENT").as_deref()!=Ok("0"));
    let enabled = std::env::var("TOUCHHLE_DS_NATIVE_TEXT_PATCH").as_deref() != Ok("0");
    let mounted = mount_enabled(bundle, bundle_id, enabled);
    if bundle_id == BUNDLE_ID { crate::ds_startup_report::text_patch(mounted); }
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

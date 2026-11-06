/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! Dead Space 1.3.34 ending: hide both helmet and separate emissive submesh.
//! The phone path finds only the first submesh with the helmet name; the
//! existing iPad2 path finds each by appearance AND name and hides all eight
//! helmet/glow entries. Reuse that cleanup without changing the device tier.

pub(super) const SYMBOL: &str = "__ZN18GameObjectPlayable18SwitchToCarrieHeadEv";
pub(super) const ENTRY: u32 = 0x0003_d948;
pub(super) const PATCH: u32 = 0x0003_d974;
pub(super) const ARM_NOP: u32 = 0xe1a0_0000; // mov r0, r0

// Guard the entry, branch and full-cleanup calls before changing one word.
const EXPECTED: &[(u32, u32)] = &[
    (ENTRY, 0xe92d_40f0),
    (0x0003_d96c, 0xebff_2379), // bl Application::IsTierIPad2
    (0x0003_d970, 0xe350_0000), // cmp r0, #0
    (PATCH, 0x0a00_0098),       // beq phone-only first-match cleanup
    (0x0003_d9f0, 0xeb02_236c), // bl Model::FindSubmesh(appearance, name)
    (0x0003_da50, 0xeb10_1218), // bl Submesh::SetVisible(false)
    (0x0003_da5c, 0xe355_000c), // 12 table entries
    (0x0003_da68, 0xe300_0faa), // helmet/glow mask: 1,3,5,7,8,9,10,11
];

pub(super) fn matches(entry: Option<u32>, mut read: impl FnMut(u32) -> u32) -> bool {
    entry == Some(ENTRY) && EXPECTED.iter().all(|&(addr, word)| read(addr) == word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_original_code_is_required() {
        let original = |addr| EXPECTED.iter().find(|p| p.0 == addr).unwrap().1;
        assert!(matches(Some(ENTRY), original));
        for &(changed, _) in EXPECTED {
            assert!(!matches(Some(ENTRY), |addr| original(addr)
                ^ u32::from(addr == changed)));
        }
        assert!(!matches(Some(ENTRY), |addr| if addr == PATCH {
            ARM_NOP
        } else {
            original(addr)
        }));
    }

    #[test]
    fn unknown_or_thumb_entry_never_reads_fixed_addresses() {
        for entry in [None, Some(ENTRY + 1), Some(ENTRY + 4)] {
            assert!(!matches(entry, |_| panic!(
                "must not read an unknown binary"
            )));
        }
    }

    #[test]
    fn cleanup_mask_covers_all_helmets_and_glows_without_body_meshes() {
        let selected: Vec<_> = (0..12).filter(|i| 0xfaa & (1 << i) != 0).collect();
        assert_eq!(selected, [1, 3, 5, 7, 8, 9, 10, 11]);
    }
}

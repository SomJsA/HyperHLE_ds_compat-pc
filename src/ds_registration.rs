//! Suppress the original registration question at its verified menu callers.
use crate::bundle::Bundle;
use crate::ds_legacy_input_profile::LegacyInputProfile;
use crate::mach_o::MachO;
use crate::mem::{ConstVoidPtr, Mem, MutPtr};

const CURRENT: [u8; 32] = [
    0x47, 0x32, 0xdb, 0x25, 0x74, 0x30, 0x0a, 0x54, 0x07, 0xb7, 0x83, 0xc7, 0x2e, 0x0c, 0xaa, 0x60,
    0x02, 0xe9, 0x61, 0x44, 0x13, 0x79, 0xcb, 0xb8, 0x6e, 0xf5, 0x78, 0x0f, 0xa0, 0x6d, 0x36, 0xf0,
];
const IPAD: [u8; 32] = [
    0xdf, 0xaf, 0x97, 0x9d, 0x07, 0x26, 0xde, 0x92, 0x5d, 0x80, 0x3c, 0x4b, 0xaa, 0x91, 0x08, 0xac,
    0xf7, 0xfd, 0xbf, 0xae, 0x22, 0x16, 0x18, 0xb1, 0x1e, 0xc1, 0xb3, 0x15, 0xe9, 0xc9, 0x30, 0x5b,
];
const IPAD_WORLD: [u8; 32] = [
    0x4b, 0xe1, 0xa4, 0x81, 0xca, 0x91, 0x27, 0xdd, 0x11, 0x8a, 0x70, 0x88, 0xb2, 0x81, 0x0f, 0xb1,
    0xfd, 0x86, 0xe6, 0x92, 0x10, 0xd9, 0x52, 0xf2, 0x48, 0x33, 0x5b, 0x9a, 0x07, 0xb7, 0x97, 0xa6,
];

const CMP_R0_ZERO: u32 = 0xe350_0000;
const MOV_R0_ZERO: u32 = 0xe3a0_0000;

fn sites(bundle: &str, version: &str, digest: [u8; 32]) -> Option<[(u32, u32, u32); 3]> {
    let sites = match (bundle, version, digest) {
        ("com.ea.deadspace.bv", "1.3.34", CURRENT) => [
            (0x001b_a868, 0xebfe_f489, CMP_R0_ZERO),
            (0x001b_c540, 0xebfe_ed53, CMP_R0_ZERO),
            (0x001c_9df4, 0xebfe_b726, CMP_R0_ZERO),
        ],
        ("com.ea.deadspace.ipad.bv", "1.3.11", IPAD | IPAD_WORLD) => [
            (0x0019_21ec, 0xebff_11c6, CMP_R0_ZERO),
            (0x0019_3df4, 0xebff_0ac4, CMP_R0_ZERO),
            (0x001a_14fc, 0xebfe_d502, CMP_R0_ZERO),
        ],
        _ => match LegacyInputProfile::identify(bundle, version, Some(digest))? {
            LegacyInputProfile::EarlyIpad => [
                (0x0021_70d8, 0xebfe_dc68, 0xe250_4000),
                (0x0021_863c, 0xebfe_d70f, CMP_R0_ZERO),
                (0x0021_9ff8, 0xebfe_d0a0, CMP_R0_ZERO),
            ],
            LegacyInputProfile::Iphone => [
                (0x0019_c224, 0xebff_0940, CMP_R0_ZERO),
                (0x0019_de2c, 0xebff_023e, CMP_R0_ZERO),
                (0x001a_b534, 0xebfe_cc7c, CMP_R0_ZERO),
            ],
            LegacyInputProfile::World => [
                (0x0020_8508, 0xebff_08a0, CMP_R0_ZERO),
                (0x0020_8ee4, 0xebff_0629, CMP_R0_ZERO),
                (0x0020_9354, 0xebff_050d, CMP_R0_ZERO),
            ],
        },
    };
    Some(sites)
}

pub(crate) fn suppress_startup_question(bundle: &Bundle, executable: &MachO, mem: &mut Mem) {
    let (id, version) = (bundle.bundle_identifier(), bundle.bundle_version());
    let Some(sites) = executable
        .executable_sha256
        .and_then(|digest| sites(id, version, digest))
    else {
        return;
    };

    // Apply before the first guest instruction runs. The read-only getter has
    // three native menu callers in each image. Supply false at those sites so
    // the original menu state machine takes its regular no-question branch.
    let matches = sites.iter().all(|&(address, call, comparison)| {
        let bytes = mem.get_bytes_fallible(ConstVoidPtr::from_bits(address), 8);
        bytes.is_some_and(|bytes| {
            bytes[..4] == call.to_le_bytes() && bytes[4..] == comparison.to_le_bytes()
        })
    });
    if !matches {
        echo!("[DSregistration] original call sites did not match");
        return;
    }

    for (address, _, _) in sites {
        mem.write(MutPtr::<u32>::from_bits(address), MOV_R0_ZERO);
    }
    echo!("[DSregistration] startup question suppressed for {id} build {version}");
}

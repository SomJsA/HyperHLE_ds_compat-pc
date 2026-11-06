/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! StoreKit

mod sk_payment_queue;
mod sk_product;

use crate::dyld::{ConstantExports, HostConstant};

/// Constants used by the StoreKit framework.
///
/// These are NSString-typed `extern const` symbols. Apps that link against
/// StoreKit on iOS 6+ pull them in via Mach-O symbol lookup (e.g. for use
/// as `SKStoreProductViewController` parameter dictionary keys); without
/// these stubs the linker leaves the slots NULL, so any guest-side
/// dereference (CFString equality check, `[dict objectForKey:nil]`, etc.)
/// crashes with a NULL-page read.
pub const CONSTANTS: ConstantExports = &[
    (
        "_SKStoreProductParameterITunesItemIdentifier",
        HostConstant::NSString("itemIdentifier"),
    ),
    (
        "_SKStoreProductParameterAffiliateToken",
        HostConstant::NSString("affiliateToken"),
    ),
    (
        "_SKStoreProductParameterCampaignToken",
        HostConstant::NSString("campaignToken"),
    ),
    (
        "_SKStoreProductParameterProviderToken",
        HostConstant::NSString("providerToken"),
    ),
    (
        "_SKStoreProductParameterAdvertisingPartnerToken",
        HostConstant::NSString("advertisingPartnerToken"),
    ),
    (
        "_SKErrorDomain",
        HostConstant::NSString("SKErrorDomain"),
    ),
];

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    path: "/System/Library/Frameworks/StoreKit.framework/StoreKit",
    aliases: &[],
    class_exports: &[sk_payment_queue::CLASSES, sk_product::CLASSES],
    constant_exports: &[CONSTANTS],
    function_exports: &[],
};

#[derive(Default)]
pub struct State {
    pub payment_queue: sk_payment_queue::State,
}

use crate::abi::{CallFromHost, GuestFunction};
use crate::mem::MutPtr;
use crate::Environment;

/// Debug-only guest-to-guest recovery path for the three delisted Dead Space
/// modules. The app's own `Settings::UnlockModule` + `Settings::Save` writers
/// are invoked so the resulting `settings.sb` uses the game's real container
/// layout instead of a hand-written approximation.
///
/// Enabled only with `TOUCHHLE_DS_UNLOCK_MTX=1`; waits until the game has
/// finished booting (settings singleton non-null) before doing anything.
fn make_wstring_from_wide(env: &mut Environment, source_va: u32, len: u32) -> u32 {
    use crate::mem::Ptr;

    // EASTL basic_string layout used by this app:
    //   +0x00 begin, +0x04 end, +0x08 capacity, +0x0c allocator[8]
    // The allocator object itself is constructed by
    // im::StringEASTLAllocator::StringEASTLAllocator(char const*).
    let allocator: MutPtr<u8> = env.mem.alloc(8).cast();
    let allocator_ctor = GuestFunction::from_addr_with_thumb_bit(0x37749c);
    let _: () = allocator_ctor.call_from_host(env, (allocator.to_bits(), 0u32));

    let allocate = GuestFunction::from_addr_with_thumb_bit(0x37751c);
    let char_bytes = (len + 1) * 4;
    let buffer: u32 = allocate.call_from_host(env, (allocator.to_bits(), char_bytes, 0i32));

    for i in 0..len {
        let ch: u32 = env.mem.read(Ptr::<u32, false>::from_bits(source_va + i * 4));
        env.mem.write(Ptr::<u32, true>::from_bits(buffer + i * 4), ch);
    }
    env.mem.write(Ptr::<u32, true>::from_bits(buffer + len * 4), 0u32);

    let string: MutPtr<u8> = env.mem.alloc(0x20).cast();
    env.mem.write(Ptr::<u32, true>::from_bits(string.to_bits()), buffer);
    env.mem.write(Ptr::<u32, true>::from_bits(string.to_bits() + 4), buffer + len * 4);
    env.mem.write(Ptr::<u32, true>::from_bits(string.to_bits() + 8), buffer + (len + 1) * 4);
    let allocator0: u32 = env.mem.read(Ptr::<u32, false>::from_bits(allocator.to_bits()));
    let allocator1: u32 = env.mem.read(Ptr::<u32, false>::from_bits(allocator.to_bits() + 4));
    env.mem.write(Ptr::<u32, true>::from_bits(string.to_bits() + 0x0c), allocator0);
    env.mem.write(Ptr::<u32, true>::from_bits(string.to_bits() + 0x10), allocator1);

    string.to_bits()
}

pub fn maybe_unlock_mtx(env: &mut Environment) {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    static DONE: AtomicBool = AtomicBool::new(false);
    static FRAMES: AtomicU32 = AtomicU32::new(0);

    if DONE.load(Ordering::Relaxed) {
        return;
    }
    if std::env::var_os("TOUCHHLE_DS_UNLOCK_MTX").is_none() {
        return;
    }
    if std::env::var_os("TOUCHHLE_DS_UNLOCK_MTX_AUTO").is_some() {
        let frame = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
        if frame < 300 {
            return;
        }
    } else if !std::path::Path::new("e3-unlock-now").exists() {
        return;
    }

    let get_instance = GuestFunction::from_addr_with_thumb_bit(0x17e078);
    let settings: u32 = get_instance.call_from_host(env, ());
    if settings == 0 {
        return;
    }

    let get_module_count = GuestFunction::from_addr_with_thumb_bit(0x178d58);
    let module_count: i32 = get_module_count.call_from_host(env, (settings,));
    if module_count < 2 {
        return;
    }

    if DONE.swap(true, Ordering::Relaxed) {
        return;
    }

    let unlock_module = GuestFunction::from_addr_with_thumb_bit(0x17a8d4);
    let save = GuestFunction::from_addr_with_thumb_bit(0x17a508);

    eprintln!(
        "E3UNLOCK: Settings instance = {:#x}, module count = {}",
        settings, module_count
    );

    for (name, source, len) in [
        ("attack", 0x8EBD80u32, 6u32),
        ("defense", 0x8EBD9Cu32, 7u32),
        ("loot", 0x8EBDBCu32, 4u32),
    ] {
        let string = make_wstring_from_wide(env, source, len);
        let _: () = unlock_module.call_from_host(env, (settings, string, true));
        eprintln!("E3UNLOCK: unlocked {}", name);
    }

    let filename = make_wstring_from_wide(env, 0x8EC604u32, 16u32);
    let _: () = save.call_from_host(env, (settings, filename));
    eprintln!("E3UNLOCK: settings save requested");
}



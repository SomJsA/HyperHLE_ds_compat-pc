/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Cross-platform memory management wrappers using the host's system calls.

/// Cross-platform memory allocation using the host's system calls.
/// Returns an address aligned to the guest's 4KB page boundaries.
///
/// - The function returns a raw pointer to allocated memory.
///   The caller is responsible for managing that memory.
/// - The returned pointer must be freed using the corresponding
///   [`free_memory`] call.
#[cfg(windows)]
pub(super) unsafe fn allocate_memory(size: usize) -> std::io::Result<*mut core::ffi::c_void> {
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
    };

    let ptr = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            size,
            MEM_RESERVE | MEM_COMMIT,
            PAGE_READWRITE,
        )
    };

    if ptr.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    Ok(ptr)
}

#[cfg(unix)]
pub(super) unsafe fn allocate_memory(size: usize) -> std::io::Result<*mut core::ffi::c_void> {
    use libc::{mmap, sysconf, _SC_PAGESIZE, MAP_ANONYMOUS, MAP_PRIVATE, PROT_READ, PROT_WRITE};

    const PAGE_SIZE: usize = crate::mem::PAGE_SIZE as usize;
    let host_page_size = unsafe { sysconf(_SC_PAGESIZE) as usize };

    assert!(
        host_page_size >= PAGE_SIZE,
        "Hosts with smaller than 4KiB pages are not supported."
    );

    let ptr = unsafe {
        mmap(
            std::ptr::null_mut(),
            size,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        )
    };

    if ptr == libc::MAP_FAILED {
        return Err(std::io::Error::last_os_error());
    }
    Ok(ptr)
}

#[cfg(windows)]
pub(super) unsafe fn allocate_guest_memory(
    size: usize,
) -> std::io::Result<*mut core::ffi::c_void> {
    use windows_sys::Win32::System::Memory::{VirtualAlloc, MEM_RESERVE, PAGE_READWRITE};

    let ptr = unsafe { VirtualAlloc(std::ptr::null(), size, MEM_RESERVE, PAGE_READWRITE) };

    if ptr.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    lazy_commit::register(ptr as usize, size);
    Ok(ptr)
}

#[cfg(unix)]
pub(super) unsafe fn allocate_guest_memory(
    size: usize,
) -> std::io::Result<*mut core::ffi::c_void> {
    unsafe { allocate_memory(size) }
}

/// Cross-platform memory free using the host's system calls.
///
/// # Safety
/// - The address and size should match parameters and result of the
///   [`allocate_memory`] call.
#[cfg(windows)]
pub(super) unsafe fn free_memory(
    address: *mut core::ffi::c_void,
    _size: usize,
) -> std::io::Result<()> {
    use windows_sys::Win32::System::Memory::{VirtualFree, MEM_RELEASE};

    let res = unsafe { VirtualFree(address, 0, MEM_RELEASE) };

    if res == 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(unix)]
pub(super) unsafe fn free_memory(
    address: *mut core::ffi::c_void,
    size: usize,
) -> std::io::Result<()> {
    use libc::munmap;

    let res = unsafe { munmap(address, size) };

    if res == -1 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(windows)]
pub(super) unsafe fn free_guest_memory(
    address: *mut core::ffi::c_void,
    size: usize,
) -> std::io::Result<()> {
    lazy_commit::unregister(address as usize);
    unsafe { free_memory(address, size) }
}

#[cfg(unix)]
pub(super) unsafe fn free_guest_memory(
    address: *mut core::ffi::c_void,
    size: usize,
) -> std::io::Result<()> {
    unsafe { free_memory(address, size) }
}

#[cfg(windows)]
mod lazy_commit {
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::EXCEPTION_ACCESS_VIOLATION;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH,
        EXCEPTION_POINTERS,
    };
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc, VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
    };

    struct Region {
        base: usize,
        size: usize,
    }

    static REGIONS: Mutex<Vec<Region>> = Mutex::new(Vec::new());
    static HANDLER_ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();

    pub(super) fn register(base: usize, size: usize) {
        {
            let mut regions = REGIONS.lock().unwrap();
            regions.push(Region { base, size });
        }
        HANDLER_ONCE.get_or_init(|| {
            let res = unsafe { AddVectoredExceptionHandler(1, Some(veh)) };
            assert!(
                !res.is_null(),
                "AddVectoredExceptionHandler failed: {}",
                std::io::Error::last_os_error()
            );
        });
    }

    pub(super) fn unregister(base: usize) {
        let mut regions = REGIONS.lock().unwrap();
        regions.retain(|r| r.base != base);
    }

    unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
        let info = unsafe { &*info };
        let rec_ptr = info.ExceptionRecord;
        if rec_ptr.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let rec = unsafe { &*rec_ptr };
        if rec.ExceptionCode as u32 != EXCEPTION_ACCESS_VIOLATION as u32 {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        if rec.NumberParameters < 2 {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let fault_addr = rec.ExceptionInformation[1] as usize;

        let in_region = {
            let regions = match REGIONS.lock() {
                Ok(g) => g,
                Err(_) => return EXCEPTION_CONTINUE_SEARCH,
            };
            regions
                .iter()
                .any(|r| fault_addr >= r.base && fault_addr < r.base.wrapping_add(r.size))
        };
        if !in_region {
            return EXCEPTION_CONTINUE_SEARCH;
        }

        // Commit only genuinely reserved pages. A committed PAGE_NOACCESS or
        // read-only violation belongs to its protection owner (e.g. Dynarmic's
        // guarded fastmem fallback), and must not be retried indefinitely here.
        let mut region: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { VirtualQuery(fault_addr as *const _, &mut region, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0
            || region.State != MEM_RESERVE
        {
            return EXCEPTION_CONTINUE_SEARCH;
        }

        const PAGE_SIZE: usize = super::super::PAGE_SIZE as usize;
        let page_addr = fault_addr & !(PAGE_SIZE - 1);

        let committed = unsafe {
            VirtualAlloc(
                page_addr as *const _,
                PAGE_SIZE,
                MEM_COMMIT,
                PAGE_READWRITE,
            )
        };
        if committed.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        EXCEPTION_CONTINUE_EXECUTION
    }


    #[cfg(test)]
    mod windows_fastmem_veh_tests {
        use super::*;
        use windows_sys::Win32::System::Memory::{VirtualFree, VirtualProtect, MEM_RELEASE, PAGE_NOACCESS};
        #[test]
        fn windows_fastmem_lazy_commit_defers_committed_protection_faults() {
            unsafe {
                let base=super::super::allocate_guest_memory(0x2000).unwrap();
                struct Cleanup(*mut std::ffi::c_void);
                impl Drop for Cleanup {
                    fn drop(&mut self) { unsafe { unregister(self.0 as usize);assert_ne!(VirtualFree(self.0,0,MEM_RELEASE),0); } }
                }
                let _cleanup=Cleanup(base);
                let mut record: windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_RECORD=std::mem::zeroed();
                record.ExceptionCode=EXCEPTION_ACCESS_VIOLATION;
                record.NumberParameters=2;
                record.ExceptionInformation[1]=base as usize;
                let mut info=EXCEPTION_POINTERS {ExceptionRecord:&mut record,ContextRecord:std::ptr::null_mut()};
                // A reserved data page really commits through the production VEH.
                assert_eq!(veh(&mut info),EXCEPTION_CONTINUE_EXECUTION);
                (base as *mut u32).write(0x12345678);
                let mut previous=0;
                assert_ne!(VirtualProtect(base,0x1000,PAGE_NOACCESS,&mut previous),0);
                // A committed protection fault must fall through, not claim success.
                assert_eq!(veh(&mut info),EXCEPTION_CONTINUE_SEARCH);
                let mut page:MEMORY_BASIC_INFORMATION=std::mem::zeroed();
                assert_ne!(VirtualQuery(base,&mut page,std::mem::size_of::<MEMORY_BASIC_INFORMATION>()),0);
                assert_eq!(page.Protect,PAGE_NOACCESS);
                let mut ignored=0;assert_ne!(VirtualProtect(base,0x1000,previous,&mut ignored),0);
                assert_eq!((base as *const u32).read(),0x12345678);
                record.ExceptionInformation[1]=1;
                assert_eq!(veh(&mut info),EXCEPTION_CONTINUE_SEARCH);
            }
        }
    }
}

/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! CPU emulation.
//!
//! Implemented using the C++ library dynarmic, which is a dynamic recompiler.
//!
//! iPhone OS apps used either ARMv6 or ARMv7-A, which are both 32-bit ISAs.
//! For the moment, only ARMv6 has been tested.

use crate::abi::GuestFunction;
use crate::mem::{ConstPtr, GuestUSize, Mem, MutPtr, Ptr, SafeRead, SafeWrite};

// Import functions from C++
use touchHLE_dynarmic_wrapper::*;

type VAddr = u32;
pub type CpuContext = touchHLE_DynarmicContext;

fn touchHLE_cpu_read_impl<T: SafeRead + Default>(
    mem: *mut touchHLE_Mem,
    addr: VAddr,
    error: *mut bool,
) -> T {
    // If a panic occurs (probably due to a null-pointer access), we can't let
    // it keep unwinding as it will hit non-Rust stack frames (dynarmic).
    // Instead we catch the unwind and then tell the C++ code a problem occurred
    // so it can immediately halt CPU execution and then panic itself, now
    // with only Rust stack frames to worry about and with CPU state information
    // available that's useful for debugging.
    //
    // TODO: Disable this in debug mode? This relies on dynarmic's
    // check_halt_on_memory_access option which surely has a significant
    // performance impact.
    //
    // I'm not sure if this actually is unwind-safe, but considering
    // the emulator will crash anyway, maybe this is okay.
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mem = unsafe { &mut *mem.cast::<Mem>() };
        let ptr: ConstPtr<T> = Ptr::from_bits(addr);
        mem.read(ptr)
    }));
    unsafe {
        error.write(res.is_err());
    }
    res.unwrap_or_default()
}

fn touchHLE_cpu_write_impl<T: SafeWrite>(mem: *mut touchHLE_Mem, addr: VAddr, value: T) -> bool {
    // See comments above about catch_unwind
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mem = unsafe { &mut *mem.cast::<Mem>() };
        let ptr: MutPtr<T> = Ptr::from_bits(addr);
        mem.write(ptr, value)
    }));
    res.is_err()
}

// Export functions for use by C++
#[no_mangle]
extern "C" fn touchHLE_cpu_read_u8(mem: *mut touchHLE_Mem, addr: VAddr, error: *mut bool) -> u8 {
    touchHLE_cpu_read_impl(mem, addr, error)
}
#[no_mangle]
extern "C" fn touchHLE_cpu_read_u16(mem: *mut touchHLE_Mem, addr: VAddr, error: *mut bool) -> u16 {
    touchHLE_cpu_read_impl(mem, addr, error)
}
#[no_mangle]
extern "C" fn touchHLE_cpu_read_u32(mem: *mut touchHLE_Mem, addr: VAddr, error: *mut bool) -> u32 {
    touchHLE_cpu_read_impl(mem, addr, error)
}
#[no_mangle]
extern "C" fn touchHLE_cpu_read_code(mem: *mut touchHLE_Mem, addr: VAddr, error: *mut bool) -> u32 {
    crate::ds_scene_diagnostics::count(crate::ds_scene_diagnostics::Kind::JitCodeRead,4,addr as u64);
    touchHLE_cpu_read_impl(mem, addr, error)
}
#[no_mangle]
extern "C" fn touchHLE_cpu_read_u64(mem: *mut touchHLE_Mem, addr: VAddr, error: *mut bool) -> u64 {
    touchHLE_cpu_read_impl(mem, addr, error)
}
#[no_mangle]
extern "C" fn touchHLE_cpu_write_u8(mem: *mut touchHLE_Mem, addr: VAddr, value: u8) -> bool {
    touchHLE_cpu_write_impl(mem, addr, value)
}
#[no_mangle]
extern "C" fn touchHLE_cpu_write_u16(mem: *mut touchHLE_Mem, addr: VAddr, value: u16) -> bool {
    touchHLE_cpu_write_impl(mem, addr, value)
}
#[no_mangle]
extern "C" fn touchHLE_cpu_write_u32(mem: *mut touchHLE_Mem, addr: VAddr, value: u32) -> bool {
    touchHLE_cpu_write_impl(mem, addr, value)
}
#[no_mangle]
extern "C" fn touchHLE_cpu_write_u64(mem: *mut touchHLE_Mem, addr: VAddr, value: u64) -> bool {
    touchHLE_cpu_write_impl(mem, addr, value)
}

pub struct Cpu {
    dynarmic_wrapper: *mut touchHLE_DynarmicWrapper,
    /// Copy of the direct memory access pointer used to check it has not
    /// changed. If this is null, direct memory access is not in use.
    direct_memory_access_ptr: *const std::ffi::c_void,
}

impl Drop for Cpu {
    fn drop(&mut self) {
        unsafe { touchHLE_DynarmicWrapper_delete(self.dynarmic_wrapper) }
    }
}

/// Why CPU execution ended.
#[derive(Debug)]
pub enum CpuState {
    /// Execution halted due to using up all remaining ticks (normal execution)
    /// or after the single instruction was executed (step execution).
    Normal,
    /// SVC instruction encountered.
    Svc(u32),
    /// An error was encountered.
    Error(CpuError),
}

/// A reason that can cause CPU execution to be interrupted.
#[derive(Debug, Clone, PartialEq)]
pub enum CpuError {
    /// Memory error during execution (probably a null page access).
    MemoryError,
    /// Undefined instruction (perhaps from a GDB software breakpoint).
    UndefinedInstruction,
    /// The guest executed an instruction whose behaviour is
    /// implementation-defined ("unpredictable" in the ARM ARM), or a hint
    /// instruction the dynarmic wrapper does not implement. Handled like
    /// `UndefinedInstruction` (log + bypass) instead of aborting the process.
    UnpredictableInstruction,
    /// Breakpoint (`bkpt` instruction).
    Breakpoint,
}

impl Cpu {
    /// The register number of the stack pointer.
    pub const SP: usize = 13;
    /// The register number of the link register.
    #[allow(unused)]
    pub const LR: usize = 14;
    /// The register number of the program counter.
    pub const PC: usize = 15;

    /// When this bit is set in CPSR, the CPU is in Thumb mode.
    pub const CPSR_THUMB: u32 = 0x00000020;

    /// When this bit is set in CPSR, the CPU is in user mode.
    pub const CPSR_USER_MODE: u32 = 0x00000010;

    /// Construct a new CPU instance. If a mutable reference to a [Mem] instance
    /// is provided, direct memory access is enabled, and the CPU instance
    /// becomes bound to that [Mem] instance (subsequent calls must use the same
    /// one).
    pub fn new(direct_memory_access: Option<&mut Mem>) -> Cpu {
        // Null page count is in pages rather than bytes. Mem ensures it is
        // page aligned.
        let null_page_count: usize = direct_memory_access
            .as_ref()
            .map_or(0, |mem| mem.null_segment_size() / 0x1000)
            .try_into()
            .unwrap();
        // Safety: the direct memory access pointer will be retained directly by
        // the dynarmic wrapper and indirectly by cached JIT code, so we must
        // ensure we only execute the CPU while holding a &mut on the Mem object
        // to which that pointer belongs.
        let direct_memory_access_ptr = direct_memory_access
            .map_or(std::ptr::null_mut(), |mem| unsafe {
                mem.direct_memory_access_ptr()
            });
        let dynarmic_wrapper =
            unsafe { touchHLE_DynarmicWrapper_new(direct_memory_access_ptr, null_page_count) };
        Cpu {
            dynarmic_wrapper,
            direct_memory_access_ptr,
        }
    }

    pub fn regs(&self) -> &[u32; 16] {
        unsafe {
            let ptr = touchHLE_DynarmicWrapper_regs_const(self.dynarmic_wrapper);
            &*(ptr as *const [u32; 16])
        }
    }
    pub fn regs_mut(&mut self) -> &mut [u32; 16] {
        unsafe {
            let ptr = touchHLE_DynarmicWrapper_regs_mut(self.dynarmic_wrapper);
            &mut *(ptr as *mut [u32; 16])
        }
    }
    pub(crate) fn extregs_mut(&mut self) -> &mut [u32; 64] {
        // Same owned-CPU lifetime contract as regs_mut. No pointer escapes the
        // mutable CPU borrow; the wrapper always owns exactly64 scalar words.
        unsafe {
            let ptr = touchHLE_DynarmicWrapper_extregs_mut(self.dynarmic_wrapper);
            &mut *(ptr as *mut [u32; 64])
        }
    }

    /// Dump the registers of the current cpu to the log output.
    /// Silently ignores panics.
    pub fn dump_regs(&self) {
        let regs = self.regs();
        Self::echo_regs(regs);
    }

    pub fn echo_regs(regs: &[u32; 16]) {
        // Silently ignore panics so it's safe to use in contexts where we
        // can't panic.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for row in 0..4 {
                use std::fmt::Write;
                let mut line = String::new();
                for col in 0..4 {
                    let reg_idx = row * 4 + col;
                    match reg_idx {
                        Self::SP => write!(&mut line, "\t SP: "),
                        Self::LR => write!(&mut line, "\t LR: "),
                        Self::PC => write!(&mut line, "\t PC: "),
                        _ if reg_idx <= 9 => write!(&mut line, "\t R{reg_idx}: "),
                        _ => write!(&mut line, "\tR{reg_idx}: "),
                    }
                    .unwrap();
                    write!(&mut line, "{:#010x}", regs[reg_idx]).unwrap();
                }
                echo!("{}", line);
            }
        }));
    }

    pub fn cpsr(&self) -> u32 {
        unsafe { touchHLE_DynarmicWrapper_cpsr(self.dynarmic_wrapper) }
    }
    pub fn set_cpsr(&mut self, cpsr: u32) {
        unsafe { touchHLE_DynarmicWrapper_set_cpsr(self.dynarmic_wrapper, cpsr) }
    }

    pub fn fpscr(&self) -> u32 {
        unsafe { touchHLE_DynarmicWrapper_fpscr(self.dynarmic_wrapper) }
    }
    pub fn set_fpscr(&mut self, fpscr: u32) {
        unsafe { touchHLE_DynarmicWrapper_set_fpscr(self.dynarmic_wrapper, fpscr) }
    }

    /// Swap the current state of the CPU (registers etc) with the state stored
    /// in the context object.
    pub fn swap_context(&mut self, context: &mut CpuContext) {
        unsafe { touchHLE_DynarmicWrapper_swap_context(self.dynarmic_wrapper, context) }
    }

    /// Get PC with the Thumb bit appropriately set.
    pub fn pc_with_thumb_bit(&self) -> GuestFunction {
        let pc = self.regs()[Self::PC];
        let thumb = (self.cpsr() & Self::CPSR_THUMB) == Self::CPSR_THUMB;
        GuestFunction::from_addr_and_thumb_flag(pc, thumb)
    }

    /// Set PC and the Thumb flag for executing a guest function. Note that this
    /// does not touch LR.
    pub fn branch(&mut self, new_pc: GuestFunction) {
        self.regs_mut()[Self::PC] = new_pc.addr_without_thumb_bit();
        let cpsr_without_thumb = self.cpsr() & (!Self::CPSR_THUMB);
        self.set_cpsr(cpsr_without_thumb | ((new_pc.is_thumb() as u32) * Self::CPSR_THUMB))
    }

    /// Set the PC and Thumb flag (like [Self::branch]), but also set the LR,
    /// and return the original PC and LR.
    pub fn branch_with_link(
        &mut self,
        new_pc: GuestFunction,
        new_lr: GuestFunction,
    ) -> (GuestFunction, GuestFunction) {
        let old_pc = self.pc_with_thumb_bit();
        let old_lr = GuestFunction::from_addr_with_thumb_bit(self.regs()[Self::LR]);
        self.branch(new_pc);
        self.regs_mut()[Self::LR] = new_lr.addr_with_thumb_bit();
        (old_pc, old_lr)
    }

    /// Clear dynarmic's instruction cache for some range of addresses.
    /// This is of interest to the dynamic linker, which will sometimes rewrite
    /// code.
    pub fn invalidate_cache_range(&mut self, base: VAddr, size: GuestUSize) {
        unsafe {
            touchHLE_DynarmicWrapper_invalidate_cache_range(self.dynarmic_wrapper, base, size)
        }
    }

    /// Start CPU execution.
    ///
    /// If `ticks` is [Some], it is used as an abstract time limit. The value
    /// will be reduced proportionately with the amount of ticks expended.
    ///
    /// If `ticks` is [None], the CPU executes only a single instruction. This
    /// is also known as "stepping".
    ///
    /// This will return either because the CPU ran out of time, or because
    /// something else happened which requires attention from the host.
    #[must_use]
    pub fn run_or_step(&mut self, mem: &mut Mem, ticks: Option<&mut u64>) -> CpuState {
        let _stutter_span=crate::ds_stutter_probe::Span::new(14,self.regs()[Self::PC] as u64);
        // See ::new() for why this is done.
        if !self.direct_memory_access_ptr.is_null() {
            assert!(self.direct_memory_access_ptr == unsafe { mem.direct_memory_access_ptr() });
        }

        let res = unsafe {
            touchHLE_DynarmicWrapper_run_or_step(
                self.dynarmic_wrapper,
                mem as *mut Mem as *mut touchHLE_Mem,
                ticks,
            )
        };
        match res {
            -1 => CpuState::Normal,
            -2 => CpuState::Error(CpuError::MemoryError),
            -3 => CpuState::Error(CpuError::UndefinedInstruction),
            -4 => CpuState::Error(CpuError::Breakpoint),
            -5 => CpuState::Error(CpuError::UnpredictableInstruction),
            _ if res < -5 => panic!("Unexpected CPU execution result"),
            svc => CpuState::Svc(svc as u32),
        }
    }
}


#[cfg(all(test, windows, target_arch = "x86_64"))]
mod windows_fastmem_tests {
    use super::*;
    use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_RESERVE, PAGE_NOACCESS, PAGE_READWRITE};
    struct EnvRestore(Option<std::ffi::OsString>);
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            match self.0.take() { Some(v) => std::env::set_var("TOUCHHLE_DS_CPU_FASTMEM",v), None => std::env::remove_var("TOUCHHLE_DS_CPU_FASTMEM") }
        }
    }
    fn page(base: *mut std::ffi::c_void) -> MEMORY_BASIC_INFORMATION {
        let mut info=unsafe { std::mem::zeroed() };
        assert_ne!(unsafe { VirtualQuery(base,&mut info,std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) },0);
        info
    }
    fn execute(cpu: &mut Cpu,mem: &mut Mem) {
        let mut ticks=10_000_000_000;
        assert!(matches!(cpu.run_or_step(mem,Some(&mut ticks)),CpuState::Svc(1)));
    }
    #[test]
    fn windows_fastmem_actual_jit_matches_page_table_and_null_contract() {
        let _restore=EnvRestore(std::env::var_os("TOUCHHLE_DS_CPU_FASTMEM"));
        // ARM memory loop, assembled form retained in dev-docs/PC-JIT-FASTMEM.md.
        let code=[0xe1a00006u32,0xe1a01007,0xe4905004,0xe0833005,0xe0255004,0xe2844001,
                  0xe5005004,0xe2511001,0x1afffff8,0xe2522001,0x1afffff4,0xef000001];
        let mut expected=None;
        for (round,fast) in [false,true,true,false,false,true].into_iter().enumerate() {
            std::env::set_var("TOUCHHLE_DS_CPU_FASTMEM",if fast {"1"} else {"0"});
            let mut mem=Mem::new(); mem.set_null_segment_size(0x1000);
            let base=unsafe { mem.direct_memory_access_ptr() };
            // Exercise both pre-existing committed and initially reserved null pages.
            let committed=round%2==0;
            if committed { unsafe { (base as *mut u32).write(0xa5a5a5a5); } }
            let initial_state=page(base).State;
            for (i,v) in code.into_iter().enumerate() {mem.write(MutPtr::from_bits(0x2000+i as u32*4),v);}
            let mut cpu=Cpu::new(Some(&mut mem));
            if fast {assert_eq!(page(base).Protect,PAGE_NOACCESS);}
            let initialize=|cpu:&mut Cpu,mem:&mut Mem,loops:u32| {
                cpu.regs_mut().fill(0);cpu.set_cpsr(Cpu::CPSR_USER_MODE);
                let r=cpu.regs_mut();r[2]=loops;r[4]=1;r[6]=0x10000;r[7]=1024;r[15]=0x2000;
                for i in 0..1024u32 {mem.write(MutPtr::from_bits(0x10000+i*4),i);}
            };
            initialize(&mut cpu,&mut mem,4);execute(&mut cpu,&mut mem);
            initialize(&mut cpu,&mut mem,10_000);
            let start=std::time::Instant::now();execute(&mut cpu,&mut mem);let elapsed=start.elapsed();
            let data:Vec<u32>=(0..1024).map(|i|mem.read(ConstPtr::from_bits(0x10000+i*4))).collect();
            let actual=(*cpu.regs(),cpu.cpsr(),data);
            if let Some(ref previous)=expected {assert_eq!(&actual,previous);} else {expected=Some(actual);}
            assert_eq!(cpu.regs()[4],1+10_000*1024);
            println!("FASTMEM_COMPONENT round={} fast={} milliseconds={:.3}",round,fast,elapsed.as_secs_f64()*1000.0);
            for (i,v) in [0xe5910000u32,0xe5812000,0xe5913000,0xef000001].into_iter().enumerate() {
                mem.write(MutPtr::from_bits(0x3000+i as u32*4),v);
            }
            cpu.invalidate_cache_range(0x3000,16);
            cpu.regs_mut()[1]=0;cpu.regs_mut()[2]=0xdeadbeef;cpu.regs_mut()[15]=0x3000;
            execute(&mut cpu,&mut mem);
            assert_eq!(cpu.regs()[0],0);assert_eq!(cpu.regs()[3],0);
            drop(cpu);
            assert_eq!(page(base).State,initial_state);
            if committed {
                assert_eq!(page(base).Protect,PAGE_READWRITE);
                assert_eq!(unsafe{(base as *const u32).read()},0xa5a5a5a5);
            } else {assert_eq!(page(base).State,MEM_RESERVE);}
        }
        // No direct memory and an unconfigured null segment must stay on callbacks/table.
        std::env::set_var("TOUCHHLE_DS_CPU_FASTMEM","1");
        drop(Cpu::new(None));
        let mut mem=Mem::new();let base=unsafe{mem.direct_memory_access_ptr()};
        let state=page(base).State;let cpu=Cpu::new(Some(&mut mem));assert_eq!(page(base).State,state);drop(cpu);
    }
}

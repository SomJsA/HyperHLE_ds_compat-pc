/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include "pc_fastmem_guard.h"
#include "pc_jit_map.h"

#include "dynarmic/interface/A32/a32.h"
#include "dynarmic/interface/A32/config.h"
#include "dynarmic/interface/A32/coprocessor.h"
#include "dynarmic/interface/exclusive_monitor.h"

namespace touchHLE::cpu {

using VAddr = std::uint32_t;

// Types and functions defined in Rust
extern "C" {
struct touchHLE_Mem;
std::uint8_t touchHLE_cpu_read_u8(touchHLE_Mem *mem, VAddr addr, bool *error);
std::uint16_t touchHLE_cpu_read_u16(touchHLE_Mem *mem, VAddr addr, bool *error);
std::uint32_t touchHLE_cpu_read_u32(touchHLE_Mem *mem, VAddr addr, bool *error);
std::uint32_t touchHLE_cpu_read_code(touchHLE_Mem *mem, VAddr addr, bool *error);
std::uint64_t touchHLE_cpu_read_u64(touchHLE_Mem *mem, VAddr addr, bool *error);
bool touchHLE_cpu_write_u8(touchHLE_Mem *mem, VAddr addr, std::uint8_t value);
bool touchHLE_cpu_write_u16(touchHLE_Mem *mem, VAddr addr, std::uint16_t value);
bool touchHLE_cpu_write_u32(touchHLE_Mem *mem, VAddr addr, std::uint32_t value);
bool touchHLE_cpu_write_u64(touchHLE_Mem *mem, VAddr addr, std::uint64_t value);
struct touchHLE_DynarmicContext {
  std::array<std::uint32_t, 16> regs;
  std::array<std::uint32_t, 64> extregs;
  std::uint32_t cpsr;
  std::uint32_t fpscr;
};
}

const auto HaltReasonSvc = Dynarmic::HaltReason::UserDefined1;
const auto HaltReasonUndefinedInstruction = Dynarmic::HaltReason::UserDefined2;
const auto HaltReasonBreakpoint = Dynarmic::HaltReason::UserDefined3;
// ★ An instruction whose behaviour the ARM ARM leaves "unpredictable", or a
// hint instruction this wrapper does not implement. Real hardware does
// *something* here; dynarmic refuses to guess. Aborting the whole emulator on
// these is much worse than continuing: measured 2026-10-17, a save-state
// restore left Dead Space executing one and the process died with
// STATUS_STACK_BUFFER_OVERRUN ("ExceptionRaised: unexpected exception 1"),
// killing an otherwise healthy session. They now halt with a recoverable
// reason so the Rust layer's existing bypass (log + fake return to LR) runs.
const auto HaltReasonUnpredictable = Dynarmic::HaltReason::UserDefined4;

class Environment final : public Dynarmic::A32::UserCallbacks {
public:
  Dynarmic::A32::Jit *cpu = nullptr;
  touchHLE_Mem *mem = nullptr;
  std::uint64_t ticks_remaining;
  uint32_t halting_svc;

private:
  std::uint8_t MemoryRead8(VAddr vaddr) override {
    bool error;
    auto value = touchHLE_cpu_read_u8(mem, vaddr, &error);
    if (error) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
    return value;
  }
  std::uint16_t MemoryRead16(VAddr vaddr) override {
    bool error;
    auto value = touchHLE_cpu_read_u16(mem, vaddr, &error);
    if (error) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
    return value;
  }
  std::uint32_t MemoryRead32(VAddr vaddr) override {
    bool error;
    auto value = touchHLE_cpu_read_u32(mem, vaddr, &error);
    if (error) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
    return value;
  }
  std::uint64_t MemoryRead64(VAddr vaddr) override {
    bool error;
    auto value = touchHLE_cpu_read_u64(mem, vaddr, &error);
    if (error) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
    return value;
  }

  std::optional<std::uint32_t> MemoryReadCode(VAddr vaddr) override {
    bool error;
    auto value = touchHLE_cpu_read_code(mem, vaddr, &error);
    if (error) {
      return std::nullopt;
    } else {
      return value;
    }
  }

  void MemoryWrite8(VAddr vaddr, std::uint8_t value) override {
    if (touchHLE_cpu_write_u8(mem, vaddr, value)) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
  }
  void MemoryWrite16(VAddr vaddr, std::uint16_t value) override {
    if (touchHLE_cpu_write_u16(mem, vaddr, value)) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
  }
  void MemoryWrite32(VAddr vaddr, std::uint32_t value) override {
    if (touchHLE_cpu_write_u32(mem, vaddr, value)) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
  }
  void MemoryWrite64(VAddr vaddr, std::uint64_t value) override {
    if (touchHLE_cpu_write_u64(mem, vaddr, value)) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    }
  }

  bool MemoryWriteExclusive8(VAddr addr, std::uint8_t value,
                             std::uint8_t expected) override {
    if (MemoryRead8(addr) != expected) {
      return false;
    }
    MemoryWrite8(addr, value);
    return true;
  }
  bool MemoryWriteExclusive16(VAddr addr, std::uint16_t value,
                              std::uint16_t expected) override {
    if (MemoryRead16(addr) != expected) {
      return false;
    }
    MemoryWrite16(addr, value);
    return true;
  }
  bool MemoryWriteExclusive32(VAddr addr, std::uint32_t value,
                              std::uint32_t expected) override {
    if (MemoryRead32(addr) != expected) {
      return false;
    }
    MemoryWrite32(addr, value);
    return true;
  }
  bool MemoryWriteExclusive64(VAddr addr, std::uint64_t value,
                              std::uint64_t expected) override {
    if (MemoryRead64(addr) != expected) {
      return false;
    }
    MemoryWrite64(addr, value);
    return true;
  }

  void InterpreterFallback(std::uint32_t, size_t) override {
    abort(); // TODO
  }
  void CallSVC(std::uint32_t svc) override {
    halting_svc = svc;
    cpu->HaltExecution(HaltReasonSvc);
  }
  void ExceptionRaised(VAddr pc, Dynarmic::A32::Exception exception) override {
    // MemoryReadCode returned nullopt
    if (exception == Dynarmic::A32::Exception::NoExecuteFault) {
      cpu->HaltExecution(Dynarmic::HaltReason::MemoryAbort);
    } else if (exception == Dynarmic::A32::Exception::UndefinedInstruction) {
      cpu->HaltExecution(HaltReasonUndefinedInstruction);
    } else if (exception == Dynarmic::A32::Exception::Breakpoint) {
      cpu->HaltExecution(HaltReasonBreakpoint);
    } else if (exception == Dynarmic::A32::Exception::DecodeError) {
      // A real decoder bug - keep failing loudly.
      std::fprintf(stderr, "ExceptionRaised: decode error at %x\n", pc);
      abort();
    } else {
      // UnpredictableInstruction, or a hint instruction (SEV/SEVL/WFI/WFE/
      // YIELD/PLD/PLI) that this wrapper does not implement. Log the kind once
      // per boot and let the Rust layer bypass it.
      static std::uint64_t seen = 0;
      const std::uint64_t bit = 1ull << (unsigned(exception) & 63u);
      if ((seen & bit) == 0) {
        seen |= bit;
        std::fprintf(stderr,
                     "ExceptionRaised: unhandled exception %u at %x - treating "
                     "it as a recoverable undefined instruction\n",
                     unsigned(exception), pc);
      }
      cpu->HaltExecution(HaltReasonUnpredictable);
    }
  }
  void AddTicks(std::uint64_t ticks) override {
    if (ticks > ticks_remaining) {
      ticks_remaining = 0;
      return;
    }
    ticks_remaining -= ticks;
  }
  std::uint64_t GetTicksRemaining() override { return ticks_remaining; }
};

class ArmDynarmicCP15 : public Dynarmic::A32::Coprocessor {
  static std::uint64_t Ignore(void *, std::uint32_t, std::uint32_t) {
    return 0;
  }

  static Callback CallbackForIgnoredOperation() {
    return Callback{&Ignore, std::nullopt};
  }

public:
  using CoprocReg = Dynarmic::A32::CoprocReg;

  CallbackOrAccessOneWord CompileSendOneWord(bool, unsigned, CoprocReg,
                                             CoprocReg, unsigned) override {
    return CallbackForIgnoredOperation();
  }

  std::optional<Callback> CompileInternalOperation(bool, unsigned, CoprocReg,
                                                   CoprocReg, CoprocReg,
                                                   unsigned) override {
    return CallbackForIgnoredOperation();
  }

  CallbackOrAccessTwoWords CompileSendTwoWords(bool, unsigned, CoprocReg) override {
    return CallbackForIgnoredOperation();
  }

  CallbackOrAccessOneWord CompileGetOneWord(bool, unsigned, CoprocReg,
                                            CoprocReg, unsigned) override {
    return CallbackForIgnoredOperation();
  }

  CallbackOrAccessTwoWords CompileGetTwoWords(bool, unsigned, CoprocReg) override {
    return CallbackForIgnoredOperation();
  }

  std::optional<Callback> CompileLoadWords(bool, bool, CoprocReg,
                                           std::optional<std::uint8_t>) override {
    return CallbackForIgnoredOperation();
  }

  std::optional<Callback> CompileStoreWords(bool, bool, CoprocReg,
                                            std::optional<std::uint8_t>) override {
    return CallbackForIgnoredOperation();
  }
};

class DynarmicWrapper {
  Environment env;
  std::unique_ptr<Dynarmic::A32::Jit> cpu;
  std::unique_ptr<Dynarmic::ExclusiveMonitor> mon;
  std::array<std::uint8_t *, Dynarmic::A32::UserConfig::NUM_PAGE_TABLE_ENTRIES>
      page_table;
#if defined(_WIN32) && defined(_M_X64)
  PcFastmemGuard fastmem_guard;
#endif

public:
  DynarmicWrapper(void *direct_memory_access_ptr, size_t null_page_count) {
#if defined(_WIN32) && defined(_M_X64)
    pc_jit_map::initialize();
#endif
    Dynarmic::A32::UserConfig user_config;
    user_config.callbacks = &env;
    user_config.coprocessors[15] = std::make_shared<ArmDynarmicCP15>();
    mon = std::make_unique<Dynarmic::ExclusiveMonitor>(1);
    user_config.global_monitor = mon.get();
#ifndef NDEBUG
    user_config.check_halt_on_memory_access = true;
#endif
    if (direct_memory_access_ptr) {
      // Preserve page-table fallback and the compatibility callbacks for the
      // null segment, including zero reads and absorbed writes.
      page_table.fill((std::uint8_t *)direct_memory_access_ptr);
      // Note that the null page size is also defined in src/mem.rs.
      static_assert(1 << Dynarmic::A32::UserConfig::PAGE_BITS == 0x1000);

      if (null_page_count > page_table.size()) {
        printf("Too many null pages, %zu requested but maximum is %zu.",
               null_page_count, page_table.size());
        abort();
      }
      for (size_t i = 0; i < null_page_count; i++) {
        page_table[i] = nullptr;
      }
      user_config.page_table = &page_table;
      user_config.absolute_offset_page_table = true;
    }
#if defined(_WIN32) && defined(_M_X64)
    // Read the actual Windows environment, not the CRT's lazily cached copy.
    // Rust's SetEnvironmentVariableW and runtime overrides do not update getenv.
    char requested[8]{};
    const DWORD requested_length = GetEnvironmentVariableA("TOUCHHLE_DS_CPU_FASTMEM", requested, sizeof(requested));
    const bool want_fastmem = requested_length == 1 && requested[0] == '1';
    if (want_fastmem && fastmem_guard.enable(direct_memory_access_ptr,
                                            null_page_count * size_t{0x1000})) {
      user_config.fastmem_pointer = reinterpret_cast<uintptr_t>(direct_memory_access_ptr);
    }
    std::fprintf(stderr,
        "[DSjit-fastmem] requested=%d effective=%s null_pages=%zu fallback=page-table\n",
        want_fastmem, fastmem_guard.active() ? "fastmem" : "page-table", null_page_count);
#endif
    cpu = std::make_unique<Dynarmic::A32::Jit>(user_config);
    env.cpu = cpu.get();
  }

  ~DynarmicWrapper() { cpu.reset(); } // Stop JIT before guard member restores pages.

  const std::uint32_t *regs() const { return &cpu->Regs().front(); }
  std::uint32_t *regs() { return &cpu->Regs().front(); }
  std::uint32_t *extregs() { return &cpu->ExtRegs().front(); }

  std::uint32_t cpsr() const { return cpu->Cpsr(); }
  void set_cpsr(std::uint32_t cpsr) { cpu->SetCpsr(cpsr); }
  std::uint32_t fpscr() const { return cpu->Fpscr(); }
  void set_fpscr(std::uint32_t fpscr) { cpu->SetFpscr(fpscr); }

  void invalidate_cache_range(VAddr start, std::uint32_t size) {
    cpu->InvalidateCacheRange(start, size);
  }

  void precompile(touchHLE_Mem* memory, const std::uint32_t* addresses, const bool* thumb,
                  std::size_t count, std::uint32_t cpsr, std::uint32_t fpscr) {
    env.mem = memory;
    cpu->Precompile(addresses, thumb, count, cpsr, fpscr);
  }

  void swap_context(touchHLE_DynarmicContext *context) {
    touchHLE_DynarmicContext tmp = {cpu->Regs(), cpu->ExtRegs(), cpu->Cpsr(),
                                    cpu->Fpscr()};
    cpu->Regs() = context->regs;
    cpu->ExtRegs() = context->extregs;
    cpu->SetCpsr(context->cpsr);
    cpu->SetFpscr(context->fpscr);
    *context = tmp;
  }

  std::int32_t run_or_step(touchHLE_Mem *mem, std::uint64_t *ticks) {
    env.mem = mem;
    Dynarmic::HaltReason hr;
    if (ticks) {
      env.ticks_remaining = *ticks;
      hr = cpu->Run();
    } else {
      hr = cpu->Step();
    }
    std::int32_t res;
    if ((!hr && ticks) || (hr == Dynarmic::HaltReason::Step && !ticks)) {
      res = -1;
    } else if (Dynarmic::Has(hr, Dynarmic::HaltReason::MemoryAbort)) {
      res = -2;
    } else if (Dynarmic::Has(hr, HaltReasonUndefinedInstruction)) {
      res = -3;
    } else if (Dynarmic::Has(hr, HaltReasonBreakpoint)) {
      res = -4;
    } else if (Dynarmic::Has(hr, HaltReasonUnpredictable)) {
      res = -5;
    } else if (Dynarmic::Has(hr, HaltReasonSvc)) {
      res = std::int32_t(env.halting_svc);
    } else {
      printf("unhandled halt reason %u\n", unsigned(hr));
      abort();
    }
    env.mem = nullptr;
    if (ticks) {
      *ticks = env.ticks_remaining;
    }
    return res;
  }
};

extern "C" {

DynarmicWrapper *touchHLE_DynarmicWrapper_new(void *direct_memory_access_ptr,
                                              size_t null_page_count) {
  return new DynarmicWrapper(direct_memory_access_ptr, null_page_count);
}
void touchHLE_DynarmicWrapper_delete(DynarmicWrapper *cpu) { delete cpu; }

const std::uint32_t *
touchHLE_DynarmicWrapper_regs_const(const DynarmicWrapper *cpu) {
  return cpu->regs();
}
std::uint32_t *touchHLE_DynarmicWrapper_regs_mut(DynarmicWrapper *cpu) {
  return cpu->regs();
}
std::uint32_t *touchHLE_DynarmicWrapper_extregs_mut(DynarmicWrapper *cpu) {
  return cpu->extregs();
}

std::uint32_t touchHLE_DynarmicWrapper_cpsr(const DynarmicWrapper *cpu) {
  return cpu->cpsr();
}
void touchHLE_DynarmicWrapper_set_cpsr(DynarmicWrapper *cpu,
                                       std::uint32_t cpsr) {
  cpu->set_cpsr(cpsr);
}

std::uint32_t touchHLE_DynarmicWrapper_fpscr(const DynarmicWrapper *cpu) {
  return cpu->fpscr();
}
void touchHLE_DynarmicWrapper_set_fpscr(DynarmicWrapper *cpu, std::uint32_t fpscr) {
  cpu->set_fpscr(fpscr);
}

void touchHLE_DynarmicWrapper_swap_context(DynarmicWrapper *cpu,
                                           touchHLE_DynarmicContext *context) {
  cpu->swap_context(context);
}

void touchHLE_DynarmicWrapper_invalidate_cache_range(DynarmicWrapper *cpu,
                                                     VAddr start,
                                                     std::uint32_t size) {
  cpu->invalidate_cache_range(start, size);
}

void touchHLE_DynarmicWrapper_precompile(DynarmicWrapper *cpu, touchHLE_Mem *memory,
                                         const std::uint32_t *addresses, const bool *thumb,
                                         std::size_t count, std::uint32_t cpsr, std::uint32_t fpscr) {
  cpu->precompile(memory, addresses, thumb, count, cpsr, fpscr);
}

std::int32_t touchHLE_DynarmicWrapper_run_or_step(DynarmicWrapper *cpu,
                                                  touchHLE_Mem *mem,
                                                  std::uint64_t *ticks) {
  return cpu->run_or_step(mem, ticks);
}
}

} // namespace touchHLE::cpu

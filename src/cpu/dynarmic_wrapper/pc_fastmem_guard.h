/* SPDX-License-Identifier: MPL-2.0 */
// Win64 opt-in fastmem null guard. It never owns or frees the guest reservation.
#pragma once
#if defined(_WIN32) && defined(_M_X64)
#define WIN32_LEAN_AND_MEAN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <cstdio>
#include <cstdint>

class PcFastmemGuard {
  void *base = nullptr;
  size_t bytes = 0;
  DWORD prior_protect = 0;
  bool was_reserved = false;
public:
  PcFastmemGuard() = default;
  PcFastmemGuard(const PcFastmemGuard&) = delete;
  PcFastmemGuard& operator=(const PcFastmemGuard&) = delete;
  bool enable(void *candidate, size_t size) {
    SYSTEM_INFO system{};
    GetSystemInfo(&system);
    if (!candidate || size == 0 || size % system.dwPageSize != 0 ||
        reinterpret_cast<uintptr_t>(candidate) % system.dwPageSize != 0)
      return false;
    MEMORY_BASIC_INFORMATION region{};
    if (!VirtualQuery(candidate, &region, sizeof(region)) ||
        region.BaseAddress != candidate || region.AllocationBase != candidate ||
        region.RegionSize < size || region.Type != MEM_PRIVATE)
      return false;
    // Mixed protection/reservation shapes deliberately use the original table.
    // Never overwrite an unknown existing protection or commit arbitrary pages.
    if (region.State == MEM_RESERVE) {
      if (VirtualAlloc(candidate, size, MEM_COMMIT, PAGE_NOACCESS) != candidate)
        return false;
      was_reserved = true;
    } else if (region.State == MEM_COMMIT && region.Protect == PAGE_READWRITE) {
      if (!VirtualProtect(candidate, size, PAGE_NOACCESS, &prior_protect))
        return false;
    } else {
      return false;
    }
    base = candidate;
    bytes = size;
    return true;
  }
  bool active() const { return base != nullptr; }
  ~PcFastmemGuard() {
    if (!base) return;
    DWORD ignored;
    const BOOL restored = was_reserved
        ? VirtualFree(base, bytes, MEM_DECOMMIT)
        : VirtualProtect(base, bytes, prior_protect, &ignored);
    if (!restored)
      std::fprintf(stderr, "[DSjit-fastmem] guard_restore_failed error=%lu\n", GetLastError());
  }
};
#endif

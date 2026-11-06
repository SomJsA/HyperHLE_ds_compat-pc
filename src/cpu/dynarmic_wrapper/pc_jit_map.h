/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. https://mozilla.org/MPL/2.0/ */
#pragma once

#if defined(_WIN32) && defined(_M_X64)
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <cstdint>
#include <cstdio>
#include <mutex>
#include <string_view>
#include "dynarmic/interface/jit_map.h"

namespace touchHLE::cpu::pc_jit_map {
class Journal {
  static constexpr std::uint64_t limit = 16 * 1024 * 1024;
  HANDLE file = INVALID_HANDLE_VALUE;
  std::mutex mutex;
  std::uint64_t size = 0;
  std::uint64_t epoch = 0;

  static long long tick() noexcept {
    LARGE_INTEGER counter{};
    QueryPerformanceCounter(&counter);
    return counter.QuadPart;
  }
  void stop(const char* reason, DWORD error = 0) noexcept {
    std::fprintf(stderr, "[DSjit-map] disabled reason=%s error=%lu qpc=%lld\n",
                 reason, static_cast<unsigned long>(error), tick());
    if (file != INVALID_HANDLE_VALUE) CloseHandle(file);
    file = INVALID_HANDLE_VALUE;
  }
  bool write(const char* bytes, std::size_t length) noexcept {
    DWORD written = 0;
    if (!WriteFile(file, bytes, static_cast<DWORD>(length), &written, nullptr) ||
        written != length) {
      stop("write", GetLastError());
      return false;
    }
    size += length;
    return true;
  }
  bool reserve(std::size_t length) noexcept {
    if (file == INVALID_HANDLE_VALUE) return false;
    // Keep room for the terminal limit record. Never truncate an existing file.
    if (size + length + 128 <= limit) return true;
    char line[128];
    const int n = std::snprintf(line, sizeof(line), "L\t%lld\t%llu\t%llu\n",
        tick(), static_cast<unsigned long long>(epoch),
        static_cast<unsigned long long>(size));
    if (n > 0 && n < sizeof(line)) write(line, static_cast<std::size_t>(n));
    stop("limit");
    return false;
  }

public:
  Journal() noexcept {
    wchar_t path[32768];
    const DWORD length = GetEnvironmentVariableW(L"TOUCHHLE_DS_JIT_MAP", path, 32768);
    if (!length) return;
    if (length >= 32768) { stop("path-length"); return; }
    file = CreateFileW(path, GENERIC_WRITE, FILE_SHARE_READ, nullptr, CREATE_NEW,
                       FILE_ATTRIBUTE_NORMAL, nullptr);
    if (file == INVALID_HANDLE_VALUE) { stop("create", GetLastError()); return; }
    LARGE_INTEGER frequency{};
    if (!QueryPerformanceFrequency(&frequency) || frequency.QuadPart <= 0) {
      stop("qpc-frequency", GetLastError()); return;
    }
    FILETIME created{}, exited{}, kernel{}, user{};
    if (!GetProcessTimes(GetCurrentProcess(), &created, &exited, &kernel, &user)) {
      stop("process-identity", GetLastError()); return;
    }
    const auto creation = (static_cast<unsigned long long>(created.dwHighDateTime) << 32) |
                           created.dwLowDateTime;
    char line[128];
    const int n = std::snprintf(line, sizeof(line), "H\t2\t%lu\t%lld\t%llu\n",
        static_cast<unsigned long>(GetCurrentProcessId()), frequency.QuadPart, creation);
    if (n <= 0 || n >= sizeof(line)) { stop("header-format"); return; }
    if (!write(line, static_cast<std::size_t>(n))) return;
    std::fprintf(stderr, "[DSjit-map] enabled pid=%lu limit_bytes=%llu; emitted ranges, not CPU timing\n",
        static_cast<unsigned long>(GetCurrentProcessId()), static_cast<unsigned long long>(limit));
  }
  ~Journal() { if (file != INVALID_HANDLE_VALUE) CloseHandle(file); }
  bool enabled() const noexcept { return file != INVALID_HANDLE_VALUE; }
  void registration(const void* first, const void* last, std::string_view name) noexcept {
    const auto start = reinterpret_cast<std::uintptr_t>(first);
    const auto end = reinterpret_cast<std::uintptr_t>(last);
    if (end <= start) return;
    std::lock_guard lock(mutex);
    if (file == INVALID_HANDLE_VALUE) return;
    char label[161];
    const std::size_t length = name.size() < 160 ? name.size() : 160;
    for (std::size_t i = 0; i < length; ++i) {
      const char c = name[i];
      label[i] = (c == '\n' || c == '\r' || c == '\t' || c == '\0') ? ' ' : c;
    }
    label[length] = '\0';
    char line[320];
    const int n = std::snprintf(line, sizeof(line), "M\t%lld\t%llu\t%llx\t%llx\t%s\n",
        tick(), static_cast<unsigned long long>(epoch),
        static_cast<unsigned long long>(start), static_cast<unsigned long long>(end-start), label);
    if (n > 0 && n < sizeof(line) && reserve(static_cast<std::size_t>(n)))
      write(line, static_cast<std::size_t>(n));
  }
  void clear() noexcept {
    std::lock_guard lock(mutex);
    if (file == INVALID_HANDLE_VALUE) return;
    ++epoch;
    char line[128];
    const int n = std::snprintf(line, sizeof(line), "C\t%lld\t%llu\n",
        tick(), static_cast<unsigned long long>(epoch));
    if (n > 0 && n < sizeof(line) && reserve(static_cast<std::size_t>(n)))
      write(line, static_cast<std::size_t>(n));
  }
};

inline Journal& journal() {
  static Journal value;
  return value;
}
inline void registration(const void* first, const void* last, std::string_view name) noexcept {
  journal().registration(first, last, name);
}
inline void clear() noexcept { journal().clear(); }
inline void initialize() {
  // Read the real Windows environment once before the first CPU creates code.
  // No callbacks, file access or per-frame polling when the option is absent.
  static std::once_flag once;
  std::call_once(once, [] {
    if (journal().enabled())
      Dynarmic::Backend::X64::SetJitMapCallbacks(registration, clear);
  });
}
}  // namespace touchHLE::cpu::pc_jit_map
#endif

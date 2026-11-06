// Standalone boundary test driver. Fake addresses are logged, never accessed.
#include "pc_jit_map.h"
#include <string>
#include <thread>
#include <vector>
namespace Dynarmic::Backend::X64::detail {
void PerfMapRegister(const void*, const void*, std::string_view);
}
namespace Dynarmic::Backend::X64 { void PerfMapClear(); }
int main(int argc, char** argv) {
  if (argc != 2) return 2;
  touchHLE::cpu::pc_jit_map::initialize();
  const std::string mode(argv[1]);
  auto record = [](std::uintptr_t at, std::string_view label) {
    Dynarmic::Backend::X64::detail::PerfMapRegister(
        reinterpret_cast<const void*>(at), reinterpret_cast<const void*>(at+256), label);
  };
  if (mode == "normal") {
    record(0x1000,"old\tlabel\n");
    Dynarmic::Backend::X64::PerfMapClear();
    record(0x1000,"a32_a0037FAC8_le_fpcr00000000");
  } else if (mode == "stress") {
    std::vector<std::thread> threads;
    for (unsigned t=0;t<4;++t) threads.emplace_back([=] {
      for (unsigned i=0;i<1000;++i) record(0x1000+(t*1000+i)*256,"concurrent");
    });
    for (auto& thread:threads) thread.join();
  } else if (mode == "limit") {
    const std::string label(200,'x');
    for (unsigned i=0;i<300000;++i) record(0x1000+std::uintptr_t(i)*256,label);
  } else return 2;
  return 0;
}

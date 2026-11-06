// Explicit, finite wall-clock instruction-pointer sampling of one owned process.
// PSS captures contexts and owns its temporary synchronization. No injector,
// manual SuspendThread, process priority changes, or guest-memory access.
#define _WIN32_WINNT 0x0603
#define NTDDI_VERSION 0x06030000
#define NOMINMAX
#include <windows.h>
#include <processsnapshot.h>
#include <tlhelp32.h>
#include <cstdint>
#include <cstddef>
#include <cstring>
#include <cstdlib>
#include <cstdio>
#include <cwchar>
#include <algorithm>

static unsigned long long ticks(FILETIME t) {
  return (static_cast<unsigned long long>(t.dwHighDateTime)<<32)|t.dwLowDateTime;
}
static long long qpc() { LARGE_INTEGER v; QueryPerformanceCounter(&v); return v.QuadPart; }
struct Handle {
  HANDLE value=nullptr;
  ~Handle() { if (value && value!=INVALID_HANDLE_VALUE) CloseHandle(value); }
};
static unsigned cleanup_errors=0;
struct Snapshot {
  HPSS value=nullptr;
  ~Snapshot() { if(value && PssFreeSnapshot(GetCurrentProcess(),value)!=ERROR_SUCCESS) ++cleanup_errors; }
};
struct Marker {
  HPSSWALK value=nullptr;
  ~Marker() { if(value && PssWalkMarkerFree(value)!=ERROR_SUCCESS) ++cleanup_errors; }
};
static bool write(HANDLE file,const char* line) {
  DWORD n=0; const auto length=static_cast<DWORD>(std::strlen(line));
  return WriteFile(file,line,length,&n,nullptr) && n==length;
}
static void modules(HANDLE file,DWORD pid) {
  Handle snapshot{CreateToolhelp32Snapshot(TH32CS_SNAPMODULE|TH32CS_SNAPMODULE32,pid)};
  if(snapshot.value==INVALID_HANDLE_VALUE) return;
  MODULEENTRY32W entry{};entry.dwSize=sizeof(entry);
  for(BOOL more=Module32FirstW(snapshot.value,&entry);more;more=Module32NextW(snapshot.value,&entry)) {
    char name[512]{};
    if(!WideCharToMultiByte(CP_UTF8,0,entry.szModule,-1,name,sizeof(name),nullptr,nullptr)) continue;
    for(char* p=name;*p;++p) if(*p=='\t'||*p=='\r'||*p=='\n') *p=' ';
    char line[640];
    std::snprintf(line,sizeof(line),"B\t%lld\t%llx\t%lx\t%s\n",qpc(),
        reinterpret_cast<unsigned long long>(entry.modBaseAddr),entry.modBaseSize,name);
    if(!write(file,line)) return;
  }
}
int wmain(int argc,wchar_t** argv) {
  if(argc==2 && std::wcscmp(argv[1],L"--burn")==0) {
    const auto until=GetTickCount64()+20000;
    volatile std::uint64_t value=1;
    while(GetTickCount64()<until) for(unsigned i=0;i<100000;++i) value=value*1664525+1013904223;
    return value==0?1:0;
  }
  // PID + exact creation time prevent attaching to a reused/unrelated PID.
  // The controller supplies these only for its own Popen child.
  if(argc!=7) {
    std::fprintf(stderr,"usage: PcJitSampler PID CREATED_100NS SECONDS PERIOD_MS OUTPUT EXPECTED_EXE\n");
    return 2;
  }
  const DWORD pid=wcstoul(argv[1],nullptr,10);
  const auto expected_creation=_wcstoui64(argv[2],nullptr,10);
  const unsigned seconds=wcstoul(argv[3],nullptr,10), period=wcstoul(argv[4],nullptr,10);
  if(!pid || !expected_creation || seconds<1 || seconds>120 || period<10 || period>1000) return 2;
  Handle process{OpenProcess(PROCESS_QUERY_INFORMATION|PROCESS_VM_READ|PROCESS_DUP_HANDLE,FALSE,pid)};
  if(!process.value) { std::fprintf(stderr,"OpenProcess error=%lu\n",GetLastError());return 3; }
  FILETIME created,exited,kernel,user;
  if(!GetProcessTimes(process.value,&created,&exited,&kernel,&user) || ticks(created)!=expected_creation) {
    std::fprintf(stderr,"process creation identity mismatch\n");return 4;
  }
  wchar_t image[32768]; DWORD image_size=32768;
  if(!QueryFullProcessImageNameW(process.value,0,image,&image_size) || _wcsicmp(image,argv[6])!=0) {
    std::fprintf(stderr,"process executable identity mismatch\n");return 4;
  }
  Handle threads{CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD,0)};
  if(threads.value==INVALID_HANDLE_VALUE) return 3;
  DWORD main_tid=0; unsigned long long first=~0ull;
  THREADENTRY32 entry{};entry.dwSize=sizeof(entry);
  for(BOOL more=Thread32First(threads.value,&entry);more;more=Thread32Next(threads.value,&entry)) {
    if(entry.th32OwnerProcessID!=pid) continue;
    Handle thread{OpenThread(THREAD_QUERY_LIMITED_INFORMATION,FALSE,entry.th32ThreadID)};
    FILETIME c,e,k,u;
    if(thread.value && GetThreadTimes(thread.value,&c,&e,&k,&u) && ticks(c)<first) {
      first=ticks(c);main_tid=entry.th32ThreadID;
    }
  }
  if(!main_tid) return 3;
  Handle output{CreateFileW(argv[5],GENERIC_WRITE,FILE_SHARE_READ,nullptr,CREATE_NEW,FILE_ATTRIBUTE_NORMAL,nullptr)};
  if(output.value==INVALID_HANDLE_VALUE) {std::fprintf(stderr,"output create error=%lu\n",GetLastError());return 5;}
  LARGE_INTEGER frequency;QueryPerformanceFrequency(&frequency);
  char line[640];
  std::snprintf(line,sizeof(line),"H\t1\t%lu\t%lu\t%lld\t%llu\n",pid,main_tid,frequency.QuadPart,expected_creation);
  if(!write(output.value,line)) return 5;
  modules(output.value,pid);
  const auto until=GetTickCount64()+seconds*1000ull;
  unsigned samples=0,missing=0; std::uint32_t random=0x5ac317u;
  while(GetTickCount64()<until) {
    DWORD exit_code=0;
    if(!GetExitCodeProcess(process.value,&exit_code) || exit_code!=STILL_ACTIVE) break;
    const auto begin=qpc();
    {
    Snapshot snapshot;
    const DWORD captured=PssCaptureSnapshot(process.value,
        static_cast<PSS_CAPTURE_FLAGS>(PSS_CAPTURE_THREADS|PSS_CAPTURE_THREAD_CONTEXT),CONTEXT_CONTROL,&snapshot.value);
    const auto captured_at=qpc();
    if(captured!=ERROR_SUCCESS) {
      std::snprintf(line,sizeof(line),"E\t%lld\tcapture\t%lu\n",begin,captured);
      write(output.value,line);return 6;
    }
    Marker marker;
    DWORD error=PssWalkMarkerCreate(nullptr,&marker.value);
    if(error!=ERROR_SUCCESS) return 6;
    PSS_THREAD_ENTRY row{};
    bool found=false;
    while((error=PssWalkSnapshot(snapshot.value,PSS_WALK_THREADS,marker.value,&row,sizeof(row)))==ERROR_SUCCESS) {
      if(row.ProcessId!=pid || row.ThreadId!=main_tid) continue;
      if(!row.ContextRecord || row.SizeOfContextRecord<offsetof(CONTEXT,Rip)+sizeof(row.ContextRecord->Rip) ||
         (row.ContextRecord->ContextFlags&CONTEXT_CONTROL)!=CONTEXT_CONTROL ||
         row.ExitStatus!=STILL_ACTIVE) break;
      const auto end=qpc();
      std::snprintf(line,sizeof(line),"S\t%lld\t%lld\t%lld\t%llx\t%llu\t%llu\t%u\n",
          begin,captured_at,end,row.ContextRecord->Rip,ticks(row.KernelTime),ticks(row.UserTime),row.SuspendCount);
      if(!write(output.value,line)) return 5;
      ++samples;found=true;break;
    }
    if(!found) {
      ++missing;
      std::snprintf(line,sizeof(line),"U\t%lld\t%lld\t%lu\n",begin,qpc(),error);
      if(!write(output.value,line)) return 5;
    }
    }
    // RAII frees marker + snapshot before sleeping or next capture. Capture
    // perturbation and waits are retained; these are not on-CPU percentages.
    random=random*1664525+1013904223;
    const auto elapsed_ms=(qpc()-begin)*1000/frequency.QuadPart;
    const unsigned jitter=period/5;
    const auto target_ms=period-jitter+(jitter ? random%(jitter*2+1):0);
    if(elapsed_ms<target_ms) Sleep(static_cast<DWORD>(target_ms-elapsed_ms));
  }
  modules(output.value,pid);
  std::snprintf(line,sizeof(line),"F\t%lld\t%u\t%u\t%u\n",qpc(),samples,missing,cleanup_errors);
  if(!write(output.value,line)) return 5;
  std::printf("samples=%u missing=%u cleanup_errors=%u main_tid=%lu; wall-clock IP observations, not on-CPU shares\n",samples,missing,cleanup_errors,main_tid);
  return samples && !cleanup_errors?0:6;
}

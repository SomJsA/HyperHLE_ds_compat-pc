#!/usr/bin/env python3
"""Archive stable native Dead Space checkpoints while a game process runs.

This preserves game-written .sb files; it is NOT a CPU/GPU save state.
No input is sent and no live save is modified. Each archive is compatible
with ds-save.ps1's Documents/var layout. Stop with Ctrl+C or game exit.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import time


def read_set(var):
    return {p.name: p.read_bytes() for p in sorted(var.glob('*.sb'))}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def checkpoint_key(files):
    return tuple((name, digest(data)) for name, data in sorted(files.items())
                 if name.endswith('_CheckpointSave.sb'))


def alive(pid):
    if os.name != 'nt':
        try:
            os.kill(pid, 0)
            return True
        except ProcessLookupError:
            return False
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.OpenProcess.argtypes = [ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
    kernel.OpenProcess.restype = ctypes.c_void_p
    kernel.GetExitCodeProcess.argtypes = [ctypes.c_void_p,
                                        ctypes.POINTER(ctypes.c_ulong)]
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    handle = kernel.OpenProcess(0x1000, False, pid)
    if not handle:
        if ctypes.get_last_error() == 5:
            raise PermissionError(f'Cannot inspect process {pid}')
        return False
    try:
        code = ctypes.c_ulong()
        if not kernel.GetExitCodeProcess(handle, ctypes.byref(code)):
            raise ctypes.WinError(ctypes.get_last_error())
        return code.value == 259
    finally:
        kernel.CloseHandle(handle)


def archive(bank, files, runtime, pid):
    # Every file is read into memory before writing; archives never link to
    # mutable live files. Publish the directory only after all writes finish.
    stamp = time.strftime('%Y%m%d-%H%M%S')
    tag = digest(repr(checkpoint_key(files)).encode())[:10]
    dest = bank / f'{stamp}-{time.time_ns() % 1_000_000_000:09d}-{tag}'
    pending = bank / (dest.name + '.partial')
    out = pending / 'Documents' / 'var'
    out.mkdir(parents=True)
    manifest = {'kind': 'native-game-checkpoint', 'captured_at': time.time(),
                'runtime': str(runtime), 'process_id': pid, 'files': {}}
    for name, data in files.items():
        (out / name).write_bytes(data)
        manifest['files'][name] = {'bytes': len(data), 'sha256': digest(data)}
    (pending / 'manifest.json').write_text(
        json.dumps(manifest, indent=2), encoding='utf-8')
    pending.rename(dest)
    print(json.dumps({'archived': str(dest), 'checkpoints': dict(checkpoint_key(files))}),
          flush=True)
    return dest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', type=Path, required=True)
    parser.add_argument('--bank', type=Path, required=True)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--interval', type=float, default=0.5)
    args = parser.parse_args()
    if args.interval < 0.1:
        parser.error('--interval must be >= 0.1 seconds')
    var = args.runtime / 'touchHLE_sandbox/com.ea.deadspace.bv/Documents/var'
    if not var.is_dir():
        parser.error(f'No native saves at {var}')
    args.bank.mkdir(parents=True, exist_ok=True)
    previous = None
    archived_key = None
    print(f'Watching native checkpoints for PID {args.pid}; no live writes.', flush=True)
    while alive(args.pid):
        try:
            current = read_set(var)
        except (PermissionError, FileNotFoundError) as error:
            print(f'Retrying unstable save files: {error}', flush=True)
            previous = None
            time.sleep(args.interval)
            continue
        key = checkpoint_key(current)
        # Require the whole set to agree across two observations. Ignore
        # transient truncation and don't create archives for survival timers.
        if (key and key != archived_key and current == previous
                and all(data.startswith(b'SBIN') for data in current.values())):
            archive(args.bank, current, args.runtime, args.pid)
            archived_key = key
        previous = current
        time.sleep(args.interval)
    print('Game process exited; checkpoint capture stopped.', flush=True)


if __name__ == '__main__':
    try:
        main()
    except KeyboardInterrupt:
        print('Checkpoint capture stopped.', flush=True)

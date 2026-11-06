#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Inspect a Dead Space emulator save-state (`.dsstate`) offline.

Format (little-endian), produced by `src/ds_savestate.rs`:

    magic        [u8; 8]   b"DSSTATE1"
    version      u32
    region_count u32
    region[n]    { base: u32, size: u32, bytes: [u8; size] }
    regs         [u32; 16]
    cpsr         u32

Usage:
    python dsstate.py <file.dsstate> summary
    python dsstate.py <file.dsstate> strings [minlen]
    python dsstate.py <file.dsstate> find <ascii-or-0xhex>
    python dsstate.py <file.dsstate> hexdump <addr> <len>
    python dsstate.py <file.dsstate> regs
    python dsstate.py <file.dsstate> sbextract [outdir]
    python dsstate.py <file.dsstate> strtab [min_items]
    python dsstate.py <file.dsstate> region <index> [outfile]
"""
import io
import os
import struct
import sys

MAGIC = b"DSSTATE1"
SB_TAGS = {b"SBIN", b"STRU", b"FIEL", b"ENUM", b"OHDR", b"DATA", b"STRS", b"SYMB"}


class State:
    def __init__(self, path):
        self.path = path
        self.regions = []          # (base, bytes)
        self.regs = []
        self.cpsr = 0
        with io.open(path, "rb") as f:
            magic = f.read(8)
            if magic != MAGIC:
                raise SystemExit("not a .dsstate file (bad magic: %r)" % magic)
            (version,) = struct.unpack("<I", f.read(4))
            if version != 1:
                raise SystemExit("unsupported version %d" % version)
            (count,) = struct.unpack("<I", f.read(4))
            for _ in range(count):
                base, size = struct.unpack("<II", f.read(8))
                data = f.read(size)
                if len(data) != size:
                    raise SystemExit(
                        "truncated region at base=0x%x want=%d got=%d" % (base, size, len(data)))
                self.regions.append((base, data))
            raw = f.read(16 * 4)
            self.regs = list(struct.unpack("<16I", raw))
            (self.cpsr,) = struct.unpack("<I", f.read(4))

    def find(self, needle):
        hits = []
        for base, data in self.regions:
            start = 0
            while True:
                i = data.find(needle, start)
                if i < 0:
                    break
                hits.append((base + i, i))
                start = i + 1
        return hits

    def read(self, addr, length):
        for base, data in self.regions:
            if base <= addr < base + len(data):
                off = addr - base
                return data[off:off + length]
        return b""


def main(argv):
    if len(argv) < 3:
        print(__doc__)
        return 1
    st = State(argv[1])
    cmd = argv[2]
    if cmd == "summary":
        total = sum(len(d) for _, d in st.regions)
        print("file      : %s" % st.path)
        print("regions   : %d" % len(st.regions))
        print("guest bytes: %d (%.1f MB)" % (total, total / 1048576.0))
        if st.regions:
            lo = min(b for b, _ in st.regions)
            hi = max(b + len(d) for b, d in st.regions)
            print("addr range : 0x%08x .. 0x%08x" % (lo, hi))
        print("cpsr      : 0x%08x" % st.cpsr)
        print("regs(r0-15): " + " ".join("%08x" % r for r in st.regs))
        print("\ntop 10 regions by size:")
        for base, data in sorted(st.regions, key=lambda r: -len(r[1]))[:10]:
            print("  0x%08x  %9d bytes" % (base, len(data)))
    elif cmd == "regs":
        names = ["r%d" % i for i in range(13)] + ["sp", "lr", "pc"]
        for n, v in zip(names, st.regs):
            print("%-3s = 0x%08x" % (n, v))
        print("cpsr = 0x%08x" % st.cpsr)
    elif cmd == "strings":
        minlen = int(argv[3]) if len(argv) > 3 else 6
        seen = set()
        for base, data in st.regions:
            cur = []
            for i, byte in enumerate(data):
                if 32 <= byte < 127:
                    cur.append(chr(byte))
                else:
                    if len(cur) >= minlen:
                        s = "".join(cur)
                        key = (base + i - len(cur), s)
                        if s not in seen:
                            seen.add(s)
                            print("0x%08x  %s" % key)
                    cur = []
    elif cmd == "find":
        arg = argv[3]
        needle = bytes.fromhex(arg[2:]) if arg.startswith("0x") else arg.encode("utf-8")
        hits = st.find(needle)
        print("%d hit(s) for %r" % (len(hits), needle))
        for addr, _ in hits[:60]:
            print("  0x%08x" % addr)
    elif cmd == "hexdump":
        addr = int(argv[3], 0)
        length = int(argv[4], 0) if len(argv) > 4 else 128
        data = st.read(addr, length)
        if not data:
            print("address 0x%08x not in any region" % addr)
            return 2
        for off in range(0, len(data), 16):
            chunk = data[off:off + 16]
            hexs = " ".join("%02x" % b for b in chunk)
            text = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
            print("%08x  %-47s  %s" % (addr + off, hexs, text))
    elif cmd == "region":
        idx = int(argv[3])
        base, data = st.regions[idx]
        out = argv[4] if len(argv) > 4 else "region_%d_%08x.bin" % (idx, base)
        with io.open(out, "wb") as f:
            f.write(data)
        print("wrote %s (%d bytes) base=0x%08x" % (out, len(data), base))
    elif cmd == "strtab":
        # The .sb save format stores lists as: u32 count, then count x (u32 len,
        # bytes). The runtime keeps the same lists in guest memory, so scanning
        # for that layout finds the app's item / name tables inside a snapshot.
        min_items = int(argv[3]) if len(argv) > 3 else 4
        found = 0
        for base, data in st.regions:
            n = len(data)
            i = 0
            while i + 4 <= n:
                (count,) = struct.unpack_from("<I", data, i)
                if 2 <= count <= 4096 and i + 4 + count * 4 <= n:
                    off = i + 4
                    items = []
                    ok = True
                    for _ in range(count):
                        if off + 4 > n:
                            ok = False
                            break
                        (ln,) = struct.unpack_from("<I", data, off)
                        off += 4
                        if ln == 0 or ln > 200 or off + ln > n:
                            ok = False
                            break
                        raw = data[off:off + ln]
                        off += ln
                        if any(b < 9 or (13 < b < 32) or b >= 127 for b in raw):
                            ok = False
                            break
                        items.append(raw.decode("ascii", "replace"))
                    if ok and len(items) >= min_items:
                        found += 1
                        print("0x%08x  count=%d  [%s]" % (base + i, len(items),
                                                          ", ".join(items[:14])))
                        if found >= 40:
                            print("... (stopping after 40 tables)")
                            return 0
                i += 4
        print("total tables: %d" % found)
    elif cmd == "sbextract":
        # The app keeps its loaded `.sb` files in guest memory verbatim. Walk
        # the SBIN container (tag + u32 len + u32 hash + payload, padded to 4)
        # and rebuild the file so it can be fed straight to tools/sb.py.
        outdir = argv[3] if len(argv) > 3 else "sbextract"
        if not os.path.isdir(outdir):
            os.makedirs(outdir)
        n = 0
        for base, data in st.regions:
            start = 0
            while True:
                i = data.find(b"SBIN", start)
                if i < 0:
                    break
                start = i + 1
                # verify this looks like a real container: version then a known tag
                if i + 12 > len(data):
                    continue
                version = struct.unpack_from("<I", data, i + 4)[0]
                if version not in (1, 2, 3):
                    continue
                if data[i + 8:i + 12] not in SB_TAGS:
                    continue
                pos = i + 8
                sections = []
                while pos + 12 <= len(data):
                    tag = data[pos:pos + 4]
                    if tag not in SB_TAGS:
                        break
                    ln, h = struct.unpack_from("<II", data, pos + 4)
                    if ln > 4 * 1024 * 1024 or pos + 12 + ln > len(data):
                        break
                    sections.append((tag.decode(), ln))
                    pos += 12 + ln
                    pos = (pos + 3) & ~3
                if len(sections) < 3:
                    continue
                blob = data[i:pos]
                n += 1
                fn = os.path.join(outdir, "sb_%08x.sb" % (base + i))
                with io.open(fn, "wb") as f:
                    f.write(blob)
                print("0x%08x  %6d bytes  v%d  sections=%s  -> %s"
                      % (base + i, len(blob), version,
                         ",".join("%s(%d)" % s for s in sections[:8]), fn))
        print("total .sb containers: %d" % n)
    else:
        print(__doc__)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

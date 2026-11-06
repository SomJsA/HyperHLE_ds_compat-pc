"""Disassemble an armv7 Mach-O function by symbol substring or address.

Usage:
  python arm-dis.py <bin> <symbol-substr> [arm|thumb] [max_insns]
  python arm-dis.py <bin> @0xADDR [arm|thumb] [max_insns]
"""
import bisect, re, struct, sys
from capstone import Cs, CS_ARCH_ARM, CS_MODE_ARM, CS_MODE_THUMB

MH_MAGIC = 0xfeedface
LC_SEGMENT = 1
LC_SYMTAB = 2

def parse(path):
    data = open(path, 'rb').read()
    ncmds = struct.unpack_from('<I', data, 16)[0]
    off = 28; segs = []; symtab = None
    for _ in range(ncmds):
        cmd, cmdsize = struct.unpack_from('<II', data, off)
        if cmd == LC_SEGMENT:
            segname = data[off+8:off+24].split(b'\0')[0].decode('latin1')
            vmaddr, vmsize, fileoff, filesize = struct.unpack_from('<IIII', data, off+24)
            segs.append((segname, vmaddr, vmsize, fileoff, filesize))
        elif cmd == LC_SYMTAB:
            symtab = struct.unpack_from('<IIII', data, off+8)
        off += cmdsize
    symoff, nsyms, stroff, strsize = symtab
    syms = []
    for i in range(nsyms):
        o = symoff + i*12
        n_strx, n_type, n_sect, n_desc, n_value = struct.unpack_from('<IBBHI', data, o)
        if n_strx >= strsize or not n_value:
            continue
        e = data.find(b'\0', stroff+n_strx, stroff+strsize)
        name = data[stroff+n_strx:e].decode('utf-8', 'replace')
        syms.append((n_value, name, n_type, n_sect, n_desc))
    syms.sort()
    return data, segs, syms

def va_to_off(segs, va):
    for name, vmaddr, vmsize, fileoff, filesize in segs:
        if vmaddr <= va < vmaddr + vmsize and va - vmaddr < filesize:
            return fileoff + (va - vmaddr)
    raise SystemExit('VA not mapped: %#x' % va)

def main():
    path, target = sys.argv[1], sys.argv[2]
    mode_s = sys.argv[3].lower() if len(sys.argv) > 3 else 'arm'
    lim = int(sys.argv[4]) if len(sys.argv) > 4 else 180
    data, segs, syms = parse(path)
    if target.startswith('@'):
        va = int(target[1:], 0)
        name = '(address)'
    else:
        hits = [(v,n,t,s,d) for v,n,t,s,d in syms if target.lower() in n.lower()]
        if not hits:
            raise SystemExit('symbol not found: ' + target)
        print('SYMBOL HITS:')
        for v,n,t,s,d in hits[:20]:
            print('  %08x type=%02x sect=%d desc=%04x %s' % (v,t,s,d,n))
        va, name, _, _, _ = hits[0]
    fo = va_to_off(segs, va)
    md = Cs(CS_ARCH_ARM, CS_MODE_THUMB if mode_s.startswith('t') else CS_MODE_ARM)
    md.detail = False
    code = data[fo:fo+lim*4]
    print('FUNCTION %s VA=%08x FILE=%08x MODE=%s' % (name, va, fo, mode_s))
    n = 0
    for insn in md.disasm(code, va & ~1):
        print('%08x  %-10s %s' % (insn.address, insn.mnemonic, insn.op_str))
        n += 1
        if n >= lim:
            break

if __name__ == '__main__':
    main()

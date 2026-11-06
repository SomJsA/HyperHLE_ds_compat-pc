"""Read-only PC input diagnostics for a user-marked F5 snapshot."""
import argparse
import bisect
import importlib.util
import json
import struct
import sys
from pathlib import Path

ap = argparse.ArgumentParser()
ap.add_argument("snapshot", type=Path)
ap.add_argument("output", type=Path)
ap.add_argument("--project-root", type=Path, default=Path(__file__).resolve().parents[3])
args = ap.parse_args()
sys.path.insert(0, str(args.project_root / "tools"))
from dsstate import State

spec = importlib.util.spec_from_file_location("arm", args.project_root / "tools/arm-dis.py")
arm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(arm)
binary, segments, symbols = arm.parse(str(args.project_root / "device-app/extracted/Dead Space.app/Dead Space"))
names = {a: n for a, n, *_ in symbols}
addresses = sorted(names)
state = State(args.snapshot)
state.regions.sort()
bases = [a for a, _ in state.regions]


def read(address, size):
    i = bisect.bisect_right(bases, address) - 1
    if i >= 0:
        base, data = state.regions[i]
        offset = address - base
        if 0 <= offset and offset + size <= len(data):
            return data[offset:offset + size]
    for _, va, vs, fo, fs in segments:
        if va <= address and address + size <= va + fs:
            return binary[fo + address - va:fo + address - va + size]
    return bytes(size)


def u(address):
    return struct.unpack("<I", read(address, 4))[0]


def name(address):
    i = bisect.bisect_right(addresses, address) - 1
    return names[addresses[i]] if i >= 0 else "?"


rows = []
for fwd, _ in state.find(struct.pack("<I", 0xa7d1e8)):
    if fwd % 4:
        continue
    owner, regions = u(fwd + 8), u(fwd + 0x10)
    if not owner or not regions:
        continue
    begin, end = u(regions + 12), u(regions + 16)
    if not begin or not 0 < end - begin <= 4096:
        continue
    shapes = []
    for p in range(begin, min(end, begin + 64), 8):
        region = u(p)
        vptr = u(region)
        if vptr not in (0xa62ca8, 0xa63278):
            continue
        x, y, w = struct.unpack("<iii", read(region + 12, 12))
        h = struct.unpack("<i", read(region + 24, 4))[0] if vptr == 0xa63278 else w
        shapes.append(dict(address=hex(region), enabled=read(region + 8, 1)[0],
                           shape="circle" if vptr == 0xa62ca8 else "box", geometry=[x, y, w, h]))
    if not shapes:
        continue
    vp = u(owner)
    rows.append(dict(forwarder=hex(fwd), code=hex(u(fwd + 28)), enabled=read(fwd + 4, 1)[0],
                     owner=hex(owner), owner_enabled=read(owner + 0x164, 1)[0],
                     owner_type=name(vp), is_telekinetic_method=name(u(vp + 0x58)),
                     list_enabled=read(regions + 8, 1)[0], shapes=shapes))

players = []
for player, _ in state.find(struct.pack("<I", 0xa7a4f8)):
    if player % 4:
        continue
    players.append(dict(address=hex(player), state=u(player + 0x208), grapple=u(player + 0x37c),
                        swarm=u(player + 0x3f0), tap_event=hex(u(player + 0x3ec))))
result = dict(snapshot=str(args.snapshot), players=players, forwarders=rows)
args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(dict(output=str(args.output), players=players, forwarder_count=len(rows))))

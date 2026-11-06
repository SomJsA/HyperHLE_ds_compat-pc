#!/usr/bin/env python3
"""Execute original ARM station entry gates; no host gameplay-state writes.

Usage: python verify-native-stations.py <original-game-binary> [--out report.json]
Requires Unicorn. Store geometry functions run unchanged. Bench range and world
combat/tweak providers are isolated external dependencies, explicitly reported.
"""
import argparse
import hashlib
import json
import struct
from pathlib import Path
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_ARM, UC_HOOK_CODE
from unicorn.arm_const import (
    UC_ARM_REG_C1_C0_2, UC_ARM_REG_FPEXC, UC_ARM_REG_R0, UC_ARM_REG_R1,
    UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_SP,
)


def segments(binary):
    assert struct.unpack_from("<I", binary)[0] == 0xFEEDFACE
    offset = 28
    for _ in range(struct.unpack_from("<I", binary, 16)[0]):
        command, length = struct.unpack_from("<II", binary, offset)
        if command == 1:
            name = binary[offset + 8:offset + 24].split(b"\0")[0]
            address, size, start, count = struct.unpack_from("<4I", binary, offset + 24)
            if name != b"__PAGEZERO" and size:
                yield address, size, binary[start:start + count]
        offset += length


def machine(binary):
    u = Uc(UC_ARCH_ARM, UC_MODE_ARM)
    for address, size, content in segments(binary):
        base = address & ~4095
        u.mem_map(base, ((address + size + 4095) & ~4095) - base)
        if content:
            u.mem_write(address, content)
    for address in (0x10000000, 0x20000000, 0x30000000):
        u.mem_map(address, 0x20000)
    u.reg_write(UC_ARM_REG_C1_C0_2, 0xF00000)
    u.reg_write(UC_ARM_REG_FPEXC, 0x40000000)
    return u


def put(u, address, value):
    u.mem_write(address, struct.pack("<I", value))


def fp(u, address, value):
    u.mem_write(address, struct.pack("<f", value))


def execute(u, entry, actor):
    u.reg_write(UC_ARM_REG_R0, actor)
    # Both verified overrides ignore this argument. Never generalize this to
    # GameObjectInteractive: other classes use the interaction-node transform.
    u.reg_write(UC_ARM_REG_R1, 0)
    u.reg_write(UC_ARM_REG_SP, 0x20010000)
    u.reg_write(UC_ARM_REG_LR, 0x30000000)
    u.emu_start(entry, 0x30000000, count=3000)
    assert u.reg_read(UC_ARM_REG_PC) == 0x30000000
    return u.reg_read(UC_ARM_REG_R0)


def store(binary, phase, enemies, pz, forward):
    u = machine(binary)
    actor, player, st, pt, vt, world, tweaks = [
        0x10001000, 0x10002000, 0x10004000, 0x10005000,
        0x10006000, 0x10007000, 0x10008000,
    ]
    for obj, transform in ((actor, st), (player, pt)):
        put(u, obj, vt)
        put(u, obj + 0x4C, transform)
        fp(u, transform + 0x10, 1)
        fp(u, transform + 0x24, 1)
        fp(u, transform + 0x38, 1)
    put(u, vt + 0xC0, 0x3A310)
    put(u, actor + 0x1D0, phase)
    put(u, actor + 0x10, world)
    put(u, world + 0xE0, player)
    fp(u, pt + 0x48, pz)
    fp(u, pt + 0x18, forward[0])
    fp(u, pt + 0x38, forward[1])
    fp(u, tweaks + 0xDFC, 3)
    fp(u, tweaks + 0xE00, 0.5)

    def hook(uc, address, _size, _data):
        if address in (0x7E448, 0xDD18):
            uc.reg_write(UC_ARM_REG_R0, tweaks if address == 0x7E448 else enemies)
            uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))
    u.hook_add(UC_HOOK_CODE, hook)
    result = execute(u, 0x9165C, actor)
    expected = int(phase == 0 and not enemies and 0 < pz < 3 and forward == (0, -1))
    assert result == expected, (phase, enemies, pz, forward, result)
    return dict(kind="store", phase=phase, enemies=enemies, pz=pz,
                forward=forward, accepted=result)


def bench(binary, phase, in_range, enemies):
    u = machine(binary)
    actor, world = 0x10001000, 0x10007000
    put(u, actor + 0x214, phase)
    put(u, actor + 0x10, world)

    def hook(uc, address, _size, _data):
        if address in (0xB59FC, 0xDD18):
            uc.reg_write(UC_ARM_REG_R0, in_range if address == 0xB59FC else enemies)
            uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))
    u.hook_add(UC_HOOK_CODE, hook)
    result = execute(u, 0xB5B84, actor)
    assert result == int(phase in (1, 2) and in_range and not enemies)
    return dict(kind="bench", phase=phase, in_range=in_range,
                enemies=enemies, accepted=result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    binary = args.binary.read_bytes()
    cases = [store(binary, *case) for case in (
        (0, 0, 1.4, (0, -1)), (0, 1, 1.4, (0, -1)),
        (0, 0, -1.4, (0, 1)), (0, 0, 1.4, (1, 0)),
        (0, 0, 5, (0, -1)), (1, 0, 1.4, (0, -1)),
        (4, 0, 1.4, (0, -1)),
    )]
    cases += [bench(binary, phase, radius, enemy)
              for phase in (0, 1, 2, 3) for radius in (0, 1) for enemy in (0, 1)]
    report = dict(binary_sha256=hashlib.sha256(binary).hexdigest(), cases=cases,
                  scope="Original CanInteract bodies. Store geometry runs unchanged; "
                        "Tweaks/world combat and Bench IsPlayerInRange providers are isolated. "
                        "Not a full UI, purchase, walking or chapter progression acceptance.")
    if args.out:
        args.out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"PASS {len(cases)} original ARM Store/Bench entry gates")


if __name__ == "__main__":
    main()

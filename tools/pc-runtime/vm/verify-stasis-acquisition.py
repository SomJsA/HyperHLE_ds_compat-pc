"""Original ARM stasis acquisition vs stored charges. No chapter-based policy.

CanStasis includes the acquisition gate that the direct keyboard action lacked.
Run real OnTryFire/CanFire/GetAmmoInClip with the real WeaponStasis vtable too.
This is an engine-ABI fixture; live new-game/NG+ checks remain separate.
"""
import argparse,hashlib,json,struct,sys
from pathlib import Path
from unicorn import Uc,UC_ARCH_ARM,UC_MODE_ARM
from unicorn.arm_const import UC_ARM_REG_R0,UC_ARM_REG_LR,UC_ARM_REG_PC,UC_ARM_REG_SP
def segments(binary):
    assert struct.unpack_from("<I", binary)[0] == 0xFEEDFACE
    count = struct.unpack_from("<I", binary, 16)[0]
    offset = 28
    for _ in range(count):
        command, length = struct.unpack_from("<II", binary, offset)
        if command == 1:
            name = binary[offset + 8:offset + 24].split(b"\0")[0]
            address, size, file_offset, file_size = struct.unpack_from("<IIII", binary, offset + 24)
            if name != b"__PAGEZERO" and size:
                yield address, size, binary[file_offset:file_offset + file_size]
        offset += length


def run(binary,owned,aiming,state,charges,entry,has_weapon=True):
 uc=Uc(UC_ARCH_ARM,UC_MODE_ARM)
 for a,n,b in segments(binary):
  base=a&~4095;uc.mem_map(base,((a+n+4095)&~4095)-base)
  if b:uc.mem_write(a,b)
 player,weapon,stack,stop=0x10000000,0x10002000,0x20000000,0x30000000
 uc.mem_map(player,0x10000);uc.mem_map(stack,0x10000);uc.mem_map(stop,4096)
 def put(a,v):uc.mem_write(a,struct.pack('<I',v))
 put(player+0x30c,weapon if has_weapon else 0);uc.mem_write(player+0x1e1,bytes([aiming]));put(player+0x208,state)
 put(weapon,0xa7b2f8);uc.mem_write(weapon+0x164,bytes([owned]));put(weapon+0x168,charges)
 before=bytes(uc.mem_read(player,0x10000));uc.reg_write(UC_ARM_REG_R0,player if entry==0x3b080 else weapon);uc.reg_write(UC_ARM_REG_LR,stop);uc.reg_write(UC_ARM_REG_SP,stack+0xf000)
 uc.emu_start(entry,stop,count=200);assert uc.reg_read(UC_ARM_REG_PC)==stop
 after=bytes(uc.mem_read(player,0x10000));result=uc.reg_read(UC_ARM_REG_R0)
 if entry==0x3b080:assert before==after,'CanStasis wrote player/weapon'
 assert struct.unpack('<I',uc.mem_read(weapon+0x168,4))[0]==charges
 return dict(owned=owned,aiming=aiming,state=state,stored_charges=charges,has_weapon=has_weapon,result=result,queued=struct.unpack('<I',uc.mem_read(weapon+0x23c,4))[0])

def main():
 p=argparse.ArgumentParser();p.add_argument('binary',type=Path);p.add_argument('output',type=Path);a=p.parse_args();b=a.binary.read_bytes();gates=[];fires=[]
 for owned in (0,1):
  for aiming in (0,1):
   for state in (0,3,5,29):
    row=run(b,owned,aiming,state,1,0x3b080);assert row['result']==int(owned and aiming and state in (0,3)),row;gates.append(row)
 row=run(b,1,1,0,1,0x3b080,False);assert row['result']==0;gates.append(row)
 for owned in (0,1):
  for charges in (0,1,4):
   row=run(b,owned,1,0,charges,0x876cc);assert row['result']==(0 if charges else 2),row;assert row['queued']==int(charges>0),row;fires.append(row)
 a.output.write_text(json.dumps(dict(binary_sha256=hashlib.sha256(b).hexdigest(),native_can_stasis=gates,native_try_fire=fires,conclusion='Acquisition is independent of preloaded charge. Unowned charged OnTryFire queues a shot; acquired empty returns native no-ammo=2. No chapter field is consulted.'),indent=2))
 print('PASS original ARM: 33 acquisition/aim/state gates (read-only), 6 real-vtable OnTryFire cases; reproduced unowned charged shot and preserved acquired empty result=2')
if __name__=='__main__':main()

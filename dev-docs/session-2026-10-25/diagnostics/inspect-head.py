import sys,struct,bisect,importlib.util,json,re
from pathlib import Path
root=Path(r'E:\CodexProjects\DeadSpaceLegacy');sys.path.insert(0,str(root/'tools'))
from dsstate import State
sp=importlib.util.spec_from_file_location('arm',root/'tools/arm-dis.py');arm=importlib.util.module_from_spec(sp);sp.loader.exec_module(arm)
data,segs,syms=arm.parse(str(root/'device-app/extracted/Dead Space.app/Dead Space'))
names={a:n for a,n,*_ in syms};addrs=sorted(names)
def sym(a):
 i=bisect.bisect_right(addrs,a)-1
 return names[addrs[i]]+(' +%x'%(a-addrs[i]) if a!=addrs[i] else '') if i>=0 else '?'
st=State(sys.argv[1]);st.regions.sort();bases=[a for a,d in st.regions]
def read(a,n):
 out=b''
 while n:
  i=bisect.bisect_right(bases,a)-1
  if i>=0:
   b,d=st.regions[i];off=a-b
   if off<len(d):
    part=d[off:off+n];out+=part;a+=len(part);n-=len(part);continue
  try:
   off=arm.va_to_off(segs,a);return out+data[off:off+n]
  except SystemExit:return out+b'\0'*n
 return out
def u(a):return struct.unpack('<I',read(a,4))[0]
def b(a):return read(a,1)[0]
def wide(a):
 v=[]
 for k in range(100):
  c=u(a+k*4)
  if not c:break
  if c>127:return '?'
  v.append(chr(c))
 return ''.join(v)
print('FILE',Path(sys.argv[1]).name)
for player,_ in st.find(struct.pack('<I',0xa7a4f8)):
 if player%4:continue
 print('PLAYER',hex(player),'state',u(player+0x208),'suit',u(player+0x550),'carrie',b(player+0x560),'model',hex(u(player+0x44)))
 print('carrie fields',[(hex(o),hex(u(player+o))) for o in range(0x564,0x580,4)])
for a in [0x3da90+8+0x8eb9fd,0x3db60+8+0x8eb92d,0x3dc38+8+0x88d588,0x3dcd8+8+0x88d558]:print('CONST',hex(a),read(a,80).split(b'\0')[0],wide(a))
table=0x3d988+8+0xa3ca40
print('TABLE',hex(table))
for i in range(12):
 a=table+i*24;print(i,hex(u(a)),wide(u(a)),hex(u(a+4)),wide(u(a+4)))

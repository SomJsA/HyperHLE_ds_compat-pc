exec(open(r'E:\CodexProjects\DeadSpaceLegacy\logs\ending-face-20261025\inspect-head.py',encoding='utf-8-sig').read().split("print('FILE'")[0])
for name in ['MESH_isaac_suit_helmet_04','nodim_eyes_level_04']:
 print('\nNAME',name)
 for a,_ in st.find(name.encode('utf-32-le')):
  if a<0x10000000:continue
  refs=[p for p,_ in st.find(struct.pack('<I',a)) if p%4==0]
  print('string',hex(a),'refs',[hex(x) for x in refs[:12]])
  for p in refs[:8]:
   vals=[(hex(o),hex(u(p+o)),sym(u(p+o))) for o in [-24,-20,-16,-12,-8,-4,0,4,8,12,16,20,24]]
   print(vals)

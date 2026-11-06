exec(open(r'E:\CodexProjects\DeadSpaceLegacy\logs\ending-face-20261025\inspect-head.py',encoding='utf-8-sig').read().split("print('FILE'")[0])
for name in ['MESH_isaac_suit_helmet_04','nodim_eyes_level_04','carrie_head_sidepart_carrie_face']:
 for a,_ in st.find(name.encode('utf-32-le')):
  if a<0x10000000:continue
  for ref,_ in st.find(struct.pack('<I',a)):
   if ref%4:continue
   for back in range(0,0x80,4):
    obj=ref-back;v=u(obj);n=sym(v)
    if n.startswith('__ZTVN3m3g'):
     print(name,hex(obj),'nameoff',hex(back),n,'render?',b(obj+0x4c),'parent?',hex(u(obj+0x48)));break
for a in [0x316d1ba0,0x316d4ae0,0x36b30c80,u(0x36b30c80+8)]:
 print('obj',hex(a),sym(u(a)),'words',[hex(u(a+x)) for x in range(0,0x60,4)])

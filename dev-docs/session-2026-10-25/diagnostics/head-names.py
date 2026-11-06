exec(open(r'E:\CodexProjects\DeadSpaceLegacy\logs\ending-face-20261025\inspect-head.py',encoding='utf-8-sig').read().split("print('FILE'")[0])
for key in ['MESH_isaac_suit_helmet_04','nodim_eyes_level_04','carrie_head_sidepart_carrie_face','isaac_suit_04']:
 for enc in ['ascii','utf-32-le']:
  hits=st.find(key.encode(enc));print(key,enc,[hex(a) for a,_ in hits[:12]],'total',len(hits))
for a,n in names.items():
 if ('getName' in n or 'GetName' in n or 'isRendering' in n or 'IsRendering' in n or 'setRendering' in n or 'FindNode' in n) and ('m3g' in n or 'Mesh' in n):print(hex(a),n)

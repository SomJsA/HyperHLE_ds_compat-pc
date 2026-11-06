exec(open(r'E:\CodexProjects\DeadSpaceLegacy\logs\ending-face-20261025\inspect-head.py',encoding='utf-8-sig').read().split("print('FILE'")[0])
# Walk the live player's scene tree, then enumerate only its actual submeshes.
rootnode=u(u(0x370f04a0+0x44)+8);seen=set();todo=[rootnode]
while todo:
 a=todo.pop()
 if a in seen:continue
 seen.add(a);typ=sym(u(a))
 if 'Group' in typ or 'SkinnedMesh' in typ:
  begin,end=u(a+0x168),u(a+0x16c)
  if begin and 0<=end-begin<=10000:todo += [u(p) for p in range(begin,end,4)]
sv=[a+8 for a,n in names.items() if n=='__ZTVN3m3g7SubmeshE'][0]
rows=[]
for a,_ in st.find(struct.pack('<I',sv)):
 if a%4 or u(a+0x48) not in seen:continue
 name=wide(u(a+0x28));app=u(a+0x40);mat=wide(u(app+0x28))
 if 'helmet' in name or 'body' in name:rows.append(dict(address=hex(a),name=name,appearance=mat,visible=b(a+0x4c)))
table=0xa7a3d0;targets={(wide(u(table+i*24)),wide(u(table+i*24+4))) for i in range(12) if 0xfaa&(1<<i)}
for row in rows:row['complete_cleanup_selects']=(row['appearance'],row['name']) in targets
out={'snapshot':Path(sys.argv[1]).name,'carrie':b(0x370f04a0+0x560),'player':hex(0x370f04a0),'rows':rows}
Path(sys.argv[2]).write_text(json.dumps(out,indent=2),encoding='utf-8')
print(json.dumps(out))
assert all(not row['complete_cleanup_selects'] for row in rows if 'body' in row['name'])
assert all(row['complete_cleanup_selects'] for row in rows if 'helmet' in row['name'])

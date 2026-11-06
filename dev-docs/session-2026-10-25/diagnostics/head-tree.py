exec(open(r'E:\CodexProjects\DeadSpaceLegacy\logs\ending-face-20261025\inspect-head.py',encoding='utf-8-sig').read().split("print('FILE'")[0])
print('lookup symbol',[(hex(a),n) for a,n in names.items() if a in [0x6758,0x3b7ee8,0xc68c4,0xc67a8,0x4422b8]])
rootnode=u(0x36b30c80+8);seen=set();todo=[rootnode]
while todo:
 a=todo.pop()
 if a in seen:continue
 seen.add(a);typ=sym(u(a))
 if 'Group' in typ or 'SkinnedMesh' in typ:
  begin,end=u(a+0x168),u(a+0x16c)
  if begin and 0<=end-begin<=10000:todo += [u(p) for p in range(begin,end,4)]
 if a==0x31824360:print('confirmed active player tree includes helmet parent',hex(a),typ)
print('tree count',len(seen))

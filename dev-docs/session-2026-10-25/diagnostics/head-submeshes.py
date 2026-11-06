exec(open(r'E:\CodexProjects\DeadSpaceLegacy\logs\ending-face-20261025\inspect-head.py',encoding='utf-8-sig').read().split("print('FILE'")[0])
print('FILE',Path(sys.argv[1]).name)
for a in [0x319e6fb0,0x319e6610,0x31824360]:
 print('OBJECT',hex(a),sym(u(a)))
 for off in range(0x38,0x90,4):
  v=u(a+off)
  typ=sym(u(v)) if 0x30000000<v<0x50000000 else ''
  nm=wide(u(v+0x28)) if typ.startswith('__ZTVN3m3g') else ''
  print(hex(off),hex(v),typ if typ.startswith('__ZTV') else '',nm)

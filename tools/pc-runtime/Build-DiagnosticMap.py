"""Build a provenance-bound diagnostic map. Read-only inputs, new output only.
No game launch, device query, IPA rewriting or profile access.
"""
from pathlib import Path
import argparse,hashlib,json,re,subprocess,zipfile,struct,plistlib
def sha(path):
 h=hashlib.sha256()
 with path.open('rb') as stream:
  for block in iter(lambda:stream.read(1024*1024),b''):h.update(block)
 return h.hexdigest()
def import_stubs(data):
 def words(offset,n):return struct.unpack_from('<'+'I'*n,data,offset)
 header=words(0,7)
 if header[0]!=0xfeedface:raise ValueError('Unsupported executable format; no guessed symbol map')
 offset=28;sections=[];sym=None;dysym=None
 for _ in range(header[4]):
  cmd,size=words(offset,2)
  if size<8 or offset+size>len(data):raise ValueError('Invalid MachO command bounds')
  if cmd==1:
   for i in range(words(offset+48,1)[0]):
    at=offset+56+i*68;v=words(at+32,9)
    if v[6]&255==8:sections.append((v[0],v[1],v[7],v[8]))
  elif cmd==2:sym=words(offset,6)
  elif cmd==0xb:dysym=words(offset,20)
  offset+=size
 if not sym or not dysym:raise ValueError('Missing import symbol tables')
 result={}
 for address,size,start,stride in sections:
  if not stride or size%stride:raise ValueError('Invalid import stub stride')
  for i in range(size//stride):
   index=words(dysym[14]+(start+i)*4,1)[0]
   if index&0xc0000000:continue
   if index>=sym[3]:raise ValueError('Symbol index out of range')
   string_index=words(sym[2]+index*12,1)[0]
   if string_index>=sym[5]:raise ValueError('String index out of range')
   begin=sym[4]+string_index;end=data.index(b'\0',begin,sym[4]+sym[5]);name=data[begin:end].decode('utf-8',errors='replace')
   result[f'{address+i*stride:08x}']={'symbol':name,'stride':stride}
 return result

def build(repo,ipa,allow_dirty=False,artifact_exclude=None):
 dirty=subprocess.run(["git","status","--porcelain=v1","--untracked-files=no"],cwd=repo,capture_output=True,text=True,check=True).stdout.strip()
 exclusions=json.loads(artifact_exclude.read_text(encoding='utf-8-sig')) if artifact_exclude else {}
 unknown=[];frozen={}
 for rel in subprocess.run(['git','ls-files','--others','--exclude-standard','-z'],cwd=repo,capture_output=True,text=True,check=True).stdout.split('\0'):
  if not rel:continue
  path=repo/rel
  if rel in exclusions and path.is_file() and not path.is_symlink() and sha(path)==exclusions[rel]:frozen[rel]=exclusions[rel]
  else:unknown.append(rel)
 dirty=dirty or bool(unknown)
 if dirty and not allow_dirty:raise ValueError("Diagnostic release map requires a clean source tree")
 commit=subprocess.run(['git','rev-parse','HEAD'],cwd=repo,capture_output=True,text=True,check=True).stdout.strip()
 files={}
 for rel in ['src/frameworks/opengles/gles_guest.rs','src/frameworks/opengles/eagl.rs','src/gles/gles1_on_gl2.rs','src/ds_scene_diagnostics.rs','src/ds_stutter_probe.rs','src/environment.rs','src/cpu.rs','src/dyld.rs','src/fs/bundle.rs','src/image.rs']:
  path=repo/rel;lines=path.read_text(encoding='utf-8').splitlines()
  functions=[{'line':i+1,'name':m.group(1)} for i,line in enumerate(lines) if (m:=re.match(r'\s*(?:(?:pub(?:\([^)]*\))?|unsafe|extern "C")\s+)*fn\s+([A-Za-z0-9_]+)\s*(?:<|\()',line))]
  files[rel]={'sha256':sha(path),'function_starts':functions}
 with zipfile.ZipFile(ipa) as archive:
  info_name=next(n for n in archive.namelist() if n.startswith('Payload/') and n.endswith('.app/Info.plist'))
  info=plistlib.loads(archive.read(info_name));binary=archive.read(info_name.rsplit('/',1)[0]+'/'+info['CFBundleExecutable'])
  members=[{'index':i,'name':f.filename,'size':f.file_size,'compressed_bytes':f.compress_size} for i,f in enumerate(archive.infolist())]
 return {'schema':1,'implementation_commit':commit,'sources_clean':not bool(dirty),'frozen_untracked_exclusions':frozen,'guest_import_stubs':import_stubs(binary),'source_files':files,'ipa_sha256':sha(ipa),'ipa_members':members,
  'scope':'function start ranges aid source-line decoding; nested functions may make ranges approximate; actual source hash authoritative. IPA index is immutable archive index, not a host path.',
  'runtime_wall_span_kinds':{'1':'GLES wrapper caller line in gles_guest.rs','2':'glClear inclusive','3':'movement','4':'QTE native observe','5':'station discovery','6':'forwarder refresh','7':'overlay prepare','8':'card build','9':'GPU pass CPU submission','10':'GPU upload CPU submission','11':'present inclusive','12':'vptr memory sweep','13':'HLE callback inclusive; guest import SVC entry PC, cooperative execution may span many presents','14':'CPU run inclusive; entry PC is NOT hottest function or exclusive compiler time','15':'native action sender and opt-in gauges','16':'cached player action lookup+sender'},
  'scene_scope_kinds':'session-start.json scene_diagnostics kinds is authoritative zero-based enum for scene_slow_scope',
  'render_keys':'session-start.json documents explicitly partial tracked setter fields; no claim of full driver shader state or compilation'}
if __name__=='__main__':
 a=argparse.ArgumentParser();a.add_argument('--repo',type=Path,required=True);a.add_argument('--ipa',type=Path,required=True);a.add_argument('--output',type=Path,required=True);a.add_argument('--allow-dirty-preview',action='store_true');a.add_argument('--artifact-exclude',type=Path);args=a.parse_args()
 data=build(args.repo,args.ipa,args.allow_dirty_preview,args.artifact_exclude)
 with args.output.open('x',encoding='utf-8') as stream:json.dump(data,stream,ensure_ascii=True,separators=(',',':'))
 print(json.dumps({'output_bytes':args.output.stat().st_size,'sha256':sha(args.output),'ipa_members':len(data['ipa_members']),'source_files':len(data['source_files']),'import_stubs':len(data['guest_import_stubs']),'sources_clean':data['sources_clean']}))

"""Relocate a fully manifested flat candidate into a new English PC layout.

This is byte-preserving staging, not runtime/release acceptance. Never modifies
the input package, original IPA, user data or existing output directory.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil

def sha(path):
    digest=hashlib.sha256()
    with path.open('rb') as stream:
        for part in iter(lambda:stream.read(4*1024*1024),b''):digest.update(part)
    return digest.hexdigest()

def safe(root,name):
    relative=Path(name)
    if relative.is_absolute() or '..' in relative.parts or ':' in name or '\\' in name:
        raise ValueError('Unsafe manifest member')
    path=root/relative
    for part in (path,*path.parents):
        if part.is_symlink() or (part.exists() and getattr(part.lstat(),'st_file_attributes',0)&0x400):
            raise ValueError('Link in package path')
        if part==root:break
    if not path.resolve().is_relative_to(root):raise ValueError('Path outside package')
    return path

def destination(name):
    parts=Path(name).parts;first=parts[0]
    if name=='DeadSpace Launcher.exe':return name
    if first=='Defaults':return name
    if first=='texture-packs':return 'Assets/'+name
    if first=='assets':return 'Assets/launcher/'+Path(*parts[1:]).as_posix()
    if first in ('res','touchHLE_dylibs','touchHLE_fonts') or first.startswith('touchHLE') or name in ('DeadSpace.ipa','build-info.json','diagnostic-map.json'):
        return 'Game/'+name
    if name in ('DeadSpace-Launcher.ini','DeadSpace-Keys.ini','DeadSpace-Gamepad.ini','DeadSpace-VisualCompat.ini','overlay-style.json'):
        return 'Defaults/'+name
    return 'Docs/'+name

def stage(source,output):
    source=source.resolve();output=output.absolute()
    if output.exists() or output.is_symlink():raise ValueError('Output must be new')
    if output.is_relative_to(source) or source.is_relative_to(output):raise ValueError('Separate input and output required')
    manifest=json.loads((source/'manifest.json').read_text(encoding='utf-8-sig'))
    expected=manifest['files'];actual={p.relative_to(source).as_posix() for p in source.rglob('*') if p.is_file()}
    if actual!=set(expected)|{'manifest.json'}:raise ValueError('Unmanifested or missing input files')
    targets={}
    for name,digest in expected.items():
        path=safe(source,name)
        if sha(path)!=digest:raise ValueError('Input hash mismatch: '+name)
        target=destination(name)
        if target in targets and targets[target][1]!=digest:raise ValueError('Conflicting default: '+target)
        targets[target]=(path,digest)
    if 'DeadSpace Launcher.exe' not in targets or 'Game/touchHLE.exe' not in targets or 'Game/DeadSpace.ipa' not in targets:
        raise ValueError('Missing launcher/native/original IPA')
    output.mkdir(parents=False,exist_ok=False)
    # Partial staging is retained on failure for inspection. It is not a
    # publishable package without the final manifest; never delete recursively.
    for target,(path,digest) in targets.items():
        dest=safe(output,target);dest.parent.mkdir(parents=True,exist_ok=True)
        with path.open('rb') as incoming,dest.open('xb') as outgoing:shutil.copyfileobj(incoming,outgoing,4*1024*1024)
        if sha(dest)!=digest or sha(path)!=digest:raise ValueError('Copy/source stability mismatch: '+target)
    manifest['files']={name:value[1] for name,value in sorted(targets.items())}
    manifest['distribution_layout']={'schema':2,'game':'Game','assets':'Assets','defaults':'Defaults','docs':'Docs',
        'diagnostics':'diagnostics/play-sessions','default_documents_leaf':'DeadSpaceLegacy',
        'staging_only':True,'runtime_validation':'not performed by relocation tool'}
    with (output/'Game/manifest.json').open('x',encoding='utf-8') as stream:json.dump(manifest,stream,ensure_ascii=False,indent=2)
    return manifest['distribution_layout']

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input',required=True,type=Path);parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args();print(json.dumps(stage(args.input,args.output)))

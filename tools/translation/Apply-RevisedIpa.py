#!/usr/bin/env python3
"""Create a derived IPA by replacing only the Chinese SBIN resource."""
from pathlib import Path
from zipfile import ZipFile
import argparse,hashlib,json,shutil,os
MEMBER='Payload/Dead Space.app/published/stringdata/CHS_CN/deadspace_iphone.bin'
ORIGINAL='47864a0c746cf439b9bab93e66aea621ae3896f2e517f710436b6302b0a3db77'
PREVIOUS_REVISIONS={
    '71f989b9cea4ae2dedb58b4e66279513ac06f077eb30e65ef7bfcdcfeada5bce',
    '4732db2574300a5407b783c72e0caa6002e961441379cbb86ef5780fa06d36f0',
    'e779145bb9c2415d23630bd844c141e7c208090e01f4a8e24193eecff6a2db9c',
}
def digest(data):return hashlib.sha256(data).hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--input',required=True,type=Path);p.add_argument('--output',type=Path);p.add_argument('--verify-only',action='store_true')
    a=p.parse_args();root=Path(__file__).resolve().parents[2];data=(root/'res/overlay/deadspace_iphone.bin').read_bytes()
    if not a.verify_only and not a.output:p.error('--output is required unless --verify-only is used')
    if a.output and (a.output.resolve()==a.input.resolve() or a.output.exists()):p.error('output must be a new file, separate from input')
    with ZipFile(a.input) as src:
        source_hash=digest(src.read(MEMBER));revised_hash=digest(data)
        if source_hash not in ({ORIGINAL,revised_hash}|PREVIOUS_REVISIONS):raise SystemExit('Unsupported Chinese resource hash; refusing to overwrite an unknown revision: '+source_hash)
        if a.verify_only:
            source_revision='revised' if source_hash==revised_hash else 'original' if source_hash==ORIGINAL else 'previous revision'
            print(json.dumps({'resource':MEMBER,'source_sha256':source_hash,'revision':source_revision,'replacement_sha256':revised_hash}));return
        temporary=a.output.with_suffix(a.output.suffix+'.tmp');a.output.parent.mkdir(parents=True,exist_ok=True)
        if temporary.exists():raise SystemExit('Temporary output already exists: '+str(temporary))
        try:
            with ZipFile(temporary,'w',allowZip64=True) as dst:
                for info in src.infolist():
                    if info.filename==MEMBER:dst.writestr(info,data)
                    else:
                        with src.open(info) as reader,dst.open(info,'w',force_zip64=info.file_size>=2**31) as writer:shutil.copyfileobj(reader,writer,1024*1024)
            with ZipFile(temporary) as check:
                if digest(check.read(MEMBER))!=revised_hash:raise RuntimeError('Round-trip verification failed')
            os.replace(temporary,a.output)
        finally:
            if temporary.exists():temporary.unlink()
    print(json.dumps({'output':str(a.output),'modified_entries':[MEMBER],'replacement_sha256':revised_hash}))
if __name__=='__main__':main()

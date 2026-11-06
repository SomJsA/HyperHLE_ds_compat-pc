"""Archive committed Android source and extract the PC visual repair history.

Does not checkout, merge, modify submodules, or change the running game.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    out = Path(args.output).resolve()
    out.mkdir(parents=True, exist_ok=False)

    def git(*argv, cwd=repo):
        return subprocess.check_output(['git', *argv], cwd=cwd)

    head = git('rev-parse', 'HEAD').decode().strip()
    baseline = git('rev-parse', '358c5731').decode().strip()
    git('archive', '--format=zip', '--prefix=source/', '-o', str(out/'source.zip'), head)
    git('bundle', 'create', str(out/'android-mainline.bundle'), 'codex/android-mainline-merge')
    modules = []
    for line in git('submodule', 'status').decode().splitlines():
        parts = line.strip().split()
        revision, path = parts[0].lstrip('+-'), parts[1]
        module = repo/path
        name = path.replace('/', '-')
        git('archive', '--format=zip', '-o', str(out/(name+'.zip')), revision, cwd=module)
        diff = git('diff', '--binary', 'HEAD', cwd=module)
        if diff:
            (out/(name+'-working.patch')).write_bytes(diff)
        modules.append({'path':path, 'revision':revision, 'working_patch':bool(diff)})

    visual = out/'visual-fixes'
    visual.mkdir()
    # Net renderer changes retain rollback decisions; applying every historical
    # experiment independently would reintroduce rejected settings.
    renderer = 'src/gles/gles1_on_gl2.rs'
    (visual/'renderer-net-from-358c5731.patch').write_bytes(
        git('diff', '--binary', baseline, head, '--', renderer))
    commits = ['5adc70fa','a518ca65','9b8d476a','261fe7a4','38ec3ce6',
               'b3fad9db','173574de','87a23def','bb420b34','d4c65100','75587095']
    history = visual/'history'
    history.mkdir()
    records = []
    for commit in commits:
        # Scope excludes keyboard dispatch and all Android FSR/present work.
        paths = [renderer]
        if commit in ['a518ca65','9b8d476a','38ec3ce6','b3fad9db','bb420b34']:
            paths.append('src/frameworks/opengles/gles_guest.rs')
        data = git('diff', '--binary', commit+'^', commit, '--', *paths)
        (history/(commit+'.patch')).write_bytes(data)
        records.append({'commit':git('rev-parse',commit).decode().strip(),
                        'subject':git('show','-s','--format=%s',commit).decode().strip(),
                        'paths':paths})
    (visual/'carrie-head.patch').write_bytes(git('diff','--binary','2ac477c4^','2ac477c4',
        '--','src/environment.rs','src/environment/ds_carrie_patch.rs'))
    for name in [renderer,'src/frameworks/opengles/gles_guest.rs','src/ds_texid.rs',
                 'src/environment.rs','src/environment/ds_carrie_patch.rs',
                 'tools/pc-runtime/Program.cs']:
        target = visual/'reference-source'/name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(git('show',head+':'+name))
    readme = repo/'dev-docs/session-2026-10-25/ANDROID-VISUAL-EXPORT.md'
    (out/'README.md').write_bytes(readme.read_bytes())
    (visual/'README.md').write_bytes(readme.read_bytes())
    # Validate ZIP CRCs and verify the renderer patch against its exact base in
    # an isolated temporary index (never touch the user's index/worktree).
    import os
    index = out/'verification.index'
    env = dict(os.environ, GIT_INDEX_FILE=str(index))
    subprocess.run(['git','read-tree',baseline],cwd=repo,env=env,check=True)
    subprocess.run(['git','apply','--cached','--check',str(visual/'renderer-net-from-358c5731.patch')],
                   cwd=repo,env=env,check=True)
    subprocess.run(['git','bundle','verify',str(out/'android-mainline.bundle')],cwd=repo,check=True,
                   stdout=subprocess.DEVNULL)
    for p in out.glob('*.zip'):
        with zipfile.ZipFile(p) as archive:
            assert archive.testzip() is None, p
    files = []
    for p in sorted(out.rglob('*')):
        if p.is_file():
            digest = hashlib.sha256()
            with p.open('rb') as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b''):
                    digest.update(block)
            files.append({'path':p.relative_to(out).as_posix(),'bytes':p.stat().st_size,
                          'sha256':digest.hexdigest()})
    manifest = {'source_commit':head,'renderer_baseline':baseline,
                'branch':'codex/android-mainline-merge','submodules':modules,
                'visual_history':records,'checks':['zip CRC','bundle verify','renderer patch check against baseline'],
                'files':files}
    (out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({'output':str(out),'source_commit':head,'files':len(files)}))


if __name__ == '__main__':
    main()

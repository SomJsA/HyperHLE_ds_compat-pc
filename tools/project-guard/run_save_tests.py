"""Run desktop-JDK save/IPA probes against copies in a new output directory."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys


def snapshot(root):
    result = {}
    for path in sorted(root.rglob('*')):
        if path.is_symlink():
            raise ValueError('Symlink in fixture: ' + str(path))
        if path.is_file():
            result[path.relative_to(root).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, required=True)
    parser.add_argument('--fixture-documents', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--javac', type=Path, default=Path('javac'))
    parser.add_argument('--java', type=Path, default=Path('java'))
    parser.add_argument('--pc-repo', type=Path, help='Optional PC implementation tree for C# ZIP regression')
    parser.add_argument('--csc', type=Path, help='C# compiler; requires --pc-repo')
    args = parser.parse_args()
    fixture, output, repo = args.fixture_documents.resolve(), args.output.resolve(), args.repo.resolve()
    if output.exists() or output.is_relative_to(fixture) or fixture.is_relative_to(output):
        parser.error('Use a new output directory separate from the fixture')
    for name in ['00_CheckpointSave.sb', '00_LevelSave.sb']:
        if not (fixture/'var'/name).is_file():
            parser.error('Missing slot0 fixture: ' + name)
    for slot in ['01', '02']:
        if (fixture/'var'/(slot+'_CheckpointSave.sb')).exists() or (fixture/'var'/(slot+'_LevelSave.sb')).exists():
            parser.error('Use a slot0-only test fixture; probes clone slots1/2 for transport coverage')
    before = snapshot(fixture)
    production = repo/'android/app/src/main/java/org/touchhle/android'
    if not (production/'BundledIpaStore.java').is_file():
        parser.error('--repo must point to the Android R15 implementation tree')
    if bool(args.csc) != bool(args.pc_repo):
        parser.error('Supply --csc and --pc-repo together')
    output.mkdir(parents=True, exist_ok=False)
    result = {'status': 'running', 'fixture': str(fixture), 'fixture_sha256': before,
              'device_operations': 0, 'native_gameplay_acceptance': False, 'steps': []}
    def run(name, command):
        process = subprocess.run(list(map(str, command)), capture_output=True, text=True,
                                 encoding='utf-8', errors='replace')
        (output/(name+'.log')).write_text(process.stdout+'\n'+process.stderr, encoding='utf-8')
        result['steps'].append({'name': name, 'exit_code': process.returncode})
        if process.returncode:
            raise RuntimeError(name+' failed; inspect output log')
    try:
        private = output/'fixture-Documents'
        shutil.copytree(fixture, private)
        classes = output/'classes'
        classes.mkdir()
        probes = repo/'tools/tests'
        run('javac', [args.javac, '-encoding', 'UTF-8', '-d', classes,
                      *[production/name for name in ['SaveArchiveCore.java', 'SaveScalarEditor.java', 'BundledIpaStore.java']],
                      probes/'SaveRoundtripTest.java', probes/'BundledIpaStoreTest.java'])
        run('save-roundtrip', [args.java, '-cp', classes, 'org.touchhle.android.SaveRoundtripTest', private, output/'profile'])
        run('ipa-protection', [args.java, '-cp', classes, 'org.touchhle.android.BundledIpaStoreTest', output/'ipa-protection'])
        if args.csc:
            pc = args.pc_repo.resolve()
            program = output/'SaveBundleTest.exe'
            run('csc', [args.csc, '/nologo', '/target:exe', '/out:'+str(program),
                        '/r:System.IO.Compression.dll', '/r:System.IO.Compression.FileSystem.dll',
                        pc/'tools/tests/SaveBundleTest.cs',
                        *[pc/'tools/pc-runtime'/name for name in ['SaveBundle.cs', 'SaveStorage.cs', 'SaveDocument.cs']]])
            all_slots = output/'profile/touchHLE_sandbox/com.ea.deadspace.bv/Documents'
            run('pc-zip-roundtrip', [program, all_slots, output/'pc-roundtrip'])
        else:
            result['skipped'] = ['C# PC ZIP probe: compiler/PC repo not supplied']
        result['status'] = 'passed'
    except Exception as error:
        result['status'] = 'failed'
        result['error'] = str(error)
    finally:
        result['original_fixture_unchanged'] = snapshot(fixture) == before
        if not result['original_fixture_unchanged']:
            result['status'] = 'failed'
        with (output/'result.json').open('x', encoding='utf-8') as stream:
            json.dump(result, stream, ensure_ascii=False, indent=2)
    print(json.dumps({key: result[key] for key in ['status', 'original_fixture_unchanged', 'device_operations']}, ensure_ascii=True))
    return 0 if result['status'] == 'passed' else 1


if __name__ == '__main__':
    sys.exit(main())

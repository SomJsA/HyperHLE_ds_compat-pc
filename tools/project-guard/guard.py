"""Read-only, offline R15 source/artifact guard. Never connects to a device."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path, PureWindowsPath
import re
import struct
import subprocess
import sys
from datetime import datetime, timezone
from zipfile import ZipFile


class GuardError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise GuardError(message)


def sha_stream(stream):
    digest = hashlib.sha256()
    for block in iter(lambda: stream.read(1024 * 1024), b''):
        digest.update(block)
    return digest.hexdigest()


def sha(path):
    with Path(path).open('rb') as stream:
        return sha_stream(stream)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'Duplicate JSON key: ' + key)
        result[key] = value
    return result


def read_json(path):
    return json.loads(Path(path).read_text(encoding='utf-8'), object_pairs_hook=unique_object)


def inside(root, name):
    name = str(name).replace('\\', '/')
    require(name and not name.startswith('/') and not PureWindowsPath(name).drive
            and '..' not in name.split('/'), 'Unsafe relative path: ' + name)
    root = Path(root).resolve()
    target = (root / name).resolve()
    require(target.is_relative_to(root), 'Path escapes root: ' + name)
    return target


def verify_hash(path, expected):
    actual = sha(path)
    require(actual == expected.lower(), 'SHA mismatch: ' + str(path))
    return actual


def verify_json_equivalent(first, second):
    require(read_json(first) == read_json(second), 'JSON content differs: ' + str(first))


def git(root, *args):
    result = subprocess.run(['git', '-C', str(root), *args], capture_output=True,
                            text=True, encoding='utf-8', errors='replace')
    require(result.returncode == 0, 'git ' + ' '.join(args) + ': ' + result.stderr[-500:])
    return result.stdout.strip()


def style_contract(style):
    layers = style['layers']
    require(style['reference'] == {'width': 480, 'height': 320, 'aspect': 1.5}, 'Reference viewport drift')
    require(layers['objective']['label'] == '目标：' and layers['objective']['labelFontSize'] == 18,
            'Objective LABEL must use large18; body remains independently fitted')
    for name in ['chapter', 'objective']:
        for key, value in {'width': 20.4, 'maxHeight': 17, 'x': 66.7, 'y': 49.1, 'fontSize': 7.2}.items():
            require(layers[name][key] == value, name + '.' + key + ' drift')
    require(layers['subtitle']['fontSize'] == 11.5 and layers['subtitle']['longTextMode'] == 'three',
            'Subtitle PC default drift')


def text_contract(rows, contract):
    values = {row['id']: row for row in rows}
    require(len(values) == len(rows) == contract['entries'], 'Missing/duplicate text IDs')
    require(values['CREDITS']['text'] == '信用点', 'Currency CREDITS confused with staff credits_list')
    titles = [values[f'CHAPTER_{n:02d}_GAME_TITLE'] for n in range(1, 13)]
    require(''.join(row['text'][0] for row in titles) == contract['chapter_acrostic'], 'Chinese chapter acrostic drift')
    require(''.join(row['english'][0].upper() for row in titles) == contract['english_acrostic'], 'English chapter acrostic drift')
    return {key: row['text'] for key, row in values.items()}


def native_strings(root):
    helper = inside(root, 'tools/translation/Update-Ledger.py')
    spec = importlib.util.spec_from_file_location('r15_guard_ledger', helper)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    _, sections = module.sections(inside(root, 'res/overlay/deadspace_iphone.bin').read_bytes())
    strings = module.unpack_strings(sections[b'STRS'])
    header = struct.unpack('<III', sections[b'OHDR'])
    offset = header[2] >> 3
    data = sections[b'DATA']
    kind, count = struct.unpack_from('<II', data, offset)
    require(kind == 16, 'Native string root is not an object')
    result = {}
    for index in range(count):
        key, value, _ = struct.unpack_from('<HHI', data, offset + 8 + 8 * index)
        key = strings[key - 1]
        require(key not in result, 'Duplicate native key: ' + key)
        result[key] = strings[value - 1]
    return result


def rust_code_without_comments(source):
    # Rust permits backslash/newline continuations in quoted strings. Dot must
    # include newline or a closing quote can consume subsequent real code.
    ignored = r'(?s:r(?P<hashes>#+)".*?"(?P=hashes)|r"[^"]*"|/\*.*?\*/)|//[^\n]*|(?s:"(?:\\.|[^"\\])*")'
    return re.sub(ignored, ' ', source)


def qte_memory_contract(source):
    # Ignore comments/quoted text before balancing this function's braces;
    # formatting, line numbers and a retained opt-in legacy helper elsewhere
    # are not the contract. This checks direct calls, not a full call graph.
    code = rust_code_without_comments(source)
    match = re.search(r'\bfn\s+ds_publish_qte_overlay\s*\(', code)
    require(match is not None, 'Missing PC QTE publisher; audit its replacement')
    start = code.find('{', match.end())
    require(start >= 0, 'PC QTE publisher has no body')
    depth = 1
    end = start + 1
    while end < len(code) and depth:
        depth += (code[end] == '{') - (code[end] == '}')
        end += 1
    require(depth == 0, 'PC QTE publisher body is incomplete')
    body = code[start:end]
    forbidden = r'\b(?:ds_find_interaction_ring|ReadPixels|ReadnPixels|CopyTexImage2D|CopyTexSubImage2D)\s*\('
    require(not re.search(forbidden, body), 'PC QTE publisher reintroduced framebuffer/pixel positioning')
    for kind in ['tap', 'swipe']:
        require(re.search(r'\bds_qte_geometry\s*::\s*' + kind + r'\s*\(', body),
                'PC QTE lost native geometry provider: ' + kind)
    return {'native_providers': ['tap', 'swipe'], 'scope': 'Direct publisher calls; actual native UI/performance still required'}


def eagl_transfer_cache_contract(source):
    # Structural guard for the reproduced legacy cache pattern. Hardware tests
    # still establish object ownership, pixels and current-context destruction.
    code = rust_code_without_comments(source)
    for match in re.finditer(r'\bthread_local\s*!\s*[({]', code):
        start = match.end() - 1
        opener = code[start]
        closer = '}' if opener == '{' else ')'
        depth, end = 1, start + 1
        while end < len(code) and depth:
            depth += (code[end] == opener) - (code[end] == closer)
            end += 1
        require(depth == 0, 'Incomplete EAGL thread_local block')
        body = code[start:end]
        forbidden = r'\b(?:GLuint|PresentProgram|PresentObjects|PRESENT_(?:PROGRAM|OBJECTS|SOURCE_RENDERBUFFER|TEXTURE_SIZE|ES1_TEXTURE|ES1_SOURCE_FBO))\b'
        require(not re.search(forbidden, body), 'EAGL transfer GL cache reintroduced in thread_local storage')
    require(re.search(r'\btransfer_state\s*:\s*Rc\s*<\s*RefCell\s*<\s*TransferState\s*>\s*>', code),
            'Missing EAGL-owned transfer cache; audit replacement ownership')
    return {'scope': 'Known GL cache types/names in thread_local macros plus owned-state anchor; not a full call graph or lifetime proof',
            'runtime_required': 'real shared/unshared GL contexts, pixels and current-context release'}


def implementation_snapshot(root, commit, allowed_untracked=None):
    require(re.fullmatch(r'[0-9a-f]{8,40}', commit or ''), 'Missing/invalid implementation commit')
    git(root, 'merge-base', '--is-ancestor', commit, 'HEAD')
    # Include uncommitted changes too. Documentation/test additions do not
    # pretend that the native binary was rebuilt from a later documentation HEAD.
    changed = git(root, 'diff', '--name-only', commit, '--').splitlines()
    allowed_untracked = allowed_untracked or {}
    for name in git(root, 'ls-files', '--others', '--exclude-standard').splitlines():
        expected = allowed_untracked.get(name)
        if expected and sha(root/name) == expected:
            continue
        changed.append(name)
    runtime = [name for name in changed if
               (name.startswith(('src/', 'android/app/src/', 'tools/pc-runtime/')) and not name.endswith('.md'))
               or name in ['Cargo.toml', 'Cargo.lock', 'build.rs', 'android/app/build.gradle']]
    require(not runtime, 'Artifact implementation is stale for runtime changes: ' + ', '.join(runtime[:10]))


class Report:
    def __init__(self, contract):
        self.data = {'schema': 1, 'time': datetime.now(timezone.utc).isoformat(),
                     'baseline': contract['baseline'], 'checks': [], 'notes': [],
                     'skipped': [], 'device_operations': 0, 'derived_ipa_generated': False,
                     'manual_gates_not_evaluated': contract['not_automatically_accepted']}

    def check(self, name, function):
        try:
            detail = function()
            self.data['checks'].append({'name': name, 'status': 'passed', 'detail': detail})
        except Exception as error:
            self.data['checks'].append({'name': name, 'status': 'failed',
                                        'detail': type(error).__name__ + ': ' + str(error)[:1200]})

    def finish(self):
        failed = [item['name'] for item in self.data['checks'] if item['status'] == 'failed']
        self.data['status'] = 'failed' if failed else 'scoped-checks-passed'
        self.data['failed_checks'] = failed
        return 1 if failed else 0


def expected_submodules(contract, platform):
    expected = dict(contract['submodules'])
    overrides = contract.get('platform_submodule_overrides', {}).get(platform, {})
    require(set(overrides).issubset(expected), 'Unknown platform submodule override')
    for path, commit in overrides.items():
        require(re.fullmatch(r'[0-9a-f]{40}', commit) is not None,
                'Invalid platform submodule commit: ' + path)
        expected[path] = commit
    return expected


def source_checks(report, pc, android, contract):
    for label, root in [('pc', pc), ('android', android)]:
        def health(root=root, label=label):
            status = git(root, 'status', '--porcelain=v1', '--untracked-files=normal').splitlines()
            report.data['notes'].append({'root': str(root), 'head': git(root, 'rev-parse', 'HEAD'),
                                         'branch': git(root, 'branch', '--show-current'),
                                         'uncommitted_count': len(status), 'uncommitted_sample': status[:15]})
            actual = {}
            for line in git(root, 'submodule', 'status').splitlines():
                require(line[0] not in '+-U', 'Uninitialized/changed submodule: ' + line)
                parts = line.split()
                actual[parts[1]] = parts[0]
            require(actual == expected_submodules(contract, label), 'Submodule baseline differs')
            return {'submodules': len(actual)}
        report.check(label + '.git-submodules', health)
        for relative, expected in contract['resource_sha256'].items():
            physical = contract.get('platform_resource_paths', {}).get(label, {}).get(relative, relative)
            report.check(label + '.resource.' + relative,
                         lambda root=root, physical=physical, expected=expected: verify_hash(inside(root, physical), expected))
        def texts(root=root):
            overlay = read_json(inside(root, 'src/ds_overlay_texts.json'))
            values = text_contract(overlay['entries'], contract)
            require(native_strings(root) == values, 'Native SBIN and overlay JSON differ')
            audit = read_json(inside(root, 'tools/translation/ledger-audit.json'))
            require(audit['native_bin_keys'] == len(values) and not audit['placeholder_differences'], 'Ledger audit failed')
            require(audit['source_sha256'].lower() == overlay['sourceSha256'].lower(), 'Ledger source differs')
            for fix in read_json(inside(root, 'tools/translation/text-corrections.json'))['corrections']:
                require(values[fix['id']] == fix['text'], 'Correction was lost: ' + fix['id'])
            return {'entries': len(values), 'native_matches_json': True}
        report.check(label + '.text-native-parity', texts)
        report.check(label + '.style-semantics', lambda root=root: style_contract(read_json(inside(root, 'src/ds_overlay_default.json'))))
    for relative, expected in contract.get('pc_font_resource_sha256', {}).items():
        report.check('pc.font-role.' + relative,
                     lambda relative=relative, expected=expected: verify_hash(inside(pc, relative), expected))
    report.check('pc.eagl-transfer-context-cache', lambda: eagl_transfer_cache_contract(
        (pc/'src/frameworks/opengles/eagl.rs').read_text(encoding='utf-8')))
    report.check('pc.qte-memory-publisher', lambda: qte_memory_contract(
        (pc/'src/frameworks/opengles/gles_guest.rs').read_text(encoding='utf-8')))
    report.check('shared.overlay-texts', lambda: require(
        (pc/'src/ds_overlay_texts.json').read_bytes() == (android/'src/ds_overlay_texts.json').read_bytes(), 'PC/Android overlay JSON differ'))
    for name in ['ChapterChinese.ttf', 'ChapterEnglish.ttf']:
        report.check('android.asset.' + name, lambda name=name: verify_hash(
            inside(android, 'android/app/src/main/assets/overlay/' + name), contract['resource_sha256']['res/overlay/' + name]))
    report.check('android.asset.style', lambda: verify_hash(
        inside(android, 'android/app/src/main/assets/overlay/default-style.json'), contract['resource_sha256']['src/ds_overlay_default.json']))
    report.check('android.ambient-platform-baseline', lambda: verify_hash(inside(android, 'src/gles/util.rs'), contract['android_ambient_sha256']))
    def api():
        gradle = (android/'android/app/build.gradle').read_text(encoding='utf-8')
        require(re.search(r'\bminSdkVersion\s+' + str(contract['min_sdk']) + r'\b', gradle), 'minSdk changed')
        folder = android/'android/app/src/main/java/org/touchhle/android'
        files = [*folder.glob('Save*.java'), folder/'BundledIpaStore.java']
        forbidden = r'java\.nio\.file|\.stream\s*\(|Integer\.toUnsignedLong|Long\.toUnsignedString'
        for file in files:
            require(not re.search(forbidden, file.read_text(encoding='utf-8')), 'Known API24/26 storage regression: ' + file.name)
        return {'files': len(files), 'scope': 'known storage API regression patterns only; not full lint/API21 device proof'}
    report.check('android.storage-api21-patterns', api)


def pc_layout(folder):
    if not (folder/'Game').exists():
        return {'game': folder, 'defaults': folder, 'schema': 1}
    require((folder/'Game/manifest.json').is_file(), 'New PC layout has no Game manifest')
    root_files = {path.name for path in folder.iterdir() if not path.is_dir()}
    require(root_files == {'DeadSpace Launcher.exe'}, 'Launcher root contains unrelated files: ' + ','.join(sorted(root_files)))
    for path in folder.rglob('*'):
        require(not path.is_symlink() and not (getattr(path.lstat(), 'st_file_attributes', 0) & 0x400), 'Link in PC distribution')
        if path.is_dir():
            require(re.fullmatch(r'[A-Za-z0-9_.-]+', path.name) is not None, 'Non-English distribution directory: ' + path.name)
    return {'game': folder/'Game', 'defaults': folder/'Defaults', 'schema': 2}


def pc_package_checks(report, folder, pc, contract, allowed_untracked=None):
    def check():
        layout = pc_layout(folder)
        game = layout['game']
        manifest = read_json(game/'manifest.json')
        implementation_snapshot(
            pc, manifest['implementation_commit'], allowed_untracked)
        for name, expected in manifest['source_sha256'].items():
            verify_hash(inside(pc, name), expected)
        for name, expected in manifest['files'].items():
            verify_hash(inside(folder, name), expected)
        # The artifact's exact bytes are verified by its manifest above. JSON
        # indentation/newlines need not equal the source resource's formatting.
        verify_json_equivalent(layout['defaults']/'overlay-style.json', pc/'src/ds_overlay_default.json')
        if manifest.get('marker_required') is True:
            require(not (game/'DeadSpace.ipa').exists(), 'Marker package must not embed the game IPA')
            require(not (folder/'Assets/texture-packs').exists(), 'Marker package must not embed texture packs')
            require(re.fullmatch(r'[0-9a-f]{64}', str(manifest.get('marker_id', ''))) is not None, 'Marker package marker_id invalid')
            require(re.fullmatch(r'[0-9a-f]{64}', str(manifest.get('marker_sha256', ''))) is not None, 'Marker package marker_sha256 invalid')
            marker_root = manifest.get('marker_install_root')
            allowed_roots = {
                'Documents/DeadSpaceLegacy/marker-content/<marker_id>',
                '%LOCALAPPDATA%\\DeadSpaceLegacy\\GameData\\marker-content\\<marker_id>',
            }
            require(marker_root in allowed_roots, 'Marker install root changed')
        else:
            require((game/'DeadSpace.ipa').is_file(), 'PC package has no game IPA')
            require(sha(game/'DeadSpace.ipa') in contract['accepted_ipa_sha256'], 'Unknown/derived PC IPA')
        bank = (pc/'res/overlay/deadspace_iphone.bin').read_bytes()
        executable = (game/'touchHLE.exe').read_bytes()
        require(bank in executable, 'PC executable does not embed current native text bank')
        for relative, expected in contract.get('pc_font_resource_sha256', {}).items():
            font_path = inside(pc, relative)
            verify_hash(font_path, expected)
            require(font_path.read_bytes() in executable, 'PC executable lacks current role font: ' + relative)
        require(manifest['native_bank_sha256'] == hashlib.sha256(bank).hexdigest(), 'PC bank manifest drift')
        return {'files': len(manifest['files']), 'declared_source_files': len(manifest['source_sha256']), 'embedded_native_bank': True,
                'marker_required': bool(manifest.get('marker_required')),
                'exact_untracked_exclusions': len(allowed_untracked or {})}
    report.check('pc.package', check)


def android_package_checks(report, folder, android, contract, apksigner):
    apk = folder/'DeadSpace-R15-arm64-release.apk'
    def check():
        manifest = read_json(folder/'manifest.json')
        implementation_snapshot(android, manifest['implementation_commit'])
        verify_hash(apk, manifest['apk_sha256'])
        bank = (android/'res/overlay/deadspace_iphone.bin').read_bytes()
        with ZipFile(apk) as archive:
            require(archive.testzip() is None, 'APK CRC failed')
            require(len(archive.namelist()) == len(set(archive.namelist())), 'Duplicate APK entries')
            for name, expected in manifest['native_sha256'].items():
                require(hashlib.sha256(archive.read(name)).hexdigest() == expected, 'Native library differs: ' + name)
            require(bank in archive.read('lib/arm64-v8a/libtouchHLE.so'), 'Android native library embeds stale text bank')
            for name in ['ChapterChinese.ttf', 'ChapterEnglish.ttf', 'default-style.json']:
                source = 'src/ds_overlay_default.json' if name.endswith('.json') else 'res/overlay/' + name
                require(hashlib.sha256(archive.read('assets/overlay/' + name)).hexdigest() == contract['resource_sha256'][source], 'APK asset drift: ' + name)
            with archive.open('assets/touchHLE_bundled/DeadSpace.ipa') as stream:
                ipa = sha_stream(stream)
            require(ipa in contract['accepted_ipa_sha256'], 'Unknown/derived Android bundled IPA')
            require(archive.read('assets/touchHLE_bundled/bundle.id').decode('ascii').strip() == ipa, 'Bundled IPA marker differs')
        return {'crc': True, 'embedded_native_bank': True, 'native_libraries': len(manifest['native_sha256']), 'ipa_sha256': ipa}
    report.check('android.package', check)
    if apksigner:
        def signature():
            result = subprocess.run([str(apksigner), 'verify', '--print-certs', str(apk)], capture_output=True, text=True)
            require(result.returncode == 0, 'APK signature verification failed: ' + result.stderr[-500:])
            expected = read_json(folder/'manifest.json')['certificate_sha256']
            require(expected in result.stdout, 'APK signing certificate differs')
            return expected
        report.check('android.signature', signature)
    else:
        report.data['skipped'].append('APK signature tool verification: --apksigner not supplied')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pc-root', type=Path, required=True)
    parser.add_argument('--android-root', type=Path, required=True)
    parser.add_argument('--pc-package', type=Path)
    parser.add_argument('--android-package', type=Path)
    parser.add_argument('--apksigner', type=Path)
    parser.add_argument('--protected-manifest', type=Path)
    parser.add_argument('--pc-artifact-exclude', type=Path)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args(argv)
    if args.report and args.report.exists():
        parser.error('Report already exists; use a new path, no overwrite')
    contract = read_json(Path(__file__).with_name('contract.json'))
    report = Report(contract)
    source_checks(report, args.pc_root.resolve(), args.android_root.resolve(), contract)
    allowed_untracked = {}
    if args.pc_artifact_exclude:
        allowed_untracked = read_json(args.pc_artifact_exclude)
        require(isinstance(allowed_untracked, dict), 'PC artifact exclude must be a JSON object')
        for name, expected in allowed_untracked.items():
            require(isinstance(name, str) and name and not PureWindowsPath(name).is_absolute() and '..' not in Path(name).parts,
                    'Unsafe PC artifact exclusion path: ' + str(name))
            require(isinstance(expected, str) and re.fullmatch(r'[0-9a-f]{64}', expected),
                    'PC artifact exclusion hash invalid: ' + name)
    if args.pc_package:
        pc_package_checks(
            report, args.pc_package.resolve(), args.pc_root.resolve(),
            contract, allowed_untracked)
    else:
        report.data['skipped'].append('PC release artifacts not supplied')
    if args.android_package:
        android_package_checks(report, args.android_package.resolve(), args.android_root.resolve(), contract, args.apksigner)
    else:
        report.data['skipped'].append('Android release artifacts/signature not supplied')
    if args.protected_manifest:
        def protected():
            values = read_json(args.protected_manifest)
            require(bool(values), 'Empty protection manifest')
            for name, expected in values.items():
                path = Path(name)
                require(path.is_absolute(), 'Protected path must be absolute: ' + name)
                verify_hash(path, expected)
            return {'files': len(values)}
        report.check('protected-files', protected)
    else:
        report.data['skipped'].append('Protected-file manifest not supplied')
    code = report.finish()
    if args.report:
        with args.report.open('x', encoding='utf-8') as stream:
            json.dump(report.data, stream, ensure_ascii=False, indent=2)
            stream.write('\n')
    print(json.dumps({'status': report.data['status'], 'checks': len(report.data['checks']),
                      'failed': report.data['failed_checks'], 'skipped': report.data['skipped'],
                      'device_operations': 0, 'report': str(args.report) if args.report else None}, ensure_ascii=True))
    return code


if __name__ == '__main__':
    sys.exit(main())

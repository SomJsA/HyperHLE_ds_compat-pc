import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('guard', Path(__file__).with_name('guard.py'))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
CONTRACT = guard.read_json(Path(__file__).with_name('contract.json'))


class ContractTests(unittest.TestCase):
    def test_platform_submodule_pin_is_scoped_and_unknown_overrides_rejected(self):
        contract = copy.deepcopy(CONTRACT)
        contract['platform_submodule_overrides'] = {'pc': {'vendor/dynarmic': '1' * 40}}
        base = copy.deepcopy(contract['submodules'])
        self.assertEqual(guard.expected_submodules(contract, 'android'), base)
        expected = dict(base, **{'vendor/dynarmic': '1' * 40})
        self.assertEqual(guard.expected_submodules(contract, 'pc'), expected)
        self.assertEqual(contract['submodules'], base)
        contract['platform_submodule_overrides']['pc']['vendor/unknown'] = '2' * 40
        with self.assertRaises(guard.GuardError):
            guard.expected_submodules(contract, 'pc')
        del contract['platform_submodule_overrides']['pc']['vendor/unknown']
        contract['platform_submodule_overrides']['pc']['vendor/dynarmic'] = 'not-a-commit'
        with self.assertRaises(guard.GuardError):
            guard.expected_submodules(contract, 'pc')


    def test_eagl_gl_caches_cannot_return_to_thread_local_storage(self):
        owned = 'struct EAGLContextHostObject { transfer_state: Rc<RefCell<TransferState>> }'
        guard.eagl_transfer_cache_contract(owned + 'thread_local! { static SCRATCH: RefCell<RgbaBuffer> = make_buffer(); }')
        for cache in ['static C: Cell<Option<PresentProgram>> = empty();',
                      'static C: Cell<Option<PresentObjects>> = empty();',
                      'static C: Cell<GLuint> = Cell::new(0);',
                      'static PRESENT_ES1_TEXTURE: Cell<Option<(u32,i32,i32)>> = empty();']:
            for block in ['thread_local! { '+cache+' }', 'thread_local ! ( '+cache+' );']:
                with self.assertRaises(guard.GuardError):
                    guard.eagl_transfer_cache_contract(owned + block)

    def test_eagl_cache_guard_ignores_quoted_history_and_requires_owner(self):
        owned = 'struct EAGLContextHostObject { transfer_state: Rc < RefCell < TransferState > > }'
        history = '// thread_local! { static C: GLuint = zero(); }\n'
        history += 'const NOTE: &str = "thread_local! { static PRESENT_ES1_TEXTURE: u32 = 0; }";'
        guard.eagl_transfer_cache_contract(owned + history)
        guard.eagl_transfer_cache_contract(owned + 'const NOTE: &str = r#"thread_local! { static C: GLuint = 0; }"#;')
        with self.assertRaises(guard.GuardError):
            guard.eagl_transfer_cache_contract('thread_local! { static BUFFER: RgbaBuffer = make(); }')

    def test_rust_continuation_cannot_hide_context_owner_or_pixel_calls(self):
        prefix = 'fn note() { echo!("first' + chr(92) + chr(10) + 'second"); }' + chr(10)
        owned = 'struct EAGLContextHostObject { transfer_state: Rc<RefCell<TransferState>> }'
        guard.eagl_transfer_cache_contract(prefix + owned)
        with self.assertRaises(guard.GuardError):
            guard.eagl_transfer_cache_contract(prefix + owned + 'thread_local! { static GL: Option<PresentProgram> = None; }')
        qte = 'fn ds_publish_qte_overlay() { ds_qte_geometry::tap(env,hud); ds_qte_geometry::swipe(env,hud); }'
        guard.qte_memory_contract(prefix + qte)
        with self.assertRaises(guard.GuardError):
            guard.qte_memory_contract(prefix + qte.replace('ds_qte_geometry::tap', 'gles.ReadPixels(x,y,w,h); ds_qte_geometry::tap'))

    def test_qte_pixel_fallback_is_rejected_even_behind_an_optional_branch(self):
        source = '''fn ds_publish_qte_overlay(env: &mut Environment) {
            let p = ds_qte_geometry :: tap (env, hud);
            let s = ds_qte_geometry::swipe(env, hud);
        }'''
        guard.qte_memory_contract(source)
        for call in ['ds_find_interaction_ring(gles, w, None)', 'gles.ReadPixels(x,y,w,h)']:
            broken = source.replace('let p =', 'if enabled { ' + call + '; } let p =')
            with self.assertRaises(guard.GuardError):
                guard.qte_memory_contract(broken)
        with self.assertRaises(guard.GuardError):
            guard.qte_memory_contract(source.replace('ds_qte_geometry::swipe', 'unknown_geometry'))

    def test_qte_guard_ignores_comments_strings_and_legacy_helper_outside_publisher(self):
        source = '''fn ds_publish_qte_overlay(env: &mut Environment) {
            // } ds_find_interaction_ring(gles);
            let label = "ReadPixels() }";
            /* { ReadPixels(1); */
            let p = ds_qte_geometry::tap(env, hud);
            let s = ds_qte_geometry::swipe(env, hud);
        }
        fn opt_in_legacy() { ds_find_interaction_ring(gles); }
        '''
        guard.qte_memory_contract(source)

    def test_objective_label_is_independent_of_body(self):
        root = Path(__file__).resolve().parents[2]
        style = guard.read_json(root/'src/ds_overlay_default.json')
        guard.style_contract(style)
        broken = copy.deepcopy(style)
        broken['layers']['objective']['labelFontSize'] = 7.2
        with self.assertRaises(guard.GuardError):
            guard.style_contract(broken)
        broken = copy.deepcopy(style)
        broken['layers']['subtitle']['longTextMode'] = 'paged'
        with self.assertRaises(guard.GuardError):
            guard.style_contract(broken)

    def test_currency_and_duplicate_ids_are_not_just_entry_count(self):
        root = Path(__file__).resolve().parents[2]
        rows = guard.read_json(root/'src/ds_overlay_texts.json')['entries']
        guard.text_contract(rows, CONTRACT)
        broken = copy.deepcopy(rows)
        next(row for row in broken if row['id'] == 'CREDITS')['text'] = '制作团队'
        with self.assertRaises(guard.GuardError):
            guard.text_contract(broken, CONTRACT)
        broken = copy.deepcopy(rows)
        broken[-1] = broken[0]
        with self.assertRaises(guard.GuardError):
            guard.text_contract(broken, CONTRACT)

    def test_native_menu_mapping_covers_interleaved_entries(self):
        chapters = CONTRACT['native_chapter_menu_zero_based']
        trains = CONTRACT['native_train_menu_zero_based']
        self.assertEqual(len(chapters), 12)
        self.assertEqual(len(trains), 5)
        self.assertEqual(sorted(chapters + trains), list(range(17)))
        self.assertEqual(chapters[3], 4)  # Index3 is a train, not chapter4.

    def test_unsafe_manifest_paths_refused(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)/'root'
            root.mkdir()
            for path in ['../bad', '/absolute', 'C:/outside', r'..\bad', r'\\server\share\file']:
                with self.assertRaises(guard.GuardError):
                    guard.inside(root, path)
            self.assertEqual(guard.inside(root, 'res/text.bin'), root/'res/text.bin')

    def test_duplicate_json_and_changed_protected_file_refused(self):
        with tempfile.TemporaryDirectory() as name:
            path = Path(name)/'manifest.json'
            path.write_text('{"sha":"old","sha":"new"}', encoding='utf-8')
            with self.assertRaises(guard.GuardError):
                guard.read_json(path)
            original = guard.sha(path)
            path.write_text('{}', encoding='utf-8')
            with self.assertRaises(guard.GuardError):
                guard.verify_hash(path, original)

    def test_json_formatting_does_not_hide_or_fake_style_changes(self):
        with tempfile.TemporaryDirectory() as name:
            first, second = Path(name)/'source.json', Path(name)/'package.json'
            first.write_text('{"fontSize":18,"mode":"three"}', encoding='utf-8')
            second.write_text('{\n  "mode": "three",\n  "fontSize": 18\n}\n', encoding='utf-8')
            guard.verify_json_equivalent(first, second)
            second.write_text('{"fontSize":7.2,"mode":"three"}', encoding='utf-8')
            with self.assertRaises(guard.GuardError):
                guard.verify_json_equivalent(first, second)

    def test_failure_does_not_become_pass_and_manual_gates_remain_open(self):
        report = guard.Report(CONTRACT)
        report.check('bad', lambda: guard.require(False, 'failure'))
        report.check('good', lambda: True)
        self.assertEqual(report.finish(), 1)
        self.assertEqual(report.data['failed_checks'], ['bad'])
        self.assertTrue(report.data['manual_gates_not_evaluated'])
        self.assertEqual(report.data['device_operations'], 0)


class ProvenanceTests(unittest.TestCase):
    def test_documentation_head_allowed_but_changed_runtime_rejected(self):
        with tempfile.TemporaryDirectory(prefix='r15-guard-') as name:
            root = Path(name)
            def git(*args):
                return subprocess.check_output(['git', '-C', str(root), *args], stderr=subprocess.DEVNULL, text=True).strip()
            def commit():
                git('add', '--', '.')  # Only a new, isolated unit-test repository.
                git('-c', 'user.name=GuardTest', '-c', 'user.email=guard@example.invalid', 'commit', '-qm', 'fixture')
            git('init', '-q')
            (root/'src').mkdir()
            (root/'src/runtime.rs').write_text('version1', encoding='utf-8')
            commit()
            implementation = git('rev-parse', 'HEAD')
            (root/'README.md').write_text('documentation', encoding='utf-8')
            commit()
            guard.implementation_snapshot(root, implementation)
            (root/'src/untracked.rs').write_text('new runtime', encoding='utf-8')
            with self.assertRaises(guard.GuardError):
                guard.implementation_snapshot(root, implementation)
            (root/'src/untracked.rs').unlink()
            (root/'src/runtime.rs').write_text('version2', encoding='utf-8')
            with self.assertRaises(guard.GuardError):
                guard.implementation_snapshot(root, implementation)
            commit()
            with self.assertRaises(guard.GuardError):
                guard.implementation_snapshot(root, implementation)


if __name__ == '__main__':
    unittest.main()

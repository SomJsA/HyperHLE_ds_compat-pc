import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

def module(name,path):
    spec=importlib.util.spec_from_file_location(name,path);result=importlib.util.module_from_spec(spec);spec.loader.exec_module(result);return result
HERE=Path(__file__).parent
stage=module('stage_distribution',HERE.parent/'pc-runtime/Stage-Distribution.py')
guard=module('layout_guard',HERE/'guard.py')

class LayoutTests(unittest.TestCase):
    def fixture(self,root):
        root.mkdir();files={}
        for name in ['DeadSpace Launcher.exe','touchHLE.exe','DeadSpace.ipa','overlay-style.json','Defaults/overlay-style.json','DeadSpace-Keys.ini','texture-packs/4/PACK.json','res/overlay/text.bin','touchHLE_fonts/font.ttf','build-info.json','README.txt']:
            path=root/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(b'fixture bytes');files[name]=stage.sha(path)
        (root/'manifest.json').write_text(json.dumps({'files':files,'implementation_commit':'fixture'}),encoding='utf-8')
        return files
    def test_byte_preserving_layout_and_guard(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);source=root/'input';files=self.fixture(source);output=root/'output'
            stage.stage(source,output);layout=guard.pc_layout(output)
            self.assertEqual(layout['schema'],2)
            self.assertEqual({p.name for p in output.iterdir() if p.is_file()},{'DeadSpace Launcher.exe'})
            self.assertEqual((output/'Game/DeadSpace.ipa').read_bytes(),b'fixture bytes')
            self.assertTrue((output/'Defaults/DeadSpace-Keys.ini').is_file())
            self.assertTrue((output/'Assets/texture-packs/4/PACK.json').is_file())
            for name,digest in files.items():self.assertEqual(stage.sha(source/name),digest)
            (output/'touchHLE.exe').write_bytes(b'bad placement')
            with self.assertRaises(guard.GuardError):guard.pc_layout(output)
    def test_tamper_and_existing_output_refused_before_write(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);source=root/'input';self.fixture(source);output=root/'output'
            (source/'DeadSpace.ipa').write_bytes(b'tampered')
            with self.assertRaises(ValueError):stage.stage(source,output)
            self.assertFalse(output.exists())
            output.mkdir();(output/'retain').write_bytes(b'original')
            with self.assertRaises(ValueError):stage.stage(source,output)
            self.assertEqual((output/'retain').read_bytes(),b'original')

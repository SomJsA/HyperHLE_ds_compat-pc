"""Boundary checks for the optional Win64 emitted-code journal + PSS sampler.

Set DS_JIT_PROFILE_TOOLS to the standalone tools directory before running.
These child processes never read an IPA, save, or guest memory.
"""
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

TOOLS = Path(os.environ['DS_JIT_PROFILE_TOOLS'])
MAP = TOOLS / 'TestPcJitMap.exe'
SAMPLER = TOOLS / 'PcJitSampler.exe'

def run_map(path, mode='normal'):
    env = {key:value for key,value in os.environ.items() if key.upper()!='TOUCHHLE_DS_JIT_MAP'}
    if path is not None:
        env['TOUCHHLE_DS_JIT_MAP']=str(path)
    return subprocess.run([str(MAP),mode],env=env,capture_output=True,check=True,timeout=30)

class MapBoundaryTests(unittest.TestCase):
    def test_disabled_and_existing_file_are_not_written(self):
        result=run_map(None)
        self.assertEqual(result.stderr,b'')
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'existing.tsv';path.write_bytes(b'keep original bytes\x00\xff')
            result=run_map(path)
            self.assertEqual(path.read_bytes(),b'keep original bytes\x00\xff')
            self.assertIn(b'reason=create',result.stderr)

    def test_epoch_and_sanitized_labels_preserve_address_reuse(self):
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'map.tsv';run_map(path)
            rows=[line.split('\t') for line in path.read_text().splitlines()]
            self.assertEqual([row[0] for row in rows],['H','M','C','M'])
            self.assertEqual(rows[1][2:5],['0','1000','100'])
            self.assertEqual(rows[1][5],'old label ')
            self.assertEqual(rows[2][2],'1')
            self.assertEqual(rows[3][2:5],['1','1000','100'])
            self.assertGreaterEqual(int(rows[3][1]),int(rows[2][1]))

    def test_concurrent_complete_records_and_size_limit(self):
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'concurrent.tsv';run_map(path,'stress')
            rows=path.read_text().splitlines()
            self.assertEqual(len(rows),4001)
            self.assertTrue(all(len(row.split('\t'))==6 for row in rows[1:]))
            self.assertEqual(len({row.split('\t')[3] for row in rows[1:]}),4000)
            path=Path(folder)/'bounded.tsv';result=run_map(path,'limit')
            self.assertLessEqual(path.stat().st_size,16*1024*1024)
            with path.open('rb') as stream:
                stream.seek(-256,2);tail=stream.read()
            self.assertIn(b'\nL\t',tail)
            self.assertEqual(result.stderr.count(b'reason=limit'),1)

    def test_bad_parent_directory_disables_without_creating_it(self):
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'missing'/'map.tsv';result=run_map(path)
            self.assertFalse(path.parent.exists())
            self.assertIn(b'reason=create',result.stderr)

class SnapshotBoundaryTests(unittest.TestCase):
    def test_owned_child_contexts_and_creation_identity(self):
        kernel=ctypes.WinDLL('kernel32',use_last_error=True)
        kernel.GetProcessTimes.argtypes=[wintypes.HANDLE]+[ctypes.POINTER(wintypes.FILETIME)]*4
        child=subprocess.Popen([str(SAMPLER),'--burn'],creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            values=[wintypes.FILETIME() for _ in range(4)]
            self.assertTrue(kernel.GetProcessTimes(int(child._handle),*(ctypes.byref(value) for value in values)))
            created=values[0].dwLowDateTime+(values[0].dwHighDateTime<<32)
            with tempfile.TemporaryDirectory() as folder:
                path=Path(folder)/'samples.tsv'
                command=[str(SAMPLER),str(child.pid),str(created),'2','20',str(path),str(SAMPLER.resolve())]
                completed=subprocess.run(command,capture_output=True,timeout=15)
                self.assertEqual(completed.returncode,0,completed.stderr.decode(errors='replace'))
                rows=[line.split('\t') for line in path.read_text().splitlines()]
                samples=[row for row in rows if row[0]=='S']
                self.assertGreater(len(samples),30)
                self.assertTrue(all(int(row[4],16)>0 and int(row[1])<=int(row[2])<=int(row[3]) for row in samples))
                self.assertEqual(rows[-1][0],'F')
                self.assertEqual(rows[-1][-1],'0','PSS resources must free successfully')
                self.assertGreater(int(samples[-1][6]),int(samples[0][6]),'Target CPU must advance through snapshots')
                self.assertIsNone(child.poll(),'Target must still be alive after sampling')
                command[2]=str(created+1);command[5]=str(Path(folder)/'rejected.tsv')
                rejected=subprocess.run(command,capture_output=True,timeout=5)
                self.assertEqual(rejected.returncode,4)
                self.assertFalse(Path(command[5]).exists())
        finally:
            if child.poll() is None:
                child.terminate()
            child.wait(timeout=5)

if __name__=='__main__':
    unittest.main()

from pathlib import Path
import tempfile
import unittest
from analyze_jit_samples import join

class JoinBoundaryTests(unittest.TestCase):
    def run_join(self,maps,samples,creation=999):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            (root/'map').write_text(f'H\t2\t123\t1000\t{creation}\n'+maps,encoding='utf-8')
            count=sum(line.startswith('S\t') for line in samples.splitlines())
            last=max([int(line.split('\t')[3]) for line in samples.splitlines() if line.startswith('S\t')]+[99])
            (root/'samples').write_text('H\t1\t123\t321\t1000\t999\n'+samples+f'F\t{last+1}\t{count}\t0\t0\n',encoding='utf-8')
            return join(root/'map',root/'samples')

    def test_clear_drops_stale_labels_and_future_mapping_is_not_used(self):
        result=self.run_join('M\t10\t0\t1000\t100\told\nC\t30\t1\nM\t40\t1\t1000\t100\tnew\n',
            'S\t20\t21\t22\t1080\t0\t1\t0\nS\t35\t36\t37\t1080\t0\t2\t0\nS\t45\t46\t47\t1080\t0\t3\t0\n')
        self.assertEqual(result['categories'],{'jit':2,'unknown_native_or_jit':1})
        self.assertEqual({row['name']:row['samples'] for row in result['top_jit_labels']},{'old':1,'new':1})

    def test_partial_overlap_and_capture_transition_are_conservative(self):
        result=self.run_join('M\t1\t0\t1000\t200\told\nM\t5\t0\t1080\t80\tnew\n',
            'S\t4\t5\t6\t1081\t0\t1\t0\nS\t10\t11\t12\t1001\t0\t2\t0\nS\t20\t21\t22\t1081\t0\t3\t0\nS\t30\t31\t32\t1181\t0\t4\t0\n')
        self.assertEqual(result['categories'],{'ambiguous_mapping_transition':1,'jit':3})
        self.assertEqual({row['name']:row['samples'] for row in result['top_jit_labels']},{'old':2,'new':1})

    def test_limit_ends_mapping_coverage(self):
        result=self.run_join('M\t1\t0\t1000\t100\told\nL\t25\t0\t100\n',
            'S\t20\t21\t22\t1080\t0\t1\t0\nS\t30\t31\t32\t1080\t0\t2\t0\n')
        self.assertEqual(result['categories'],{'jit':1,'map_limit_reached':1})

    def test_inconsistent_epoch_and_unknown_sampler_records_reject(self):
        with self.assertRaises(ValueError):
            self.run_join('C\t1\t3\n','S\t2\t3\t4\t1080\t0\t1\t0\n')
        with self.assertRaises(ValueError):
            self.run_join('','E\t1\tcapture\t5\n')

    def test_reused_pid_with_other_creation_time_is_rejected(self):
        with self.assertRaisesRegex(ValueError,'creation time'):
            self.run_join('M\t1\t0\t1000\t100\told\n','S\t2\t3\t4\t1080\t0\t1\t0\n',creation=998)

    def test_complete_histogram_keeps_blocks_below_summary_limit(self):
        maps=''.join(f'M\t1\t0\t{0x1000+i*256:x}\t100\ta32_a{i*4:08X}_le_fpcr00000000\n' for i in range(45))
        samples=''.join(f'S\t{10+i*3}\t{11+i*3}\t{12+i*3}\t{0x1000+i*256:x}\t0\t{i}\t0\n' for i in range(45))
        result=self.run_join(maps,samples)
        self.assertEqual(len(result['top_arm_block_addresses']),30)
        self.assertEqual(len(result['arm_block_histogram']),45)
        self.assertEqual(sum(result['arm_block_histogram'].values()),45)

if __name__=='__main__':unittest.main()

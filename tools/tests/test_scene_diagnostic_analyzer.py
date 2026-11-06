"""Regression with real event names and a long live frame amid detail gaps."""
import csv,importlib.util,json,tempfile,unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('scene_analyzer',Path(__file__).resolve().parents[1]/'pc-runtime/Analyze-SceneDiagnostics.py')
analyzer=importlib.util.module_from_spec(spec);spec.loader.exec_module(analyzer)
class SceneAnalyzerRegression(unittest.TestCase):
    def test_actual_self_protection_and_hd_events_survive_summary_with_valid_long_frames(self):
        with tempfile.TemporaryDirectory(prefix='ds-scene-test-') as folder:
            root=Path(folder)/'session-test';root.mkdir()
            (root/'session-start.json').write_text(json.dumps({'scene_diagnostics':{'kinds':['test_resource']}}),encoding='utf-8')
            with (root/'frames-000000.csv').open('w',newline='',encoding='utf-8') as f:
                writer=csv.writer(f);writer.writerow(['mono_us','native_present','interval_us','phase','native_state','native_hud']);writer.writerows([[17000,1,16000,4,0,0],[35000,2,18000,4,0,0],[300000,4,250000,4,9,0],[2300000,5,2000000,2,0,0]])
            events=[{'type':'self_protection','mono_us':20000,'level':1,'old_level':0,'reason':'slow_writer','skipped_total':1},
                {'type':'native_event','mono_us':25000,'code':'hd_replacement_submission','values':[123,8192,8192,0],'origin_thread_tag':1},
                {'type':'one_second_essentials','mono_us':1000000,'dropped_total':0,'detail_skipped_total':1,'collector_ns':1000}]
            (root/'timeline-000000.jsonl').write_text('\n'.join(json.dumps(e) for e in events)+'\n',encoding='utf-8')
            result=analyzer.summarize(root)
            self.assertEqual(result['recorded_live_frames'],3)
            self.assertEqual(result['max_ms'],250)
            self.assertEqual(result['over50ms'],1)
            self.assertEqual(result['missing_frame_numbers'],1)
            self.assertEqual(result['skipped_frames_max'],1)
            self.assertEqual(result['detail_changes_first256'][0]['type'],'self_protection')
            self.assertTrue(any(e['code']=='hd_replacement_submission' for w in result['worst32_recorded_live_frames'] for e in w['nearby_events']))
if __name__=='__main__':unittest.main()

import importlib.util
from pathlib import Path
from types import SimpleNamespace
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('device_job', Path(__file__).with_name('device_job.py'))
job = importlib.util.module_from_spec(spec)
spec.loader.exec_module(job)


class DeviceJobTests(unittest.TestCase):
    def exercise(self, first, sleep=0):
        calls = []
        def runner(command, **kwargs):
            calls.append(command)
            if len(calls) == 1:
                if isinstance(first, BaseException):
                    raise first
                return SimpleNamespace(returncode=first)
            return SimpleNamespace(returncode=sleep)
        with tempfile.TemporaryDirectory() as output:
            result = job.run_job('adb-mock', 'explicit-serial', ['finite-job'], Path(output), 30, runner)
            self.assertEqual(calls[-1], ['adb-mock', '-s', 'explicit-serial', 'shell', 'input', 'keyevent', 'KEYCODE_SLEEP'])
            self.assertEqual(len(calls), 2)
            self.assertNotIn('KEYCODE_WAKEUP', str(calls))
            self.assertEqual(result['device_queries'], 0)
            return result

    def test_success_and_failure_both_request_sleep(self):
        self.assertEqual(self.exercise(0)['status'], 'job-passed-sleep-requested')
        self.assertEqual(self.exercise(7)['job_exit_code'], 7)

    def test_timeout_and_interrupt_still_request_sleep(self):
        self.assertEqual(self.exercise(subprocess.TimeoutExpired('job', 30))['job_exit_code'], 124)
        self.assertEqual(self.exercise(KeyboardInterrupt())['job_exit_code'], 130)

    def test_sleep_failure_is_not_reported_as_success(self):
        self.assertEqual(self.exercise(0, sleep=1)['status'], 'failed')


if __name__ == '__main__':
    unittest.main()

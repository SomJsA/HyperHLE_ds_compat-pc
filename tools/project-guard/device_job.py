"""Explicit opt-in device job wrapper: always request screen off on exit."""
import argparse
import json
from pathlib import Path
import subprocess
import sys


def run_job(adb, serial, command, output, timeout, runner=subprocess.run):
    result = {'job_exit_code': 127, 'sleep_requested': False, 'device_queries': 0}
    try:
        with (output/'job.log').open('x', encoding='utf-8') as log:
            result['job_exit_code'] = runner(command, stdout=log, stderr=subprocess.STDOUT, timeout=timeout).returncode
    except subprocess.TimeoutExpired:
        result.update(job_exit_code=124, error='Owned job timed out')
    except KeyboardInterrupt:
        result.update(job_exit_code=130, error='Job interrupted')
    except OSError as error:
        result['error'] = str(error)
    finally:
        result['sleep_requested'] = True
        try:
            with (output/'sleep.log').open('x', encoding='utf-8') as log:
                result['sleep_exit_code'] = runner(
                    [str(adb), '-s', serial, 'shell', 'input', 'keyevent', 'KEYCODE_SLEEP'],
                    stdout=log, stderr=subprocess.STDOUT, timeout=15).returncode
        except (OSError, subprocess.TimeoutExpired) as error:
            result.update(sleep_exit_code=1, sleep_error=str(error))
    result['status'] = 'job-passed-sleep-requested' if result['job_exit_code'] == result['sleep_exit_code'] == 0 else 'failed'
    with (output/'result.json').open('x', encoding='utf-8') as stream:
        json.dump(result, stream, ensure_ascii=False, indent=2)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--adb', type=Path, required=True)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--timeout', type=float, default=300)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command or not args.serial.strip() or args.timeout <= 0 or args.output.exists():
        parser.error('Require explicit serial, command, positive timeout and a new output directory')
    args.output.mkdir(parents=True, exist_ok=False)
    result = run_job(args.adb, args.serial, command, args.output, args.timeout)
    print(json.dumps(result, ensure_ascii=True))
    return result['job_exit_code'] or result['sleep_exit_code']


if __name__ == '__main__':
    sys.exit(main())

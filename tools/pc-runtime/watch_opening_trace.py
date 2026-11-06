"""Gate diagnostic draws using user-created F7/F5 artifacts; no UI input."""
import argparse
import json
import time
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('evidence')
a = p.parse_args()
root = Path(a.evidence).resolve()
gate = root / 'draw-trace.enabled'
started = time.monotonic()
armed = None
events = []
try:
    while time.monotonic() - started < 900:
        if armed is None and any(root.glob('mark_*.ppm')):
            gate.write_text('F7 marked by user\n', encoding='utf-8')
            armed = time.monotonic()
            events.append({'event': 'armed', 'elapsed': armed - started})
        if armed is not None:
            # F5 dump creation follows the user's mark at the faulty moment.
            if any(root.glob('state-*.dsstate')) or time.monotonic() - armed > 120:
                events.append({'event': 'stopped', 'elapsed': time.monotonic() - started})
                break
        time.sleep(0.1)
finally:
    gate.unlink(missing_ok=True)
    (root / 'trace-gate-events.json').write_text(json.dumps(events, indent=2), encoding='utf-8')

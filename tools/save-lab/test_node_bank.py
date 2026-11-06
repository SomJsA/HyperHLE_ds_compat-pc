import json
from pathlib import Path
import tempfile
import unittest

import cold_profiles
from node_bank import inspect_bank
from sbin import digest
from test_save_lab import fixture


class NodeBankTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        source = self.root / "source/Documents"
        (source / "var").mkdir(parents=True)
        (source / "var/00_CheckpointSave.sb").write_bytes(fixture())
        self.bank = self.root / "bank"
        self.entry = cold_profiles.snapshot(source, self.bank / "nodes", "node-one", check_local_process=False)
        cold_profiles.json_write(self.entry / "node.json", {"format": "deadspace.save-node.v1", "id": "node-one",
            "session": "一周目", "label": "电梯前", "reason": "automatic", "metadata": {"level_id": "level-a"}})
        (self.bank / "versions").mkdir()
        raw = fixture()
        (self.bank / "versions" / (digest(raw) + ".sb")).write_bytes(raw)

    def test_reads_labels_versions_and_validated_whole_group(self):
        result = inspect_bank(self.bank)
        self.assertEqual(result["nodes"][0]["label"], "电梯前")
        self.assertTrue(result["nodes"][0]["editor_supported"])
        self.assertEqual(result["versions"], 1)
        self.assertFalse(result["complete_playthrough_coverage_verified"])
        self.assertEqual(result["invalid_nodes"], [])

    def test_node_tampering_is_reported_without_activating_or_repairing(self):
        path = self.entry / "Documents/var/00_CheckpointSave.sb"
        path.write_bytes(b"changed")
        result = inspect_bank(self.bank)
        self.assertEqual(len(result["invalid_nodes"]), 1)
        self.assertEqual(result["nodes"], [])
        self.assertEqual(path.read_bytes(), b"changed")

    def test_pending_nodes_and_event_gaps_are_visible(self):
        (self.bank / "nodes/.stage-interrupted").mkdir()
        (self.bank / "events.jsonl").write_text(json.dumps({"sequence": 1, "stored": True}) + "\n" +
                json.dumps({"sequence": 3, "stored": False}) + "\n{partial", encoding="utf-8")
        (self.bank / "events-interrupted-old.txt").write_text("incomplete", encoding="utf-8")
        result = inspect_bank(self.bank)
        self.assertEqual(result["pending_nodes"], [".stage-interrupted"])
        self.assertEqual(result["events"], 2)
        self.assertEqual(result["unavailable_version_events"], 1)
        self.assertEqual(len(result["journal_gaps"]), 2)
        self.assertEqual(result["interrupted_event_files"], 1)


if __name__ == "__main__":
    unittest.main()

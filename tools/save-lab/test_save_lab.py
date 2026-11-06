import contextlib
import copy
import io
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import cold_profiles as profiles
import save_lab
from sbin import Document, SaveError, fnv1, pack_strings


def fixture(reverse=False, settings=False):
    """Independent tiny writer: non-leading root, packed final bool, string lists."""
    if settings:
        definitions = [
            (1, [("dlc_modules", 15, 1), ("brightness", 10, 1.0)]),
            (1, [("attack", 15, 2)]),
            (1, [("unlocked", 9, 1)]),
        ]
    else:
        definitions = [
            (1, [("player", 15, 1), ("dlc_modules", 15, 5), ("level", 13, "level-a")]),
            (1, [("inventory", 15, 2), ("hp", 5, 100)]),
            (1, [("credits", 5, 50), ("nodes", 5, 2), ("items", 15, 3)]),
            (2, (16, [("AMMO_PLASMA_CUTTER", 10)])),
            (1, [("unlocked", 9, 1)]),  # unreachable nodes must also be preserved
            (2, (13, ["attack"])),
        ]
    strings = ["player", "inventory", "hp", "credits", "nodes", "items", "dlc_modules", "level", "level-a",
               "AMMO_PLASMA_CUTTER", "attack", "brightness", "unlocked"]
    if reverse:
        strings = list(reversed(strings))

    def scalar(kind, value):
        if kind == 13:
            return struct.pack("<H", strings.index(value) + 1)
        if kind == 9:
            return bytes((value, 0xCD))
        return struct.pack("<f" if kind == 10 else "<I", value)

    data, entries = bytearray(b"\xCD" * 4), [None] * len(definitions)
    for index in [*range(1, len(definitions)), 0]:
        flag, value = definitions[index]
        data += b"\xCD" * ((-len(data)) % 4)
        entries[index] = (len(data) << 3) | flag
        if flag == 1:
            body = bytearray()
            for name, kind, item in value:
                encoded = scalar(kind, item)
                body += struct.pack("<HHI", strings.index(name) + 1, kind, 4 + len(body) + 8) + encoded
            size = 4 + len(body)
            if value and value[-1][1] == 9:
                size -= 1  # 13-byte boolean object, CD byte lies outside size
            data += struct.pack("<I", len(value) | (size << 16)) + body
        else:
            kind, items = value
            data += struct.pack("<II", kind, len(items))
            for item in items:
                data += (scalar(13, item[0]) + b"\xCD\xCD" + scalar(5, item[1])) if kind == 16 else scalar(kind, item)
    out = bytearray(b"SBIN\x02\0\0\0")
    for tag, payload in [(b"META", b"opaque"), (b"OHDR", struct.pack("<" + "I" * len(entries), *entries)),
                         (b"DATA", bytes(data)), (b"STRS", pack_strings(strings))]:
        out += tag + struct.pack("<II", len(payload), fnv1(payload)) + payload
        out += b"\xCD" * ((-len(out)) % 4)
    return bytes(out)


def semantics(document):
    return {path: (value.type, value.value) for path, value in document.walk().items() if value.type != 15}


class BinaryTests(unittest.TestCase):
    def setUp(self):
        self.raw = fixture()
        self.doc = Document(self.raw)

    def test_identity_and_nonleading_root(self):
        self.assertGreater(self.doc.entries[0] >> 3, 4)
        self.assertEqual(self.doc.rebuild({}), self.raw)
        self.assertEqual(self.doc.edit({p: v.value for p, v in self.doc.walk().items() if v.type != 15}), self.raw)

    def test_scalar_boundaries_padding_and_new_string(self):
        out = self.doc.edit({"player/inventory/credits": 0xFFFFFFFF, "level": "new-level"})
        new = Document(out)
        self.assertEqual(new.locate("player/inventory/credits").value, 0xFFFFFFFF)
        self.assertEqual(new.locate("level").value, "new-level")
        self.assertEqual(new.locate("player/hp").value, 100)
        settings = Document(fixture(settings=True))
        field = settings.locate("dlc_modules/attack/unlocked")
        edited = Document(settings.edit({"dlc_modules/attack/unlocked": 0}))
        self.assertEqual(edited.data[field.offset + 1], 0xCD)
        for value in (-1, 2**32, True, "10"):
            with self.subTest(value=value), self.assertRaises(SaveError):
                self.doc.edit({"player/hp": value})

    def test_cross_string_table_transplant(self):
        donor = Document(fixture(reverse=True))
        donor = Document(donor.replace_list("player/inventory/items", [{"type": "HEALTH_NEW", "count": 9}]))
        module = donor.export_module("player/inventory")
        result = Document(self.doc.import_module("player/inventory", module))
        self.assertEqual(result.locate("player/inventory/items/0/type").value, "HEALTH_NEW")
        self.assertEqual(result.locate("player/inventory/items/0/count").value, 9)
        self.assertEqual(result.locate("player/hp").value, 100)
        field = self.doc.locate("player/inventory")
        for offset, (left, right) in enumerate(zip(self.doc.data, result.data)):
            if not field.offset <= offset < field.offset + 4:
                self.assertEqual(left, right)
        self.assertEqual(result.by_tag[b"META"].payload, b"opaque")

    def test_grow_shrink_and_empty_inventory_and_string_lists(self):
        for items in ([], [{"type": "PENG", "count": 1}] * 30):
            result = Document(self.doc.replace_list("player/inventory/items", items))
            node = result.node(result.locate("player/inventory/items").value)
            self.assertEqual(len(node.items), len(items))
            self.assertEqual(result.locate("level").value, "level-a")
        result = Document(self.doc.replace_list("dlc_modules", ["attack", "defense", "loot"]))
        self.assertEqual(result.locate("dlc_modules/2").value, "loot")

    def test_summary_keeps_empty_containers_visible(self):
        result = Document(self.doc.replace_list("dlc_modules", []))
        self.assertEqual(result.summary()["containers"]["dlc_modules"]["count"], 0)
        detached = Document(result.detach("dlc_modules"))
        self.assertNotIn("dlc_modules", detached.summary()["containers"])

    def test_detach_reattach_and_root_replace(self):
        source = Document(fixture(settings=True))
        module = source.export_module("dlc_modules/attack")
        removed = Document(source.detach("dlc_modules/attack"))
        self.assertNotIn("dlc_modules/attack", removed.walk())
        restored = Document(removed.attach("dlc_modules", "attack", module))
        self.assertEqual(semantics(restored), semantics(source))
        attached = Document(source.attach("dlc_modules", "defense", module))
        self.assertEqual(attached.locate("dlc_modules/defense/unlocked").value, 1)
        root_copy = Document(source.import_module("", source.export_module("")))
        self.assertEqual(semantics(root_copy), semantics(source))

    def test_corruption_and_malformed_modules(self):
        for raw in (self.raw[:-1], self.raw[:100], self.raw[:28] + bytes((self.raw[28] ^ 1,)) + self.raw[29:]):
            with self.subTest(size=len(raw)), self.assertRaises(SaveError):
                Document(raw)
        module = self.doc.export_module("player")
        corrupt = copy.deepcopy(module)
        node = corrupt["nodes"][corrupt["root"]["ref"]]
        node["fields"][0]["ref"] = corrupt["root"]["ref"]
        with self.assertRaises(SaveError):
            self.doc.import_module("player", corrupt)
        with self.assertRaises(SaveError):
            self.doc.import_module("dlc_modules", module)
        with self.assertRaises(SaveError):
            self.doc.replace_list("dlc_modules", [123])

    def test_cli_never_overwrites_and_reports_semantic_diff(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source, changes, output = root / "in.sb", root / "changes.json", root / "out.sb"
            source.write_bytes(self.raw)
            changes.write_text('{"player/hp": 200}', encoding="utf-8")
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(save_lab.main(["edit", str(source), str(changes), "--out", str(source)]), 2)
                self.assertEqual(save_lab.main(["edit", str(source), str(changes), "--out", str(output)]), 0)
                self.assertEqual(save_lab.main(["edit", str(source), str(changes), "--out", str(output)]), 2)
            self.assertEqual(source.read_bytes(), self.raw)
            capture = io.StringIO()
            with contextlib.redirect_stdout(capture):
                self.assertEqual(save_lab.main(["diff", str(source), str(output)]), 0)
            self.assertEqual([c["path"] for c in json.loads(capture.getvalue())["changes"]], ["player/hp"])


class ProfileTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / "source" / "Documents"
        (self.source / "var").mkdir(parents=True)
        (self.source / "var" / "00_CheckpointSave.sb").write_bytes(fixture())
        (self.source / "var" / "Core.sb").write_bytes(fixture(settings=True))
        (self.source / "empty").mkdir()
        (self.source / ".metadata").write_bytes(b"opaque dependent file")
        self.bank = self.root / "bank"
        self.active = self.root / "runtime" / "Documents"
        self.backups = self.root / "backups"
        stopped = patch.object(profiles, "require_stopped")
        stopped.start()
        self.addCleanup(stopped.stop)

    def existing(self):
        self.active.mkdir(parents=True)
        (self.active / "old.txt").write_text("original", encoding="utf-8")
        return profiles.inventory(self.active)

    def test_whole_profile_switch_backup_and_restore(self):
        original = self.existing()
        entry = profiles.snapshot(self.source, self.bank, "profile-a")
        result = profiles.activate(entry, self.active, self.backups)
        self.assertEqual(profiles.inventory(self.active), profiles.inventory(self.source))
        self.assertTrue((self.active / ".metadata").exists())
        self.assertTrue((self.active / "empty").is_dir())
        self.assertFalse((self.active / "old.txt").exists())
        self.assertEqual(profiles.verify(result["backup"]), original)
        profiles.activate(result["backup"], self.active, self.root / "restore-backups")
        self.assertEqual(profiles.inventory(self.active), original)

    def test_failed_publish_rolls_back(self):
        original = self.existing()
        entry = profiles.snapshot(self.source, self.bank, "a")
        real_rename = profiles.os.rename

        def fail_stage(source, target):
            if Path(source).name.startswith(".save-lab-stage-") and Path(target) == self.active:
                raise OSError("injected directory publication failure")
            return real_rename(source, target)

        with patch.object(profiles.os, "rename", side_effect=fail_stage), self.assertRaises(OSError):
            profiles.activate(entry, self.active, self.backups)
        self.assertEqual(profiles.inventory(self.active), original)
        self.assertEqual(len(list(self.backups.glob("pre-*"))), 1)

    def test_failed_post_publish_verification_rolls_back(self):
        original = self.existing()
        entry = profiles.snapshot(self.source, self.bank, "a")
        real_inventory = profiles.inventory

        def fail_installed(path, *args, **kwargs):
            if Path(path) == self.active and (self.active / ".metadata").exists():
                raise SaveError("injected verification failure")
            return real_inventory(path, *args, **kwargs)

        with patch.object(profiles, "inventory", side_effect=fail_installed), self.assertRaises(SaveError):
            profiles.activate(entry, self.active, self.backups)
        self.assertEqual(profiles.inventory(self.active), original)
        self.assertEqual(len(list(self.active.parent.glob(".save-lab-failed-*"))), 1)

    def test_tampering_overlap_and_lock_rejected(self):
        self.existing()
        entry = profiles.snapshot(self.source, self.bank, "a")
        with self.assertRaises(SaveError):
            profiles.snapshot(self.source, self.source / "bank", "bad")
        with self.assertRaises(SaveError):
            profiles.snapshot(self.source, self.bank, "a")
        (entry / "Documents" / "extra.txt").write_text("unexpected", encoding="utf-8")
        with self.assertRaises(SaveError):
            profiles.activate(entry, self.active, self.backups)
        self.assertEqual((self.active / "old.txt").read_text(), "original")
        for name in ("../outside", "C:/outside", "dir\\bad", "NUL.txt", "bad."):
            with self.subTest(name=name), self.assertRaises(SaveError):
                profiles.relative_name(name)
        (self.bank / ".save-lab-bank.lock").touch()
        with self.assertRaises(SaveError):
            profiles.snapshot(self.source, self.bank, "b")

    def test_manifest_traversal_and_corrupt_save_rejected(self):
        entry = profiles.snapshot(self.source, self.bank, "a")
        manifest_path = entry / "manifest.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["files"]["../outside"] = {"bytes": 0, "sha256": "0" * 64}
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaises(SaveError):
            profiles.verify(entry)
        (self.source / "var" / "bad.sb").write_bytes(b"SBINbroken")
        with self.assertRaises(SaveError):
            profiles.snapshot(self.source, self.bank, "b")
        self.assertFalse((self.bank / "b").exists())

    def test_process_guard_failure_blocks_mutation(self):
        with patch.object(profiles, "require_stopped", side_effect=SaveError("game running")), self.assertRaises(SaveError):
            profiles.snapshot(self.source, self.bank, "blocked")
        self.assertFalse(self.bank.exists())


class ProcessGuardTests(unittest.TestCase):
    def test_actual_process_enumeration_rejects_game(self):
        completed = subprocess.CompletedProcess([], 0)
        completed.stdout = b'"touchHLE-test.exe","123","Console","1","1024 K"\r\n' if profiles.os.name == "nt" else "touchHLE-test\n"
        with patch.object(profiles.subprocess, "run", return_value=completed), self.assertRaises(SaveError):
            profiles.require_stopped()

    def test_enumeration_failure_fails_closed(self):
        with patch.object(profiles.subprocess, "run", side_effect=subprocess.TimeoutExpired("tasklist", 15)), self.assertRaises(SaveError):
            profiles.require_stopped()


if __name__ == "__main__":
    unittest.main()

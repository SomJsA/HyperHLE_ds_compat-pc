import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

from adb_profiles import AdbProfiles, create_tar, extract_tar
import cold_profiles
import progress
from profile_ops import slot_copy
from sbin import Document, SaveError
from test_save_lab import fixture, semantics


def object_module(fields):
    return {"format": "deadspace.sbin.module.v1", "root": {"type": 15, "ref": "root"},
            "nodes": {"root": {"kind": "object", "fields": fields}}}


def story_fixture():
    doc = Document(fixture())
    for name, fields in (
        ("current_level", [{"name": "level_id", "type": 13, "value": "level-a"}]),
        ("actors", [{"name": "state", "type": 5, "value": 0}]),
        ("objectives", [{"name": "state", "type": 5, "value": 1}]),
        ("misc", [{"name": "time", "type": 5, "value": 100}]),
    ):
        doc = Document(doc.attach("", name, object_module(fields)))
    doc = Document(doc.attach("player", "transform", object_module([{"name": "x", "type": 10, "value": 0.0}])))
    doc = Document(doc.attach("", "triggers", {"format": "deadspace.sbin.module.v1", "root": {"type": 15, "ref": "r"},
        "nodes": {"r": {"kind": "list", "element_type": 13, "items": [{"type": 13, "value": "trigger-a"}]}}}))
    return doc


class ProgressTests(unittest.TestCase):
    def test_transplant_moves_story_and_position_but_keeps_equipment(self):
        target = story_fixture()
        donor = Document(target.edit({"current_level/level_id": "level-b", "actors/state": 2,
                                     "objectives/state": 2, "misc/time": 200, "player/transform/x": 9.0,
                                     "player/inventory/credits": 999}))
        output, paths = progress.transplant(target, donor)
        result = Document(output)
        self.assertEqual(result.locate("current_level/level_id").value, "level-b")
        self.assertEqual(result.locate("actors/state").value, 2)
        self.assertEqual(result.locate("objectives/state").value, 2)
        self.assertEqual(result.locate("player/transform/x").value, 9.0)
        self.assertEqual(result.locate("player/inventory/credits").value, 50)
        self.assertEqual(set(paths), {*progress.PROGRESS_ROOTS, "player/transform"})

    def test_incompatible_story_schema_rejected(self):
        target = story_fixture()
        donor = Document(target.detach("actors"))
        with self.assertRaises(SaveError):
            progress.transplant(target, donor)

    def test_anchor_roundtrip_and_unexpected_path_rejected(self):
        target = story_fixture()
        anchor = progress.export_anchor(target)
        output, _ = progress.apply_anchor(target, anchor)
        self.assertEqual(semantics(Document(output)), semantics(target))
        anchor["modules"]["player/inventory"] = target.export_module("player/inventory")
        with self.assertRaises(SaveError):
            progress.apply_anchor(target, anchor)


class AdbTransportTests(unittest.TestCase):
    def test_tar_roundtrip_preserves_dependencies_and_empty_dirs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "Documents"
            (source / "var").mkdir(parents=True)
            (source / "empty").mkdir()
            (source / "var" / "Core.sb").write_bytes(fixture())
            (source / "loggedIn").write_bytes(b"opaque")
            (source / "var" / "last_version.txt").write_bytes(b"1.3.34")
            archive = root / "Documents.tar"
            create_tar(source, archive)
            result = root / "result"
            extract_tar(archive.read_bytes(), result)
            self.assertEqual(cold_profiles.inventory(source), cold_profiles.inventory(result))

    def test_unsafe_tar_members_rejected(self):
        for name, kind in (("../outside", tarfile.REGTYPE), ("/outside", tarfile.REGTYPE),
                           ("link", tarfile.SYMTYPE), ("link", tarfile.LNKTYPE)):
            data = io.BytesIO()
            with tarfile.open(fileobj=data, mode="w:") as archive:
                entry = tarfile.TarInfo(name)
                entry.type = kind
                entry.linkname = "../../outside"
                archive.addfile(entry)
            with self.subTest(name=name, kind=kind), tempfile.TemporaryDirectory() as tmp:
                with self.assertRaises(SaveError):
                    extract_tar(data.getvalue(), Path(tmp) / "out")

    def test_other_package_can_never_be_stopped_or_mutated(self):
        client = AdbProfiles("adb", "serial", "org.touchhle.android")
        with patch.object(client, "call") as call, self.assertRaises(SaveError):
            client.stop()
        call.assert_not_called()

    def test_remote_process_detection(self):
        client = AdbProfiles("adb", "serial")
        completed = type("Result", (), {"stdout": b"NAME\norg.touchhle.android.savelab:game\n"})()
        with patch.object(client, "call", return_value=completed), self.assertRaises(SaveError):
            client.require_stopped()


class ProfileSlotTests(unittest.TestCase):
    def test_whole_slot_and_story_pair_keep_shared_files(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(cold_profiles, "require_stopped"):
            root = Path(temporary)
            target, donor = root / "target/Documents", root / "donor/Documents"
            (target / "var").mkdir(parents=True)
            (donor / "var").mkdir(parents=True)
            left = story_fixture()
            right = Document(left.edit({"current_level/level_id": "level-b", "player/inventory/credits": 999}))
            for suffix in ("CheckpointSave", "LevelSave", "SurvivalSave"):
                (target / f"var/00_{suffix}.sb").write_bytes(left.original)
                (donor / f"var/02_{suffix}.sb").write_bytes(right.original)
            (target / "var/settings.sb").write_bytes(fixture(settings=True))
            (donor / "var/settings.sb").write_bytes(fixture(settings=True))
            t = cold_profiles.snapshot(target, root / "inputs", "target")
            d = cold_profiles.snapshot(donor, root / "inputs", "donor")
            for story_only in (False, True):
                result = slot_copy(t, d, 0, 2, root / "outputs", str(story_only), story_only)
                document = Document((result / "Documents/var/00_CheckpointSave.sb").read_bytes())
                self.assertEqual(document.locate("current_level/level_id").value, "level-b")
                self.assertEqual(document.locate("player/inventory/credits").value, 50 if story_only else 999)
                self.assertEqual((result / "Documents/var/settings.sb").read_bytes(), (target / "var/settings.sb").read_bytes())
            self.assertEqual(Document((target / "var/00_CheckpointSave.sb").read_bytes()).locate("player/inventory/credits").value, 50)


if __name__ == "__main__":
    unittest.main()

import json
import pathlib
import tempfile
import unittest
import zipfile

import vandal_marker


class VandalMarkerTests(unittest.TestCase):
    def fixture(self, root: pathlib.Path):
        payload = root / "payload"
        payload.mkdir(parents=True)
        (payload / "DeadSpace.ipa").write_bytes(b"ipa")
        for tier in ("2", "4", "8"):
            (payload / "texture-packs" / tier).mkdir(parents=True)
            (payload / "texture-packs" / tier / "PACK.json").write_text(
                '{"tier":' + tier + "}",
                encoding="utf-8",
            )

    def test_create_verify_and_tamper_warning(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary) / "input"
            self.fixture(root)
            marker = pathlib.Path(temporary) / "vandal.marker"
            manifest = vandal_marker.create_marker(root, marker, "R15-test", "test")
            self.assertEqual(manifest["game"]["bundle_id"], "com.ea.deadspace.bv")
            self.assertEqual(manifest["game"]["target_version"], "1.3.34")
            valid = vandal_marker.verify_marker(
                marker,
                "R15-other",
                {"android": 1, "pc": 1},
            )
            self.assertTrue(valid["operable"])
            self.assertTrue(valid["warnings"])

            tampered = pathlib.Path(temporary) / "tampered.marker"
            with zipfile.ZipFile(marker, "r") as source, zipfile.ZipFile(
                tampered, "x"
            ) as destination:
                for info in source.infolist():
                    data = source.read(info)
                    if info.filename == "payload/texture-packs/8/PACK.json":
                        data = b"changed"
                    destination.writestr(info, data)
            result = vandal_marker.verify_marker(tampered, full=True)
            self.assertTrue(result["operable"])
            self.assertTrue(any("SHA-256 mismatch" in item for item in result["warnings"]))

    def test_symlink_input_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary) / "input"
            self.fixture(root)
            link = root / "payload" / "texture-packs" / "8" / "linked"
            try:
                link.symlink_to(root / "payload" / "DeadSpace.ipa")
            except OSError:
                self.skipTest("symlink capability unavailable")
            marker = pathlib.Path(temporary) / "vandal.marker"
            with self.assertRaises(ValueError):
                vandal_marker.create_marker(root, marker, "R15-test", "test")

    def test_non_texture_payload_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary) / "input"
            self.fixture(root)
            (root / "payload" / "program").mkdir()
            (root / "payload" / "program" / "touchHLE.exe").write_bytes(b"program")
            marker = pathlib.Path(temporary) / "vandal.marker"
            with self.assertRaises(ValueError):
                vandal_marker.create_marker(root, marker, "R15-test", "test")

    def test_duplicate_zip_names_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            marker = pathlib.Path(temporary) / "vandal.marker"
            manifest = {
                "schema": 1,
                "kind": vandal_marker.KIND,
                "entries": [],
            }
            with zipfile.ZipFile(marker, "x") as archive:
                archive.writestr("vandal.json", json.dumps(manifest))
                archive.writestr("vandal.json", json.dumps(manifest))
            with self.assertRaises(ValueError):
                vandal_marker.read_manifest(marker)

    def test_detect_direct_ipa(self):
        with tempfile.TemporaryDirectory() as temporary:
            ipa = pathlib.Path(temporary) / "DeadSpace.ipa"
            with zipfile.ZipFile(ipa, "x") as archive:
                archive.writestr(
                    "Payload/Dead Space.app/Info.plist",
                    vandal_marker.plistlib.dumps(
                        {
                            "CFBundleIdentifier": "com.ea.deadspace.bv",
                            "CFBundleVersion": "1.3.34",
                        }
                    ),
                )
            result = vandal_marker.detect_ipa(ipa)
            self.assertTrue(result["is_dead_space_ipa"])
            self.assertEqual(result["bundle_version"], "1.3.34")
            self.assertEqual(result["warnings"], [])

    def test_other_ipa_version_only_warns(self):
        with tempfile.TemporaryDirectory() as temporary:
            ipa = pathlib.Path(temporary) / "DeadSpace.ipa"
            with zipfile.ZipFile(ipa, "x") as archive:
                archive.writestr(
                    "Payload/Dead Space.app/Info.plist",
                    vandal_marker.plistlib.dumps(
                        {
                            "CFBundleIdentifier": "com.ea.deadspace.bv",
                            "CFBundleVersion": "1.4.0",
                        }
                    ),
                )
            result = vandal_marker.detect_ipa(ipa)
            self.assertTrue(result["is_dead_space_ipa"])
            self.assertTrue(result["warnings"])

    def test_optional_install_is_content_addressed_and_reused(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = pathlib.Path(temporary)
            root = base / "input"
            self.fixture(root)
            marker = base / "vandal.marker"
            vandal_marker.create_marker(root, marker, "R15-test", "test")
            output = base / "Documents" / "DeadSpaceLegacy"
            installed = vandal_marker.install_marker(
                marker,
                output,
                {"4"},
            )
            destination = output / "marker-content" / installed["marker_id"]
            self.assertTrue((destination / "payload" / "DeadSpace.ipa").is_file())
            self.assertTrue(installed["base_data"]["installed"])
            self.assertTrue(
                (destination / "payload" / "texture-packs" / "4" / "PACK.json").is_file()
            )
            self.assertFalse((destination / "payload" / "texture-packs" / "8").exists())
            added = vandal_marker.install_marker(marker, output, {"8"})
            self.assertEqual(added["texture_tiers"], ["4", "8"])
            self.assertTrue(
                (destination / "payload" / "texture-packs" / "8" / "PACK.json").is_file()
            )
            reused = vandal_marker.install_marker(marker, output, {"4", "8"})
            self.assertEqual(reused["marker_id"], installed["marker_id"])
            self.assertEqual(reused["texture_tiers"], ["4", "8"])

    def test_base_data_is_installed_when_no_texture_tier_is_selected(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = pathlib.Path(temporary)
            root = base / "input"
            self.fixture(root)
            marker = base / "vandal.marker"
            vandal_marker.create_marker(root, marker, "R15-test", "test")
            output = base / "Documents" / "DeadSpaceLegacy"
            installed = vandal_marker.install_marker(marker, output, set())
            destination = output / "marker-content" / installed["marker_id"]
            self.assertTrue(installed["base_data"]["installed"])
            self.assertEqual(installed["texture_tiers"], [])
            self.assertTrue((destination / "payload" / "DeadSpace.ipa").is_file())
            self.assertFalse((destination / "payload" / "texture-packs" / "2").exists())

    def test_selected_tier_verification_skips_other_payload_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = pathlib.Path(temporary)
            root = base / "input"
            self.fixture(root)
            marker = base / "vandal.marker"
            vandal_marker.create_marker(root, marker, "R15-test", "test")
            tampered = base / "tampered.marker"
            with zipfile.ZipFile(marker, "r") as source, zipfile.ZipFile(
                tampered, "x"
            ) as destination:
                for info in source.infolist():
                    data = source.read(info)
                    if info.filename == "payload/texture-packs/8/PACK.json":
                        data = b"changed"
                    destination.writestr(info, data)
            selected = vandal_marker.verify_marker(
                tampered,
                prefixes={"payload/texture-packs/4/"},
            )
            self.assertTrue(selected["operable"])
            self.assertFalse(selected["warnings"])
            full = vandal_marker.verify_marker(tampered, full=True)
            self.assertTrue(any("SHA-256 mismatch" in item for item in full["warnings"]))

    def test_six_x_texture_tier_can_be_packaged_and_selected(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = pathlib.Path(temporary)
            root = base / "input"
            payload = root / "payload"
            payload.mkdir(parents=True)
            (payload / "DeadSpace.ipa").write_bytes(b"ipa")
            for tier in ("2", "4", "6"):
                (payload / "texture-packs" / tier).mkdir(parents=True)
                (payload / "texture-packs" / tier / "PACK.json").write_text(
                    '{"tier":' + tier + "}",
                    encoding="utf-8",
                )
            marker = base / "vandal.marker"
            manifest = vandal_marker.create_marker(
                root,
                marker,
                "R15-six",
                "test",
            )
            self.assertEqual(set(manifest["texture_tiers"]), {"2", "4", "6"})
            output = base / "Documents" / "DeadSpaceLegacy"
            installed = vandal_marker.install_marker(marker, output, {"6"})
            destination = output / "marker-content" / installed["marker_id"]
            self.assertEqual(installed["texture_tiers"], ["6"])
            self.assertTrue(
                (destination / "payload" / "texture-packs" / "6" / "PACK.json").is_file()
            )


if __name__ == "__main__":
    unittest.main()

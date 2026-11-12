#!/usr/bin/env python3
"""Stage the lean PC program payload for the two-file installer delivery."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil


def sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def read_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def write_json(path: Path, value: dict) -> None:
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--launcher", required=True, type=Path)
    parser.add_argument("--native", required=True, type=Path)
    parser.add_argument("--pdb", required=True, type=Path)
    parser.add_argument("--diagnostic-map", required=True, type=Path)
    parser.add_argument("--repo", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--implementation-commit", required=True)
    parser.add_argument("--report", required=True, type=Path)
    args = parser.parse_args()

    source = args.source.resolve()
    launcher = args.launcher.resolve()
    repo = args.repo.resolve()
    output = args.output.resolve()
    if output.exists():
        raise FileExistsError(output)
    manifest_path = source / "Game/manifest.json"
    if not manifest_path.is_file():
        raise FileNotFoundError(manifest_path)
    if not launcher.is_file():
        raise FileNotFoundError(launcher)
    if (source / "Game/DeadSpace.ipa").exists():
        raise ValueError("Source program package contains the original IPA")
    if (source / "Assets/texture-packs").exists():
        raise ValueError("Source program package contains texture packs")

    manifest = read_json(manifest_path)
    expected = manifest["files"]
    actual = {
        path.relative_to(source).as_posix()
        for path in source.rglob("*")
        if path.is_file()
    }
    if actual != set(expected) | {"Game/manifest.json"}:
        raise ValueError("Source package has unmanifested or missing files")
    for name, digest in expected.items():
        if sha(source / name) != digest:
            raise ValueError("Source package hash mismatch: " + name)

    output.mkdir(parents=True)
    for path in source.rglob("*"):
        if not path.is_file():
            continue
        relative = path.relative_to(source)
        target = output / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)

    shutil.copy2(launcher, output / "DeadSpace Launcher.exe")
    shutil.copy2(args.native.resolve(), output / "Game/touchHLE.exe")
    shutil.copy2(args.pdb.resolve(), output / "Game/touchHLE.pdb")
    shutil.copy2(args.diagnostic_map.resolve(), output / "Game/diagnostic-map.json")
    readme = output / "Docs/INSTALLER-README.txt"
    readme.write_text(
        "DEAD SPACE MOBILE PC installation payload - 20261112-R4\n\n"
        "This directory is embedded inside the installation EXE. It is not a\n"
        "user-facing floating package.\n\n"
        "Program files default to %LOCALAPPDATA%\\Programs\\DeadSpaceLegacy.\n"
        "Game data default to %LOCALAPPDATA%\\DeadSpaceLegacy\\GameData.\n"
        "Documents stores launcher configuration and the original three save slots.\n\n"
        "The separate vandal.marker is required only for first installation or\n"
        "resource-tier changes and remains read-only. An existing installation\n"
        "can be upgraded or downgraded using only the new installer EXE;\n"
        "program updates preserve GameData and Documents.\n\n"
        "3D acceleration mode is enabled by default; disabling it selects CPU-copy\n"
        "compatibility mode. Automatic diagnostics and the performance HUD default\n"
        "to off. Explicit F7 captures remain available without automatic history.\n",
        encoding="ascii",
    )
    (output / "Docs/DIAGNOSTICS-README.txt").write_text(
        "1. Reproduce the issue and note whether 3D acceleration mode was on.\n"
        "2. Press F7 for a problem capture.\n"
        "3. Close the game normally.\n"
        "4. Export all diagnostics sessions from the launcher.\n\n"
        "No automatic screenshots, keystrokes, saves or uploads occur.\n",
        encoding="ascii",
    )

    defaults = output / "Defaults/DeadSpace-Launcher.ini"
    replacements = {
        "ipa": "",
        "texture_scale": "4",
        "texture_root_2": "",
        "texture_root_4": "",
        "texture_root_6": "",
        "install_root": "",
        "gpu_present_copy": "1",
        "native_hud": "1",
        "play_diagnostics": "0",
        "diagnostic_defaults_policy": "manual-f7-20261107",
        "diagnostic_detail": "2",
        "super_monitor": "0",
    }
    seen = set()
    lines = []
    for line in defaults.read_text(encoding="utf-8").splitlines():
        if "=" in line:
            key = line.split("=", 1)[0]
            if key in replacements:
                lines.append(key + "=" + replacements[key])
                seen.add(key)
                continue
        lines.append(line)
    for key, value in replacements.items():
        if key not in seen:
            lines.append(key + "=" + value)
    defaults.write_text("\n".join(lines) + "\n", encoding="utf-8")

    native = output / "Game/touchHLE.exe"
    diagnostic_map = output / "Game/diagnostic-map.json"
    launcher_sha = sha(output / "DeadSpace Launcher.exe")
    native_sha = sha(native)
    map_sha = sha(diagnostic_map)
    marker_id = "1d88478ce864937e8d9aedaa5231fa5c0d29829a1ce0ad98948cf72b7036ff90"
    marker_sha = "31eed3209ec3207cfb82d52b82dca1a41f4fe4240525fdb4cdbf5f646187577b"
    install_root = (
        "%LOCALAPPDATA%\\DeadSpaceLegacy\\GameData\\marker-content\\"
        "<marker_id>"
    )

    source_names = set(manifest.get("source_sha256", {}))
    source_names.update(
        {
            "src/window.rs",
            "src/gles/present_attrib.rs",
            "src/gles/ds_native_fog.rs",
            "src/gles/ds_fog_scope.rs",
            "src/ds_shader_projection.rs",
            "src/environment.rs",
            "src/frameworks/opengles.rs",
            "src/lib.rs",
            "src/gles/gles2_native.rs",
            "src/gles/gles3_native.rs",
            "src/gles/gles2_on_gl3.rs",
            "src/frameworks/opengles/gles_guest.rs",
            "src/fs/ds_resource_overlay.rs",
            "src/gles/gles2_glsl.rs",
            "src/gles.rs",
            "src/frameworks/opengles/eagl.rs",
            "src/ds_overlay_gpu.rs",
            "src/frameworks/opengles/eagl_fog_probe.rs",
            "src/ds_overlay_gpu_es2.rs",
            "src/ds_overlay_gpu_r4_tests.rs",
            "dev-docs/R4-NATIVE-EFFECTS.md",
            "src/frameworks/opengles/eagl_r4_tests.rs",
            "res/overlay/deadspace_subtitles.sb",
            "src/fs.rs",
            "src/fs/ds_weapon_layout.rs",
            "src/fs/ds_pc_weapon_layout.rs",
            "src/ds_hud_commands.rs",
            "src/ds_tutorial_text.rs",
            "tools/pc-runtime/Program.cs",
            "tools/pc-runtime/LauncherShell.cs",
            "tools/pc-runtime/StorageControls.cs",
            "tools/pc-runtime/MarkerInstaller.cs",
            "tools/pc-installer/Installer.cs",
            "tools/pc-installer/InstallerForm.cs",
            "tools/pc-installer/Uninstaller.cs",
            "tools/pc-installer/Build-Installer.ps1",
            "tools/pc-installer/Stage-Program.py",
            "tools/pc-runtime/Build-DiagnosticMap.py",
            "tools/pc-runtime/Build-Launcher.ps1",
            "tools/pc-runtime/Windows.manifest",
            "tools/pc-runtime/LauncherLayout.cs",
            "tools/pc-runtime/ReleaseUi.cs",
            "tools/pc-runtime/LauncherAssemblyInfo.cs",
            "tools/pc-installer/InstallerLanguage.cs",
            "tools/pc-installer/InstallerAssemblyInfo.cs",
            "tools/pc-installer/UninstallerAssemblyInfo.cs",

            "tools/pc-runtime/assets/DeadSpace.ico",
            "dev-docs/PC-INSTALLER-DISTRIBUTION.md",
            "dev-docs/PC-WEAPON-ORDER.md",
            "dev-docs/PC-DISTRIBUTION-LAYOUT.md",
        }
    )
    source_hashes = {}
    for name in sorted(source_names):
        path = repo / name
        if path.is_file():
            source_hashes[name] = sha(path)

    build_info_path = output / "Game/build-info.json"
    build_info = read_json(build_info_path)
    build_info.update(
        implementation_commit=args.implementation_commit,
        native_implementation_commit=args.implementation_commit,
        launcher_implementation_commit=args.implementation_commit,
        native_code_commit=args.implementation_commit,
        distribution_id=output.name,
        release_tag="20261112-r4",
        launcher_sha256=launcher_sha,
        native_sha256=native_sha,
        native_bank_sha256=sha(repo / "res/overlay/deadspace_iphone.bin"),
        diagnostic_map_sha256=map_sha,
        marker_required=True,
        marker_required_for_upgrade=False,
        marker_id=marker_id,
        marker_sha256=marker_sha,
        marker_install_root=install_root,
        purpose=(
            "PC internal installer payload: separate selectable program and "
            "GameData roots, default 3D acceleration, prepared diagnostics"
        ),
        final_acceptance="open",
    )
    write_json(build_info_path, build_info)

    manifest.update(
        implementation_commit=args.implementation_commit,
        native_implementation_commit=args.implementation_commit,
        launcher_implementation_commit=args.implementation_commit,
        native_code_commit=args.implementation_commit,
        distribution_id=output.name,
        release_tag="20261112-r4",
        launcher_sha256=launcher_sha,
        native_sha256=native_sha,
        native_bank_sha256=sha(repo / "res/overlay/deadspace_iphone.bin"),
        diagnostic_map_sha256=map_sha,
        marker_required=True,
        marker_required_for_upgrade=False,
        marker_id=marker_id,
        marker_sha256=marker_sha,
        marker_install_root=install_root,
        source_sha256=source_hashes,
        status=(
            "Internal PC installer payload; program and GameData are separate "
            "user-selected paths; full R15 open"
        ),
    )
    manifest["installer_distribution"] = {
        "delivery": ["installer-exe", "vandal.marker"],
        "default_program_root": "%LOCALAPPDATA%\\Programs\\DeadSpaceLegacy",
        "default_game_data_root": "%LOCALAPPDATA%\\DeadSpaceLegacy\\GameData",
        "documents": "configuration-and-three-slot-saves-only",
        "program_updates_preserve_game_data": True,
        "marker_read_only": True,
        "marker_required_for_upgrade": False,
        "direct_ipa_supported": True,
        "uninstall_removes_game_data": False,
        "multiple_texture_tiers": True,
        "base_only_marker_install": True,
        "desktop_shortcut_default": True,
        "start_menu_shortcut_default": True,
    }
    manifest["files"] = {
        path.relative_to(output).as_posix(): sha(path)
        for path in sorted(output.rglob("*"))
        if path.is_file() and path != output / "Game/manifest.json"
    }
    write_json(output / "Game/manifest.json", manifest)

    result = {
        "status": "program-payload-staged",
        "program_package": str(output),
        "implementation_commit": args.implementation_commit,
        "files": len(manifest["files"]),
        "bytes": sum(
            path.stat().st_size
            for path in output.rglob("*")
            if path.is_file()
        ),
        "native_sha256": native_sha,
        "launcher_sha256": launcher_sha,
        "diagnostic_map_sha256": map_sha,
        "marker_required": True,
        "contains_game_ipa": False,
        "contains_texture_packs": False,
    }
    write_json(args.report.resolve(), result)
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

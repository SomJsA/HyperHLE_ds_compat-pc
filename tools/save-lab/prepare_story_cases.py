"""Build a reproducible story-point experiment matrix from two verified sources."""
import argparse
import json
from pathlib import Path
import shutil

import cold_profiles
import progress
from sbin import Document, digest


def prepare(tablet_entry, donor_var, output):
    cold_profiles.verify(tablet_entry)
    output.mkdir()
    work, bank = output / "work", output / "bank"
    work.mkdir()
    original = tablet_entry / "Documents"
    canonical = work / "canonical" / "Documents"
    shutil.copytree(original, canonical)
    for file in donor_var.glob("*.sb"):
        if file.name != "settings.sb":
            shutil.copy2(file, canonical / "var" / file.name)
    settings_path = canonical / "var/settings.sb"
    settings = Document(settings_path.read_bytes())
    settings_path.write_bytes(settings.edit({"profile_index": 0}))
    cases = []

    def add(key, title, source, labels, modify=None):
        documents = work / key / "Documents"
        shutil.copytree(source, documents)
        if modify:
            modify(documents)
        entry = cold_profiles.snapshot(documents, bank, key)
        checkpoint = Document((documents / "var/00_CheckpointSave.sb").read_bytes())
        # Keep the exact filled values, including every leaf of copied modules.
        changes = []
        for suffix in ("CheckpointSave", "LevelSave", "SurvivalSave"):
            filename = f"00_{suffix}.sb"
            before = Document((canonical / "var" / filename).read_bytes()).summary()
            after = Document((documents / "var" / filename).read_bytes()).summary()
            left = {**before["containers"], **before["values"]}
            right = {**after["containers"], **after["values"]}
            for path in sorted(left.keys() | right.keys()):
                a = {k: v for k, v in left.get(path, {}).items() if k != "data_offset"}
                b = {k: v for k, v in right.get(path, {}).items() if k != "data_offset"}
                if a != b:
                    changes.append({"file": filename, "path": path, "before": a or None, "after": b or None})
        (output / f"{key}-fields.json").write_text(json.dumps(changes, ensure_ascii=False, indent=2), encoding="utf-8")
        cases.append({"id": key, "title": title, "entry": str(entry.resolve()), "labels": labels,
                      "level_id": checkpoint.locate("current_level/level_id").value,
                      "checkpoint_sha256": digest(checkpoint.original), "changed_paths": len(changes),
                      "fields_json": str((output / f"{key}-fields.json").resolve())})

    def pair(documents, operation):
        for suffix in ("CheckpointSave", "LevelSave"):
            path = documents / f"var/00_{suffix}.sb"
            target = Document(path.read_bytes())
            donor = Document((donor_var / f"02_{suffix}.sb").read_bytes())
            path.write_bytes(operation(target, donor))

    def clear_triggers(documents):
        path = documents / "var/00_CheckpointSave.sb"
        document = Document(path.read_bytes())
        module = document.export_module("triggers")
        key = module["root"]["ref"]
        node = module["nodes"][key]
        node["items"] = []
        module["nodes"] = {key: node}
        path.write_bytes(document.import_module("triggers", module))

    def full_slot(documents):
        for suffix in ("CheckpointSave", "LevelSave", "SurvivalSave"):
            shutil.copy2(donor_var / f"02_{suffix}.sb", documents / f"var/00_{suffix}.sb")

    add("A-tablet", "A 平板现状：关卡与剧情状态不配套", original,
        ["未改字段（现有档对照）", "level_id=01_gov_01", "objectives=OBJ_MINES_02_*；actors=矿区状态"])
    add("B-canonical", "B 原始检查点：完整矿区对照", canonical,
        ["Checkpoint + Level = 原2012槽00", "level_id=02_mines_02；XYZ=(-12.7793,1,5.3407)", "actors/objectives/triggers 保持配套"])
    add("C-level-only", "C 仅改关卡 ID", canonical,
        ["Checkpoint + Level: current_level/level_id=01_gov_02", "XYZ仍=(-12.7793,1,5.3407)", "actors/objectives/triggers 仍为 MINES_02"],
        lambda documents: pair(documents, lambda target, donor: target.edit({"current_level/level_id": "01_gov_02"})))
    add("D-position-only", "D 仅移植坐标", canonical,
        ["level_id 仍=02_mines_02", "player/transform/[9,10,11]=(19.7084,1,-44.0500)", "Checkpoint/Level 坐标取各自原02槽；剧情状态不改"],
        lambda documents: pair(documents, lambda target, donor: target.edit(
            {f"player/transform/{i}": donor.locate(f"player/transform/{i}").value for i in (9, 10, 11)})))
    add("E-no-triggers", "E 清空待处理剧情触发器", canonical,
        ["Checkpoint: triggers.count=0（原为1）", "移除 mine_02_tunnels / OBJ_MINES_02_01", "level_id、坐标、objectives、actors 均不改"], clear_triggers)
    add("F-grouped", "F 成组移植剧情锚点，保留玩家装备", canonical,
        ["源=原02槽：current_level + actors + objectives", "+ triggers + misc + player/transform；CP/Level配套",
         "level_id=01_gov_02；XYZ=(19.7084,1,-44.0500)", "保留原00槽武器、资源、升级"],
        lambda documents: pair(documents, lambda target, donor: progress.transplant(target, donor)[0]))
    add("G-full-slot", "G 完整槽02移到槽00", canonical,
        ["00_{Checkpoint,Level,Survival}Save = 原02槽整套", "level_id=01_gov_02；XYZ=(19.7084,1,-44.0500)",
         "剧情、玩家装备、难度=原02槽；共享settings不改"], full_slot)
    result = {"format": "deadspace.story-cases.v1", "cases": cases,
              "source_tablet_entry": str(tablet_entry.resolve()), "source_donor_var": str(donor_var.resolve())}
    (output / "cases.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tablet-entry", type=Path, required=True)
    parser.add_argument("--donor-var", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    result = prepare(args.tablet_entry, args.donor_var, args.out)
    print(json.dumps({"cases": len(result["cases"]), "output": str(args.out)}))

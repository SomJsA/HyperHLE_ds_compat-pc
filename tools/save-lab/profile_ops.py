"""Clone complete slot triplets or paired story anchors into a new profile."""
import json
from pathlib import Path
import tempfile

import cold_profiles
import progress
from sbin import Document, require


def slot_copy(target_entry, donor_entry, target_slot, donor_slot, bank, name, story_only=False):
    require(target_slot in (0, 1, 2) and donor_slot in (0, 1, 2), "slot must be 0, 1 or 2")
    target_entry, donor_entry = Path(target_entry), Path(donor_entry)
    target_listing = cold_profiles.verify(target_entry)
    cold_profiles.verify(donor_entry)
    suffixes = ("CheckpointSave", "LevelSave") if story_only else ("CheckpointSave", "LevelSave", "SurvivalSave")
    replacements = {}
    for suffix in suffixes:
        relative = f"var/{target_slot:02d}_{suffix}.sb"
        target = target_entry / "Documents" / relative
        donor = donor_entry / "Documents" / f"var/{donor_slot:02d}_{suffix}.sb"
        require(target.is_file() and donor.is_file(), f"missing paired slot file: {suffix}")
        left, right = Document(target.read_bytes()), Document(donor.read_bytes())
        replacements[relative] = progress.transplant(left, right)[0] if story_only else right.original
    with tempfile.TemporaryDirectory(prefix="save-lab-slot-") as temporary:
        documents = Path(temporary) / "Documents"
        cold_profiles.copy_inventory(target_entry / "Documents", documents, target_listing)
        for relative, data in replacements.items():
            (documents / relative).write_bytes(data)
        result = cold_profiles.snapshot(documents, bank, name)
    operation = {"mode": "story-anchors" if story_only else "whole-slot", "target_slot": target_slot,
                 "donor_slot": donor_slot, "files": sorted(replacements),
                 "settings_and_achievements": "retained from target profile"}
    cold_profiles.json_write(result / "slot-operation.json", operation)
    return result

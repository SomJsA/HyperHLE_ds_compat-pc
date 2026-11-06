"""Explicit story-anchor transplant; never substitutes level_id for scene state."""
from sbin import Document, digest, require

PROGRESS_ROOTS = ("current_level", "actors", "objectives", "triggers", "misc")


def transplant(target, donor):
    """Copy scene/quest/trigger state and player transform, retaining equipment.

The caller must supply matching Level + Checkpoint files together. This is a
structural experiment, not a guarantee that scripts can resume every checkpoint.
"""
    return apply_anchor(target, export_anchor(donor))


def export_anchor(donor):
    modules = {path: donor.export_module(path) for path in (*PROGRESS_ROOTS, "player/transform") if path in donor.paths}
    return {"format": "deadspace.story-anchor.v1", "source_sha256": digest(donor.original),
            "level_id": donor.locate("current_level/level_id").value,
            "modules": modules}


def apply_anchor(target, anchor):
    require(anchor.get("format") == "deadspace.story-anchor.v1", "unsupported story anchor")
    require(set(anchor["modules"]) <= {*PROGRESS_ROOTS, "player/transform"}, "unexpected story anchor path")
    original = target
    changed = []
    for path in (*PROGRESS_ROOTS, "player/transform"):
        left, right = path in target.paths, path in anchor["modules"]
        require(left == right, f"incompatible progress schema at {path}")
        if left:
            target = Document(target.import_module(path, anchor["modules"][path]))
            changed.append(path)
    # Prove that inventory, weapon and upgrade values were not moved with position.
    before = {p: (v.type, v.value) for p, v in original.paths.items()
              if p.startswith("player/") and not p.startswith("player/transform/") and v.type != 15}
    after = {p: (v.type, v.value) for p, v in target.paths.items()
             if p.startswith("player/") and not p.startswith("player/transform/") and v.type != 15}
    require(before == after, "progress transplant altered retained player equipment")
    return target.original, changed

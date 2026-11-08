"""Require every reviewed routing rule to be exercised by a real menu builder.

The UI library tests emit rule identities only after checking the complete live
tree for payload preservation, reachability, root budget and category order.
"""
import json
from pathlib import Path


def check(directory, root):
    fixture = Path(root) / "crates/xirius-ui/tests/context_menus/mapping.json"
    expected = {row["id"]: row for row in json.loads(fixture.read_text(encoding="utf-8"))}
    expected.update({row["id"]: row for row in json.loads(fixture.with_name("supplemental.json").read_text(encoding="utf-8"))})
    seen = set()
    trees = 0
    maximum = 0
    for file in Path(directory).glob("*.json"):
        tree = json.loads(file.read_text(encoding="utf-8"))
        seen.update(tree["ids"])
        trees += 1
        maximum = max(maximum, tree["root_rows"])
        if tree["root_rows"] > 12 or any(tree[key] for key in ("missing", "duplicate", "unreachable")):
            print(f"Context catalog: invalid live tree {file.name}")
            return False
    missing = sorted(set(expected) - seen)
    unknown = sorted(seen - set(expected))
    if missing or unknown:
        print(f"Context catalog: {len(seen & expected.keys())}/{len(expected)} live rules")
        for identity in missing:
            print(f"  missing {identity}: {' > '.join(expected[identity]['tobe'])}")
        for identity in unknown:
            print(f"  unknown {identity}")
        return False
    print(f"Context catalog: all {len(expected)} live rules covered in {trees} trees; "
          f"root_max={maximum}, missing=0, duplicate=0, unreachable=0, unknown=0")
    return True

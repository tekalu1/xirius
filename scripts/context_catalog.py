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
    for file in Path(directory).glob("*.json"):
        seen.update(json.loads(file.read_text(encoding="utf-8"))["ids"])
    missing = sorted(set(expected) - seen)
    unknown = sorted(seen - set(expected))
    if missing or unknown:
        print(f"Context catalog: {len(seen & expected.keys())}/{len(expected)} live rules")
        for identity in missing:
            print(f"  missing {identity}: {' > '.join(expected[identity]['tobe'])}")
        for identity in unknown:
            print(f"  unknown {identity}")
        return False
    print(f"Context catalog: all {len(expected)} live rules covered; missing=0, unknown=0")
    return True

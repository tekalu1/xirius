#!/usr/bin/env python3
"""Check .dawproject files against the DAWproject 1.0 schema (EXP-007).

The schema is the one of github.com/bitwig/dawproject (MIT), kept beside the reference
examples in crates/xirius-core/src/dawproject/testdata/reference. Each file's project.xml is
checked against Project.xsd and its metadata.xml (when it has one) against MetaData.xsd.

Usage:
  python scripts/dawproject_xsd.py FILE_OR_FOLDER...

A folder stands for the .dawproject files in it. The files Xirius writes in its tests are
kept in a folder with XIRIUS_DAWPROJECT_OUT set:
  XIRIUS_DAWPROJECT_OUT=out cargo test -p xirius-core --lib dawproject
  python scripts/dawproject_xsd.py out

Exit 1 when a file does not pass. Needs lxml (pip install lxml).
"""
from __future__ import annotations

import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCHEMA_DIR = ROOT / "crates" / "xirius-core" / "src" / "dawproject" / "testdata" / "reference"


def files_of(args: list[str]) -> list[Path]:
    out: list[Path] = []
    for a in args:
        p = Path(a)
        if p.is_dir():
            out.extend(sorted(p.glob("*.dawproject")))
        else:
            out.append(p)
    return out


def main(argv: list[str]) -> int:
    try:
        from lxml import etree
    except ImportError:
        print("lxml がありません（pip install lxml）", file=sys.stderr)
        return 2
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    schemas = {
        "project.xml": etree.XMLSchema(etree.parse(str(SCHEMA_DIR / "Project.xsd"))),
        "metadata.xml": etree.XMLSchema(etree.parse(str(SCHEMA_DIR / "MetaData.xsd"))),
    }
    files = files_of(argv)
    if not files:
        print("確かめる .dawproject がありません", file=sys.stderr)
        return 2
    failed = 0
    for f in files:
        errors: list[str] = []
        with zipfile.ZipFile(f) as z:
            names = set(z.namelist())
            if "project.xml" not in names:
                errors.append("project.xml がありません")
            for name, schema in schemas.items():
                if name not in names:
                    continue
                try:
                    doc = etree.fromstring(z.read(name))
                except etree.XMLSyntaxError as e:
                    errors.append(f"{name}: {e}")
                    continue
                if not schema.validate(doc):
                    errors.extend(f"{name}:{e.line}: {e.message}" for e in schema.error_log)
        if errors:
            failed += 1
            print(f"NG {f.name}")
            for e in errors:
                print(f"   {e}")
        else:
            print(f"OK {f.name}")
    print(f"{len(files) - failed}/{len(files)} が通りました")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

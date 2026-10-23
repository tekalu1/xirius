#!/usr/bin/env python3
"""Xirius plan: validate docs/plan.toml and show its progress.

docs/plan.toml is the only source of truth. Progress is the `status` of each item:
absent = not started, "partial" (with a one-line `remaining`), "done".
An item may name the items to build first in `after = ["UI-001", ...]`.

Subcommands:
  check                          validate docs/plan.toml (exit 1 on failure)
  status [--phase P] [--area A]  done / partial / not started per phase and per area
         [--open]                ... and list the partial and not-started items

Standard library only (Python 3.11+).
"""
from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PLAN_PATH = ROOT / "docs" / "plan.toml"
RESEARCH_DIR = ROOT / "temporary" / "research"

PHASES = ["spike", "mvp", "v1", "v1.1", "later"]
PHASE_TITLE = {"spike": "スパイク", "mvp": "MVP", "v1": "1.0", "v1.1": "1.1", "later": "後で"}
STATUSES = ["done", "partial"]

ID_RE = re.compile(r"^([A-Z]{2,4})-(\d{3})$")
DECIDED_RE = re.compile(r"^\d{4}-\d{2}-\d{2}: \S")


# ---------------------------------------------------------------- plan

def load_plan(path: Path = PLAN_PATH) -> dict:
    with open(path, "rb") as f:
        return tomllib.load(f)


def source_paths(source: str) -> list[str]:
    """'a.md § 3 ; b.html' -> ['a.md', 'b.html']"""
    out = []
    for part in source.split(";"):
        path = part.split(" § ")[0].strip()
        if path:
            out.append(path)
    return out


def source_exists(path: str, research_dir: Path = RESEARCH_DIR) -> bool:
    if not research_dir.is_dir():
        return False
    return any((d / path).is_file() for d in research_dir.iterdir() if d.is_dir())


def check(plan: dict, research_dir: Path = RESEARCH_DIR) -> list[str]:
    errors: list[str] = []
    seen: dict[str, str] = {}

    def need(entry: dict, kind: str, fields: list[str]) -> bool:
        ok = True
        for k in fields:
            v = entry.get(k)
            if not isinstance(v, str) or not v.strip():
                errors.append(f"{kind} {entry.get('id', '?')}: {k} is missing or empty")
                ok = False
            elif "\n" in v:
                errors.append(f"{kind} {entry.get('id', '?')}: {k} must be one line")
        return ok

    # Research is a local, ignored archive under temporary/research:
    # when it is absent, a source still has to name its origin file.
    have_research = research_dir.is_dir()

    def check_sources(eid: str, source: str) -> None:
        paths = source_paths(source)
        if not paths:
            errors.append(f"{eid}: source has no file path")
        for p in paths:
            if have_research and not source_exists(p, research_dir):
                errors.append(f"{eid}: source file not found under temporary/research: {p}")

    def check_status(it: dict) -> None:
        eid = it["id"]
        status = it.get("status")
        if "status" in it and status not in STATUSES:
            errors.append(f"item {eid}: status must be one of {STATUSES} or absent, got {status!r}")
            return
        if status == "partial":
            need(it, "item", ["remaining"])
        elif "remaining" in it:
            errors.append(f"item {eid}: remaining is only for status = \"partial\"")

    def register(eid: str, kind: str) -> None:
        if eid in seen:
            errors.append(f"duplicate id {eid} ({kind} and {seen[eid]})")
        seen[eid] = kind

    for it in plan.get("item", []):
        eid = it.get("id", "?")
        if not need(it, "item", ["id", "title", "area", "phase", "source", "done_when"]):
            continue
        m = ID_RE.match(eid)
        if not m:
            errors.append(f"item {eid}: id must look like UI-012")
        elif m.group(1) != it["area"]:
            errors.append(f"item {eid}: id prefix does not match area {it['area']}")
        if it["phase"] not in PHASES:
            errors.append(f"item {eid}: phase must be one of {PHASES}, got {it['phase']!r}")
        register(eid, "item")
        check_sources(eid, it["source"])
        check_status(it)
    for d in plan.get("dropped", []):
        if need(d, "dropped", ["id", "title", "reason", "source"]):
            register(d["id"], "dropped")
            check_sources(d["id"], d["source"])
    for o in plan.get("open", []):
        if need(o, "open", ["id", "title", "proposal", "source"]):
            register(o["id"], "open")
            check_sources(o["id"], o["source"])
            decided = o.get("decided")
            if decided is not None:
                if not isinstance(decided, str) or not decided.strip():
                    errors.append(f"open {o['id']}: decided is missing or empty")
                elif "\n" in decided:
                    errors.append(f"open {o['id']}: decided must be one line")
                elif not DECIDED_RE.match(decided):
                    errors.append(f"open {o['id']}: decided must start with the date, YYYY-MM-DD: ...")
    check_after(plan.get("item", []), errors)
    if not plan.get("item"):
        errors.append("plan has no [[item]]")
    return errors


def check_after(items: list[dict], errors: list[str]) -> None:
    """`after` (optional): the ids of the items to build first. They must be items, and must not loop."""
    ids = {it.get("id") for it in items}
    graph: dict[str, list[str]] = {}
    for it in items:
        if "after" not in it:
            continue
        eid = it.get("id", "?")
        after = it["after"]
        if not isinstance(after, list) or not after or not all(isinstance(a, str) for a in after):
            errors.append(f"item {eid}: after must be a non-empty list of item ids")
            continue
        for a in after:
            if a == eid:
                errors.append(f"item {eid}: after names the item itself")
            elif a not in ids:
                errors.append(f"item {eid}: after names {a}, which is not an [[item]]")
        graph[eid] = [a for a in after if a in ids and a != eid]
    # A loop means no order can satisfy every `after`.
    done: set[str] = set()
    for start in graph:
        path: list[str] = []
        on_path: set[str] = set()
        stack = [(start, iter(graph.get(start, [])))]
        path.append(start)
        on_path.add(start)
        while stack:
            node, rest = stack[-1]
            nxt = next(rest, None)
            if nxt is None:
                stack.pop()
                path.pop()
                on_path.discard(node)
                done.add(node)
            elif nxt in on_path:
                loop = path[path.index(nxt):] + [nxt]
                errors.append(f"item {start}: after loops: {' -> '.join(loop)}")
                return
            elif nxt not in done:
                stack.append((nxt, iter(graph.get(nxt, []))))
                path.append(nxt)
                on_path.add(nxt)


# -------------------------------------------------------------- status

def state(item: dict) -> str:
    """'done' / 'partial' / 'none' (not started)."""
    return item.get("status") if item.get("status") in STATUSES else "none"


def counts(items: list[dict]) -> dict[str, int]:
    c = {"done": 0, "partial": 0, "none": 0}
    for i in items:
        c[state(i)] += 1
    c["total"] = len(items)
    return c


def select(plan: dict, phase: str | None = None, area: str | None = None) -> list[dict]:
    return [
        i
        for i in plan.get("item", [])
        if (phase is None or i["phase"] == phase) and (area is None or i["area"] == area)
    ]


def _table(head: str, rows: list[tuple[str, dict[str, int]]]) -> list[str]:
    out = [f"| {head} | 済み | 一部 | 未着手 | 計 |", "|---|--:|--:|--:|--:|"]
    for name, c in rows:
        out.append(f"| {name} | {c['done']} | {c['partial']} | {c['none']} | {c['total']} |")
    return out


def status_report(plan: dict, phase: str | None = None, area: str | None = None, open_list: bool = False) -> str:
    """Markdown: the totals, a table per phase and per area, and (open_list) the items left to do."""
    items = select(plan, phase, area)
    c = counts(items)
    scope = " ".join(x for x in (PHASE_TITLE.get(phase or "", ""), area or "") if x) or "全体"
    out = [f"# 進み具合（{scope}）: 済み {c['done']}・一部 {c['partial']}・未着手 {c['none']} / {c['total']}", ""]
    if phase is None:
        out += _table("phase", [(PHASE_TITLE[p], counts([i for i in items if i["phase"] == p])) for p in PHASES])
        out.append("")
    if area is None:
        areas: dict[str, list[dict]] = {}
        for i in items:
            areas.setdefault(i["area"], []).append(i)
        out += _table("領域", [(a, counts(lst)) for a, lst in areas.items()] + [("計", c)])
        out.append("")
    if open_list:
        mark = {"partial": "一部", "none": "未着手"}
        by_id = {i["id"]: i for i in plan.get("item", [])}
        for p in PHASES:
            left = [i for i in items if i["phase"] == p and state(i) != "done"]
            if not left:
                continue
            out.append(f"## {PHASE_TITLE[p]} の残り（{len(left)}）")
            out.append("")
            for i in left:
                tail = f" — {i['remaining']}" if state(i) == "partial" else ""
                waiting = [a for a in i.get("after", []) if a in by_id and state(by_id[a]) != "done"]
                wait = f"（先に {'・'.join(waiting)}）" if waiting else ""
                out.append(f"- [{mark[state(i)]}] {i['id']} {i['title']}{wait}{tail}")
            out.append("")
    return "\n".join(out).rstrip("\n") + "\n"


# ------------------------------------------------------------ commands

def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--plan", default=str(PLAN_PATH))
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("check")
    p = sub.add_parser("status", help="done / partial / not started per phase and per area")
    p.add_argument("--phase", choices=PHASES)
    p.add_argument("--area", help="id prefix, e.g. UI")
    p.add_argument("--open", action="store_true", help="also list the partial and not-started items")
    args = ap.parse_args(argv)

    plan = load_plan(Path(args.plan))

    if args.cmd == "check":
        errors = check(plan)
        for e in errors:
            print("ERROR:", e, file=sys.stderr)
        if errors:
            print(f"{len(errors)} error(s)", file=sys.stderr)
            return 1
        c = counts(plan["item"])
        print(
            f"ok: {c['total']} items ({c['done']} done, {c['partial']} partial), "
            f"{len(plan.get('dropped', []))} dropped, {len(plan.get('open', []))} open"
        )
        return 0
    if args.cmd == "status":
        if args.area and not select(plan, area=args.area):
            print(f"no items in area {args.area}", file=sys.stderr)
            return 1
        print(status_report(plan, args.phase, args.area, args.open), end="")
        return 0
    return 2


if __name__ == "__main__":
    sys.exit(main())

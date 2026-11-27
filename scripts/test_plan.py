import io
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import plan as ps  # noqa: E402


def mini_plan():
    return {
        "item": [
            dict(id="UI-001", title="一つ目", area="UI", phase="mvp", source="reports/a.md § 1", done_when="できる"),
            dict(id="UI-002", title="二つ目", area="UI", phase="mvp", source="reports/a.md", done_when="できる"),
            dict(id="ENG-001", title="三つ目", area="ENG", phase="spike", source="reports/a.md", done_when="できる"),
        ],
        "dropped": [dict(id="DROP-01", title="捨てる", reason="理由", source="reports/a.md")],
        "open": [dict(id="OPEN-01", title="未決", proposal="こうする", source="reports/a.md")],
    }


def with_status():
    p = mini_plan()
    p["item"][0]["status"] = "done"
    p["item"][0]["tests"] = ["xirius-ui a::b"]
    p["item"][1]["status"] = "partial"
    p["item"][1]["remaining"] = "保存が無い"
    return p


class CheckTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        d = Path(self.tmp.name) / "2026-10-11" / "reports"
        d.mkdir(parents=True)
        (d / "a.md").write_text("x", encoding="utf-8")
        self.rd = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_ok(self):
        self.assertEqual(ps.check(mini_plan(), self.rd), [])

    def test_duplicate_id(self):
        p = mini_plan()
        p["item"][1]["id"] = "UI-001"
        self.assertTrue(any("duplicate" in e for e in ps.check(p, self.rd)))

    def test_bad_phase_and_prefix(self):
        p = mini_plan()
        p["item"][0]["phase"] = "now"
        p["item"][2]["id"] = "UI-009"
        errs = ps.check(p, self.rd)
        self.assertTrue(any("phase" in e for e in errs))
        self.assertTrue(any("prefix" in e for e in errs))

    def test_status_ok(self):
        self.assertEqual(ps.check(with_status(), self.rd), [])

    def test_unknown_status(self):
        p = mini_plan()
        p["item"][0]["status"] = "wip"
        self.assertTrue(any("status must be" in e for e in ps.check(p, self.rd)))

    def test_partial_needs_remaining(self):
        p = mini_plan()
        p["item"][0]["status"] = "partial"
        self.assertTrue(any("remaining is missing" in e for e in ps.check(p, self.rd)))
        p["item"][0]["remaining"] = "一行目\n二行目"
        self.assertTrue(any("remaining must be one line" in e for e in ps.check(p, self.rd)))

    def test_remaining_without_partial(self):
        p = mini_plan()
        p["item"][0]["remaining"] = "x"
        p["item"][1]["status"] = "done"
        p["item"][1]["tests"] = ["xirius-ui a::b"]
        p["item"][1]["remaining"] = "x"
        errs = [e for e in ps.check(p, self.rd) if "only for" in e]
        self.assertEqual(len(errs), 2)

    def test_missing_field_and_source(self):
        p = mini_plan()
        del p["item"][0]["done_when"]
        p["item"][1]["source"] = "reports/none.md"
        errs = ps.check(p, self.rd)
        self.assertTrue(any("done_when" in e for e in errs))
        self.assertTrue(any("not found" in e for e in errs))

    def test_sources_are_not_looked_up_without_the_research_folder(self):
        # The public repository does not include the local temporary/research archive.
        p = mini_plan()
        p["item"][1]["source"] = "reports/none.md"
        self.assertEqual(ps.check(p, self.rd / "missing"), [])
        p["item"][2]["source"] = " § 3"
        errs = ps.check(p, self.rd / "missing")
        self.assertEqual(errs, ["ENG-001: source has no file path"])

    def test_open_decided(self):
        p = mini_plan()
        p["open"][0]["decided"] = "2026-10-19: GPUI で作る（スパイクで確かめた）"
        self.assertEqual(ps.check(p, self.rd), [])
        p["open"][0]["decided"] = "GPUI で作る"
        errs = ps.check(p, self.rd)
        self.assertTrue(any("decided must start with the date" in e for e in errs), errs)
        p["open"][0]["decided"] = "2026-10-19: 一行目\n二行目"
        errs = ps.check(p, self.rd)
        self.assertTrue(any("decided must be one line" in e for e in errs), errs)
        p["open"][0]["decided"] = ""
        errs = ps.check(p, self.rd)
        self.assertTrue(any("decided is missing or empty" in e for e in errs), errs)

    def test_after(self):
        p = mini_plan()
        p["item"][1]["after"] = ["UI-001"]
        p["item"][2]["after"] = ["UI-001", "UI-002"]
        self.assertEqual(ps.check(p, self.rd), [])
        p["item"][0]["after"] = ["ENG-001"]
        errs = ps.check(p, self.rd)
        self.assertTrue(any("after loops" in e for e in errs), errs)

    def test_after_names_items_only(self):
        p = mini_plan()
        p["item"][0]["after"] = ["UI-009", "DROP-01", "UI-001"]
        p["item"][1]["after"] = []
        p["item"][2]["after"] = "UI-001"
        errs = ps.check(p, self.rd)
        self.assertIn("item UI-001: after names UI-009, which is not an [[item]]", errs)
        self.assertIn("item UI-001: after names DROP-01, which is not an [[item]]", errs)
        self.assertIn("item UI-001: after names the item itself", errs)
        self.assertIn("item UI-002: after must be a non-empty list of item ids", errs)
        self.assertIn("item ENG-001: after must be a non-empty list of item ids", errs)

    def test_local_archive_checks_each_source_of_dropped_and_open_entries(self):
        p = mini_plan()
        p["dropped"][0]["source"] = "reports/a.md § 1 ; reports/missing.md § 2"
        p["open"][0]["source"] = "reports/missing.md"
        self.assertEqual(ps.check(p, self.rd), [
            "DROP-01: source file not found under temporary/research: reports/missing.md",
            "OPEN-01: source file not found under temporary/research: reports/missing.md",
        ])
        self.assertEqual(ps.check(p, self.rd / "missing"), [])

    def test_done_needs_tests(self):
        p = mini_plan()
        p["item"][1]["status"] = "done"
        self.assertEqual(
            ps.check(p, self.rd), ['item UI-002: status = "done" needs tests, the tests that run its flow']
        )
        p["item"][1]["tests"] = ["xirius-ui user_flow_tests::reopen_child"]
        self.assertEqual(ps.check(p, self.rd), [])
        del p["item"][1]["tests"]
        p["done_before_tests"] = ["UI-002"]
        self.assertEqual(ps.check(p, self.rd), [])

    def test_done_before_tests_names_done_items_without_tests_once(self):
        p = with_status()
        p["done_before_tests"] = ["UI-001", "UI-002", "UI-009", "UI-002"]
        errs = ps.check(p, self.rd)
        self.assertIn("done_before_tests names UI-001, which is not a done item without tests: take it out", errs)
        self.assertIn("done_before_tests names UI-002, which is not a done item without tests: take it out", errs)
        self.assertIn("done_before_tests names UI-009, which is not an [[item]]", errs)
        self.assertIn("done_before_tests names UI-002 twice", errs)
        p["done_before_tests"] = "UI-001"
        self.assertIn("done_before_tests must be a list of item ids", ps.check(p, self.rd))

    def test_done_before_tests_only_shrinks_from_an_earlier_plan(self):
        earlier = {"done_before_tests": ["UI-001", "UI-002"]}
        self.assertEqual(ps.grown_done_before_tests({"done_before_tests": ["UI-002"]}, earlier), [])
        self.assertEqual(ps.grown_done_before_tests({}, earlier), [])
        self.assertEqual(
            ps.grown_done_before_tests({"done_before_tests": ["UI-002", "ENG-001"]}, earlier),
            ["done_before_tests gains ENG-001: an item marked done now names its tests instead"],
        )
        self.assertEqual(ps.grown_done_before_tests({"done_before_tests": ["ENG-001"]}, {}), [])

    def test_check_since_reads_the_plan_at_a_git_ref_and_a_pull_request_runs_it(self):
        with redirect_stdout(io.StringIO()):
            self.assertEqual(ps.main(["check", "--since", "HEAD"]), 0)
        err = io.StringIO()
        with mock.patch.object(ps, "plan_at", return_value={"done_before_tests": []}), redirect_stderr(err):
            self.assertEqual(ps.main(["check", "--since", "HEAD"]), 1)
        self.assertIn("done_before_tests gains", err.getvalue())
        workflow = (ps.ROOT / ".github" / "workflows" / "plan.yml").read_text(encoding="utf-8")
        self.assertIn('git fetch --no-tags --depth=1 origin "$BASE"', workflow)
        self.assertIn("python scripts/plan.py check --since FETCH_HEAD", workflow)

    def test_tests_are_nextest_names_or_script_tests_that_exist(self):
        p = mini_plan()
        p["item"][1]["tests"] = [
            "xirius-core::it core::every_history_command_undoes_and_redoes",
            "xirius-ui save_flow_tests::ctrl_shift_s_asks_where_to_save_the_song_as",
            "scripts/test_plan.py CheckTests.test_ok",
        ]
        self.assertEqual(ps.check(p, self.rd), [])
        p["item"][1]["tests"] = ["reopen_child", "scripts/test_plan.py CheckTests.test_none"]
        errs = ps.check(p, self.rd)
        self.assertIn(
            "item UI-002: tests names 'reopen_child': write `<binary> <test>` as `cargo nextest list` does, "
            "or `scripts/<file>.py <Class>.<method>`",
            errs,
        )
        self.assertIn("item UI-002: tests names scripts/test_plan.py CheckTests.test_none, which is not a test there", errs)
        for bad in ([], "xirius-ui a", ["xirius-ui a", 1]):
            p["item"][1]["tests"] = bad
            self.assertEqual(ps.check(p, self.rd), ["item UI-002: tests must be a non-empty list of test names"])

    def test_nextest_names(self):
        p = with_status()
        p["item"][1]["tests"] = ["scripts/test_plan.py CheckTests.test_ok", "xirius-ui b::c"]
        self.assertEqual(ps.nextest_names(p), [("UI-001", "xirius-ui a::b"), ("UI-002", "xirius-ui b::c")])


class StatusTests(unittest.TestCase):
    def test_counts(self):
        self.assertEqual(ps.counts(with_status()["item"]), {"done": 1, "partial": 1, "none": 1, "total": 3})

    def test_report_tables(self):
        text = ps.status_report(with_status())
        self.assertIn("# 進み具合（全体）: 済み 1・一部 1・未着手 1 / 3", text)
        self.assertIn("| MVP | 1 | 1 | 0 | 2 |", text)
        self.assertIn("| スパイク | 0 | 0 | 1 | 1 |", text)
        self.assertIn("| 1.0 | 0 | 0 | 0 | 0 |", text)
        self.assertIn("| UI | 1 | 1 | 0 | 2 |", text)
        self.assertIn("| ENG | 0 | 0 | 1 | 1 |", text)
        self.assertIn("| 計 | 1 | 1 | 1 | 3 |", text)
        self.assertNotIn("の残り", text)

    def test_report_open_list(self):
        text = ps.status_report(with_status(), open_list=True)
        self.assertIn("## MVP の残り（1）", text)
        self.assertIn("- [一部] UI-002 二つ目 — 保存が無い", text)
        self.assertIn("## スパイク の残り（1）", text)
        self.assertIn("- [未着手] ENG-001 三つ目\n", text)
        self.assertNotIn("UI-001", text)

    def test_report_open_list_names_unfinished_after(self):
        p = with_status()
        p["item"][2]["after"] = ["UI-001", "UI-002"]
        text = ps.status_report(p, open_list=True)
        self.assertIn("- [未着手] ENG-001 三つ目（先に UI-002）\n", text)

    def test_report_filters(self):
        text = ps.status_report(with_status(), phase="mvp", open_list=True)
        self.assertIn("# 進み具合（MVP）: 済み 1・一部 1・未着手 0 / 2", text)
        self.assertNotIn("| phase |", text)
        self.assertNotIn("ENG", text)
        text = ps.status_report(with_status(), area="ENG")
        self.assertIn("# 進み具合（ENG）: 済み 0・一部 0・未着手 1 / 1", text)
        self.assertNotIn("| 領域 |", text)

    def test_cli(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "plan.toml"
            path.write_text(
                '[[item]]\nid = "UI-001"\ntitle = "t"\narea = "UI"\nphase = "mvp"\nsource = "s"\ndone_when = "d"\n'
                'status = "partial"\nremaining = "r"\n',
                encoding="utf-8",
            )
            buf = io.StringIO()
            with redirect_stdout(buf):
                self.assertEqual(ps.main(["--plan", str(path), "status", "--open"]), 0)
            self.assertIn("- [一部] UI-001 t — r", buf.getvalue())


class RealPlanTests(unittest.TestCase):
    def test_real_plan_passes_check(self):
        plan = ps.load_plan()
        self.assertEqual(ps.check(plan), [])

    def test_real_plan_passes_without_the_local_archive(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(ps.check(ps.load_plan(), Path(tmp) / "missing"), [])


if __name__ == "__main__":
    unittest.main()

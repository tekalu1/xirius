"""Exercise the real foreground rules with SendInput on an unused Windows desktop.

Build with `python scripts/ci_tests.py --prepare` first. This is deliberately outside CI.
No input queues are attached, no button messages are synthesized, and no window is topmost.
"""
import argparse
from contextlib import contextmanager
import ctypes as c
from ctypes import wintypes as w
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import time


@contextmanager
def isolated_data(parent):
    path = tempfile.mkdtemp(prefix="preset-input-", dir=parent)
    try:
        yield path
    finally:
        # Windows can still have a crash dump open after the app has exited.
        shutil.rmtree(path, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path, required=True, help="temporary data parent")
    parser.add_argument("--out", type=Path, required=True, help="screenshots and result.json")
    parser.add_argument("--case", choices=["A", "E", "F", "G", "cold"], help="run one scenario")
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("this test requires Windows")
    processes = subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         "Get-CimInstance Win32_Process -Filter \"Name LIKE 'xirius%'\" | Select-Object ProcessId,ExecutablePath | ConvertTo-Json -Compress"],
        capture_output=True, text=True, check=True, timeout=20,
    ).stdout.strip()
    if processes:
        raise RuntimeError("Another Xirius process is running; leave its desktop alone: " + processes)
    args.work.mkdir(parents=True, exist_ok=True)
    args.out.mkdir(parents=True, exist_ok=True)
    out = args.out.resolve()
    u, g = c.windll.user32, c.windll.gdi32
    u.SetProcessDPIAware()
    signatures = {
        "GetForegroundWindow": (w.HWND, []),
        "WindowFromPoint": (w.HWND, [w.POINT]),
        "GetAncestor": (w.HWND, [w.HWND, w.UINT]),
        "GetClassNameW": (c.c_int, [w.HWND, w.LPWSTR, c.c_int]),
        "GetWindowTextW": (c.c_int, [w.HWND, w.LPWSTR, c.c_int]),
        "GetDlgItem": (w.HWND, [w.HWND, c.c_int]),
        "GetWindow": (w.HWND, [w.HWND, w.UINT]),
        "GetTopWindow": (w.HWND, [w.HWND]),
        "GetWindowRect": (w.BOOL, [w.HWND, c.POINTER(w.RECT)]),
        "MonitorFromWindow": (w.HANDLE, [w.HWND, w.DWORD]),
        "IsWindowVisible": (w.BOOL, [w.HWND]),
        "GetWindowThreadProcessId": (w.DWORD, [w.HWND, c.POINTER(w.DWORD)]),
        "FindWindowExW": (w.HWND, [w.HWND, w.HWND, w.LPCWSTR, w.LPCWSTR]),
        "SetWindowPos": (w.BOOL, [w.HWND, w.HWND, c.c_int, c.c_int, c.c_int, c.c_int, w.UINT]),
        "SendMessageTimeoutW": (c.c_ssize_t, [w.HWND, w.UINT, c.c_size_t, c.c_ssize_t,
                                               w.UINT, w.UINT, c.POINTER(c.c_size_t)]),
        "GetDC": (w.HDC, [w.HWND]),
        "ReleaseDC": (c.c_int, [w.HWND, w.HDC]),
    }
    for name, (result, params) in signatures.items():
        fn = getattr(u, name)
        fn.restype, fn.argtypes = result, params
    class Mouse(c.Structure):
        _fields_ = [("dx", w.LONG), ("dy", w.LONG), ("data", w.DWORD),
                    ("flags", w.DWORD), ("time", w.DWORD), ("extra", c.c_size_t)]
    class Keyboard(c.Structure):
        _fields_ = [("vk", w.WORD), ("scan", w.WORD), ("flags", w.DWORD),
                    ("time", w.DWORD), ("extra", c.c_size_t)]
    class Payload(c.Union):
        _fields_ = [("mouse", Mouse), ("key", Keyboard)]
    class Input(c.Structure):
        _fields_ = [("kind", w.DWORD), ("payload", Payload)]
    class MonitorInfo(c.Structure):
        _fields_ = [("size", w.DWORD), ("monitor", w.RECT), ("work", w.RECT), ("flags", w.DWORD)]
    u.SendInput.argtypes = [w.UINT, c.POINTER(Input), c.c_int]
    u.GetMonitorInfoW.argtypes = [w.HANDLE, c.POINTER(MonitorInfo)]

    def wait(test, label, seconds=15, detail=None):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            value = test()
            if value:
                return value
            time.sleep(0.04)
        raise AssertionError(f"{label}: {detail()}" if detail else label)

    def send(events, pause=0.12):
        batch = (Input * len(events))(*events)
        assert u.SendInput(len(events), batch, c.sizeof(Input)) == len(events), "SendInput failed"
        time.sleep(pause)

    def mouse(x, y, flags):
        left, top, width, height = [u.GetSystemMetrics(n) for n in (76, 77, 78, 79)]
        dx, dy = round((x-left)*65535/(width-1)), round((y-top)*65535/(height-1))
        return Input(0, Payload(mouse=Mouse(dx, dy, 0, flags, 0, 0)))

    def click(x, y):
        send([mouse(x, y, flags) for flags in (0xC001, 0x0002, 0x0004)])

    def hit_root(x, y):
        return u.GetAncestor(u.WindowFromPoint(w.POINT(x, y)), 2)

    def window_title(hwnd):
        title = c.create_unicode_buffer(256)
        u.GetWindowTextW(hwnd, title, len(title))
        return title.value

    def click_window(hwnd, x, y):
        assert hit_root(x, y) == hwnd, f"window {hwnd} is covered at {(x, y)}"
        click(x, y)

    def drag(hwnd, start, end):
        for x, y in (start, end):
            hit = hit_root(x, y)
            if hit != hwnd:
                name = c.create_unicode_buffer(64)
                u.GetClassNameW(hit, name, len(name))
                raise AssertionError(f"drag point {(x, y)} expected {hwnd}, found {hit} ({name.value})")
        send([mouse(*start, 0xC001), mouse(*start, 0x0002)])
        send([mouse(*end, 0xC001)])
        send([mouse(*end, 0x0004)])

    def rect(hwnd):
        r = w.RECT()
        assert u.GetWindowRect(hwnd, c.byref(r))
        return r

    def move_frame(hwnd, x, y):
        # Test layout only: SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE.
        assert u.SetWindowPos(hwnd, 0, x, y, 0, 0, 0x15)

    def move_right(hwnd):
        r = rect(hwnd)
        move_frame(hwnd, u.GetSystemMetrics(0) - (r.right - r.left) - 40, 180)

    def click_main(hwnd):
        r = rect(hwnd)
        x, y = r.right - 120, (r.top + r.bottom) // 2
        click_window(hwnd, x, y)

    def button(hwnd, control=2):
        child = u.GetDlgItem(hwnd, control)
        r = rect(child)
        x, y = (r.left+r.right)//2, (r.top+r.bottom)//2
        wait(lambda: u.WindowFromPoint(w.POINT(x, y)) == child,
             f"button {control} is covered at {(x, y)}", seconds=3,
             detail=lambda: window_title(hit_root(x, y)))
        click(x, y)

    def control_text(hwnd):
        # GetWindowTextW only retrieves captions across processes. Read the native
        # control's text with a deadline, without synthesizing any input messages.
        buffer = c.create_unicode_buffer(256)
        result = c.c_size_t()
        assert u.SendMessageTimeoutW(hwnd, 0x000D, len(buffer), c.addressof(buffer),
                                     0x0002, 500, c.byref(result)), "cannot read control text"
        return buffer.value

    def foreground_dialog():
        hwnd = u.GetForegroundWindow()
        name = c.create_unicode_buffer(48)
        u.GetClassNameW(hwnd, name, len(name))
        return hwnd if name.value == "#32770" else None

    def key(vk, modifier=None):
        keys = [vk] if modifier is None else [modifier, vk]
        send([Input(1, Payload(key=Keyboard(code, 0, flags, 0, 0)))
              for codes, flags in ((keys, 0), (reversed(keys), 2)) for code in codes])

    def type_text(value):
        for ch in value:
            send([Input(1, Payload(key=Keyboard(0, ord(ch), f, 0, 0))) for f in (4, 6)])

    def popup(frame):
        h = 0
        while True:
            h = u.FindWindowExW(0, h, "XiriusPresetList", None)
            if not h:
                return None
            if u.GetWindow(h, 4) == frame and u.IsWindowVisible(h):
                return h

    def order(handles):
        result, h = [], u.GetTopWindow(0)
        while h:
            if h in handles:
                result.append(h)
            h = u.GetWindow(h, 2)
        return result

    def new_frame(known):
        h = 0
        while True:
            h = u.FindWindowExW(0, h, "XiriusPluginEditor", None)
            if not h:
                return None
            if h not in known and u.IsWindowVisible(h):
                return h

    def loading_frame(pid, name):
        hwnd = u.GetTopWindow(0)
        while hwnd:
            if u.IsWindowVisible(hwnd) and window_title(hwnd).startswith(f"Xirius · {name} · "):
                process = w.DWORD()
                u.GetWindowThreadProcessId(hwnd, c.byref(process))
                if process.value == pid:
                    return hwnd
            hwnd = u.GetWindow(hwnd, 2)

    def fixture_label_control(frame):
        viewport = u.FindWindowExW(frame, 0, "STATIC", None)
        view = u.FindWindowExW(viewport, 0, "XiriusTestVst3View", None) if viewport else None
        return u.FindWindowExW(view, 0, "STATIC", None) if view else None

    def fixture_label(frame):
        label = fixture_label_control(frame)
        if label and u.IsWindowVisible(label):
            try:
                return control_text(label)
            except AssertionError:
                # The newly attached editor's GUI thread may still be busy.
                pass
        return ""

    def screenshot(name):
        width, height = u.GetSystemMetrics(0), u.GetSystemMetrics(1)
        for fn, result, params in [
            ("CreateCompatibleDC", w.HDC, [w.HDC]),
            ("CreateCompatibleBitmap", w.HBITMAP, [w.HDC, c.c_int, c.c_int]),
            ("SelectObject", w.HGDIOBJ, [w.HDC, w.HGDIOBJ]),
            ("DeleteObject", w.BOOL, [w.HGDIOBJ]), ("DeleteDC", w.BOOL, [w.HDC]),
            ("BitBlt", w.BOOL, [w.HDC,c.c_int,c.c_int,c.c_int,c.c_int,w.HDC,c.c_int,c.c_int,w.DWORD]),
            ("GetDIBits", c.c_int, [w.HDC,w.HBITMAP,w.UINT,w.UINT,c.c_void_p,c.c_void_p,w.UINT]),
        ]:
            getattr(g, fn).restype, getattr(g, fn).argtypes = result, params
        dc = u.GetDC(0)
        memory = g.CreateCompatibleDC(dc)
        bitmap = g.CreateCompatibleBitmap(dc, width, height)
        old = g.SelectObject(memory, bitmap)
        assert g.BitBlt(memory,0,0,width,height,dc,0,0,0x00CC0020)
        g.SelectObject(memory, old)
        header = struct.pack("<IiiHHIIiiII",40,width,-height,1,32,0,width*height*4,0,0,0,0)
        info = c.create_string_buffer(header)
        data = c.create_string_buffer(width*height*4)
        assert g.GetDIBits(memory,bitmap,0,height,data,info,0) == height
        (out / (name + ".bmp")).write_bytes(struct.pack("<2sIHHI",b"BM",54+len(data),0,0,54)+header+data.raw)
        g.DeleteObject(bitmap)
        g.DeleteDC(memory)
        u.ReleaseDC(0,dc)

    checks = []
    def passed(name):
        screenshot(name)
        checks.append({"step":name,"result":"passed","screen":name+".bmp"})

    root = Path(__file__).resolve().parents[1]
    debug = root / "target" / "debug"
    executable = debug / "xirius-ui.exe"
    fixture = next(path for path in (debug/"xirius_test_vst3.dll",
                                     debug/"deps"/"xirius_test_vst3.dll") if path.is_file())
    with isolated_data(args.work) as temp:
        env = os.environ.copy()
        env.update({"LOCALAPPDATA":temp, "APPDATA":temp, "TEMP":temp, "TMP":temp,
                    "XIRIUS_CONFIG_DIR":temp+"/config", "XIRIUS_PRESETS":temp+"/presets",
                    "XIRIUS_PLUGIN_CACHE":temp+"/plugins.json", "XIRIUS_KEEP_DIR":temp+"/keep",
                    "XIRIUS_CRASH_DIR":temp+"/crashes", "XIRIUS_PEAK_CACHE":temp+"/peaks",
                    "XIRIUS_VIEW_FILE":temp+"/view.json", "XIRIUS_DESKTOP_INPUT":"1"})
        env.pop("XIRIUS_EDITOR_OFFSCREEN", None)
        env.pop("XIRIUS_PRESET_INPUT_BUILTIN", None)
        if args.case == "G":
            env["XIRIUS_PRESET_INPUT_BUILTIN"] = "1"
        if args.case == "cold":
            env.setdefault("XIRIUS_TEST_VST3_EDITOR_OPEN_MS", "2000")
        # The fixture accepts both roles, but scans as an effect. Seed the isolated
        # cache before the app starts so the song owner's index lists it as an instrument.
        probe = Path(temp) / "fixture.json"
        subprocess.run([str(executable), "__scan-plugin", "--bundle", str(fixture),
                        "--out", str(probe)], env=env, check=True, timeout=20)
        entries = json.loads(probe.read_text(encoding="utf-8"))["entries"]
        entries[0]["kind"] = "instrument"
        Path(env["XIRIUS_PLUGIN_CACHE"]).write_text(json.dumps({"version": 2, "bundles": [
            {"path": str(fixture), "fingerprint": "", "entries": entries}]
        }), encoding="utf-8")
        for name in ("input-ready.json", "input-done", "input-state-request", "input-state.json"):
            (out/name).unlink(missing_ok=True)
        with (out/"app.log").open("w", encoding="utf-8") as log:
            script = "preset-input-cold" if args.case == "cold" else "preset-input"
            app = subprocess.Popen([str(executable), "--silent", "--at", "10,10",
                                    "--size", "1040,640", "--shot", str(out), "--script", script],
                                   cwd=temp, env=env, stdout=log, stderr=log)
            def run_checks():
                wait(lambda: (out/"input-ready.json").exists() or app.poll() is not None, "fixture setup", 90)
                assert (out/"input-ready.json").exists(), "fixture app exited during setup; read app.log"
                info = json.loads((out/"input-ready.json").read_text(encoding="utf-8"))
                main_hwnd = info["main"]
                if args.case == "cold":
                    hosts = subprocess.run(
                        ["powershell", "-NoProfile", "-Command",
                         "Get-CimInstance Win32_Process -Filter \"Name = 'xirius-plugin-host.exe'\" | Select-Object -ExpandProperty ProcessId"],
                        capture_output=True, text=True, check=True, timeout=20,
                    ).stdout.strip()
                    assert not hosts, f"plugin host was already running before the drag: {hosts}"
                    assert not new_frame(set()), "a native editor opened before the drag"
                    click_main(main_hwnd)
                    wait(lambda: u.GetForegroundWindow() == main_hwnd, "main window did not activate")
                    started = time.monotonic()
                    drag(main_hwnd, info["drag_from"], info["drag_to"])
                    editor = wait(lambda: new_frame(set()),
                                  "cold drop did not open a native editor", seconds=45)
                    opened_blank = not fixture_label_control(editor)
                    if opened_blank:
                        screenshot("cold-blank-frame")
                    process = w.DWORD()
                    u.GetWindowThreadProcessId(editor, c.byref(process))
                    assert process.value and process.value != app.pid
                    label = wait(lambda: (label if (label := fixture_label(editor)).startswith("delay ")
                                          and f" pid {process.value} " in label else None),
                                 "cold editor had no fixture content")
                    wait(lambda: u.GetForegroundWindow() == editor
                         and order([editor, main_hwnd]) == [editor, main_hwnd],
                         "cold editor with content did not come to the foreground",
                         detail=lambda: (u.GetForegroundWindow(), order([editor, main_hwnd])))
                    wait(lambda: not loading_frame(app.pid, entries[0]["name"]),
                         "cold editor's loading window stayed visible")
                    elapsed = round(time.monotonic() - started, 2)
                    assert not opened_blank, ("native editor became visible before its view attached; "
                                              f"content and foreground followed after {elapsed}s")
                    passed("cold-first-drop")
                    checks[-1].update(seconds=elapsed, label=label)
                    return
                first, second = info["frames"]
                move_frame(first, 640, 180)
                move_right(second)
                wait(lambda: order([main_hwnd, first, second]) == [second, first, main_hwnd],
                     "new editor opened behind the pinned editor or main window",
                     detail=lambda: (order([main_hwnd, first, second]), info, u.GetForegroundWindow()))
                screenshot("00-desktop")
                if args.case == "G":
                    request = out / "input-state-request"
                    reply = out / "input-state.json"

                    def state():
                        reply.unlink(missing_ok=True)
                        request.touch()
                        wait(lambda: not request.exists(), "fixture did not report the track order")
                        return json.loads(reply.read_text(encoding="utf-8"))

                    # Keep both external frames outside the browser-to-column path.
                    move_right(first)
                    if u.GetForegroundWindow() != main_hwnd:
                        click_main(main_hwnd)
                        wait(lambda: u.GetForegroundWindow() == main_hwnd,
                             "main window did not come to the foreground")
                    wait(lambda: all(hit_root(*point) == main_hwnd
                                     for point in (info["drag_from"], info["seam"])),
                         "insertion path is covered", seconds=3,
                         detail=lambda: [window_title(hit_root(*point))
                                         for point in (info["drag_from"], info["seam"])])
                    drag(main_hwnd, info["drag_from"], info["seam"])
                    original = info["root"]
                    index = info["before_index"]

                    def new_order():
                        current = state()
                        return current if current["root"] != original else None

                    inserted = wait(new_order, "instrument drop did not create a track")
                    actual = inserted["root"]
                    assert len(actual) == len(original) + 1, actual
                    assert actual[:index] + actual[index + 1:] == original, actual
                    title = inserted["editor"]
                    assert title, "the inserted track was not selected"
                    editor = wait(lambda: u.FindWindowExW(0, 0, None, title),
                                  "inserted track's built-in editor did not open")
                    wait(lambda: u.GetForegroundWindow() == editor
                         and order([editor, first, second, main_hwnd])[0] == editor,
                         "inserted track's editor did not come to the foreground")
                    passed("17-instrument-insertion")
                    click_main(main_hwnd)
                    key(ord("Z"), 0x11)
                    wait(lambda: state()["root"] == original, "undo did not restore the track order")
                    passed("18-instrument-insertion-undo")
                    return
                if args.case == "A":
                    # Leave one unpinned editor and open the channel from its routing button.
                    button(second, 5)
                    wait(lambda: not u.IsWindowVisible(second), "second editor did not close")
                    button(first, 4)
                    button(first, 6)

                    def shown_channel():
                        hwnd = u.FindWindowExW(0, 0, None, "Xirius · Channel Editor")
                        return hwnd if hwnd and u.IsWindowVisible(hwnd) else None

                    channel = wait(shown_channel, "channel editor did not open")
                    r = rect(first)
                    monitor = MonitorInfo()
                    monitor.size = c.sizeof(monitor)
                    assert u.GetMonitorInfoW(u.MonitorFromWindow(first, 2), c.byref(monitor))
                    work = monitor.work
                    bounds = rect(channel)
                    assert (work.left <= bounds.left < bounds.right <= work.right
                            and work.top <= bounds.top < bounds.bottom <= work.bottom), (
                                "channel did not fit within the working area",
                                (bounds.left, bounds.top, bounds.right, bounds.bottom),
                                (work.left, work.top, work.right, work.bottom))
                    assert (bounds.right <= r.left or bounds.left >= r.right
                            or bounds.bottom <= r.top or bounds.top >= r.bottom), (
                                "channel did not open beside the external editor",
                                (bounds.left, bounds.top, bounds.right, bounds.bottom),
                                (r.left, r.top, r.right, r.bottom))
                    exposed = next(((x, y) for x in range(bounds.left + 12, bounds.right - 12, 8)
                                    for y in (bounds.top + 80, bounds.top + 120, bounds.top + 200)
                                    if hit_root(x, y) == channel), None)
                    assert exposed, "the channel edge did not become visible"
                    windows = {first:"external", main_hwnd:"main", channel:"channel"}
                    for hold in (0.12, 0, 0.08, 0):
                        r = rect(first)
                        click_window(first, r.left + 40, r.top + 110)
                        wait(lambda: u.GetForegroundWindow() == first, "external editor is not foreground")
                        x, y = exposed
                        assert hit_root(x, y) == channel, "channel click point is covered"
                        send([mouse(x, y, 0xC001), mouse(x, y, 0x0002)], pause=hold)
                        send([mouse(x, y, 0x0004)], pause=0)
                        wait(lambda: u.GetForegroundWindow() == channel and order(windows) == [channel, first, main_hwnd],
                             "channel click did not raise it above the external editor", seconds=3,
                             detail=lambda: ([windows[h] for h in order(windows)],
                                             windows.get(u.GetForegroundWindow())))
                        assert hit_root(x, y) == channel, "clicked channel window is covered"
                    passed("16-channel-unpinned")
                    return
                def open_list(frame):
                    button(frame)
                    hwnd = wait(lambda: popup(frame), f"list did not open for editor {frame} after {len(checks)} checks")
                    wait(lambda: u.GetForegroundWindow() == hwnd, "list did not become foreground")
                    process, owner_process = w.DWORD(), w.DWORD()
                    u.GetWindowThreadProcessId(hwnd, c.byref(process))
                    u.GetWindowThreadProcessId(frame, c.byref(owner_process))
                    assert process.value == owner_process.value != app.pid
                    assert order([main_hwnd, frame]) == [frame, main_hwnd]
                    return hwnd
                p = open_list(first)
                passed("01-open-foreground")
                type_text("bRi")
                edit = u.GetDlgItem(p, 1)
                assert control_text(edit) == "bRi"
                key(27)
                wait(lambda: not control_text(edit), "Esc did not clear search")
                assert popup(first)
                key(27)
                wait(lambda: not popup(first), "Esc did not close")
                passed("02-two-stage-escape")
                open_list(first)
                button(first)
                wait(lambda: not popup(first), "same button did not close")
                passed("03-same-button")
                open_list(first)
                r = rect(first)
                click(r.left+100, r.top+110)
                wait(lambda: not popup(first), "editor body did not close list")
                passed("04-editor-body")
                open_list(first)
                click_main(main_hwnd)
                wait(lambda: not popup(first), "outside click did not close list")
                wait(lambda: order([main_hwnd, first, second])[-1] == main_hwnd, "editors behind main")
                passed("05-outside-and-raise")
                open_list(first)
                open_list(second)
                wait(lambda: not popup(first), "first list blocked second editor")
                key(27)
                passed("06-two-editors")
                click_window(main_hwnd, *info["builtin"])
                key(0x7A, 0x10)  # Shift+F11 opens the selected track's instrument.
                builtin = wait(lambda: u.FindWindowExW(0, 0, None, info["builtin_title"]), "built-in editor did not open")
                wait(lambda: u.GetForegroundWindow() == builtin and order([builtin, first, second])[0] == builtin,
                     "built-in editor did not open in front of the pinned editors")
                assert u.IsWindowVisible(first) and u.IsWindowVisible(second)
                passed("07-builtin-pinned")
                move_frame(builtin, 35, u.GetSystemMetrics(1) - 185)
                button(first, 6)
                channel = wait(lambda: u.FindWindowExW(0, 0, None, "Xirius · Channel Editor"), "channel editor did not open")
                assert u.IsWindowVisible(first) and u.IsWindowVisible(second)
                passed("08-channel-pinned")
                # Leave the studio windows open and place their bars clear of the host controls.
                for palette, y in [(builtin, u.GetSystemMetrics(1) - 185), (channel, 320)]:
                    move_frame(palette, 35, y)
                open_list(second)
                key(27)
                click_main(main_hwnd)
                windows = {second:"second", first:"first", main_hwnd:"main", builtin:"built-in", channel:"channel"}
                wait(lambda: order(windows)[:2] == [second, first],
                     "last touched editor order above the studio windows",
                     detail=lambda: [windows[h] for h in order(windows)])
                passed("09-touch-order")
                if args.case is None:
                    # The left edge of each palette is exposed, even after the editors
                    # return above the studio group. Re-activate it from an external editor.
                    for palette in (channel, builtin, channel, builtin):
                        r = rect(palette)
                        x, y = r.left + 12, r.top + 80
                        for hold in (0.12, 0):
                            r = rect(second)
                            click_window(second, r.left + 40, r.top + 110)
                            wait(lambda: u.GetForegroundWindow() == second, "external editor is not foreground")
                            assert hit_root(x, y) == palette, "palette click point is covered"
                            if hold:
                                send([mouse(x, y, 0xC001), mouse(x, y, 0x0002)], pause=hold)
                                send([mouse(x, y, 0x0004)], pause=0)
                            else:
                                send([mouse(x, y, flag) for flag in (0xC001, 0x0002, 0x0004)], pause=0)
                            wait(lambda: u.GetForegroundWindow() == palette
                                 and order([palette, second, first, main_hwnd]) == [palette, second, first, main_hwnd],
                                 "palette click did not raise it while preserving external editor order", seconds=3,
                                 detail=lambda: ([windows[h] for h in order(windows)],
                                                 windows.get(u.GetForegroundWindow())))
                            assert hit_root(x, y) == palette, "clicked palette is covered"
                    passed("13-palette-reactivation")
                move_frame(channel, 35, info["external_track"][1] + 64)
                if args.case in (None, "F"):
                    click_main(main_hwnd)
                    drag(main_hwnd, info["drag_from"], info["drag_to"])
                    dropped = wait(lambda: new_frame([first, second]),
                                   "new track's instrument did not open on drop")
                    wait(lambda: order([main_hwnd, first, second, dropped])
                         == [dropped, second, first, main_hwnd]
                         and u.GetForegroundWindow() == dropped, "new track's editor is buried")
                    passed("14-browser-drop")
                    button(dropped, 5)
                    wait(lambda: not u.IsWindowVisible(dropped), "new track's editor did not close")
                if args.case in (None, "E"):
                    button(second, 5)
                    wait(lambda: not u.IsWindowVisible(second), "second editor did not close")
                    click_window(main_hwnd, *info["external_track"])
                    wait(lambda: u.GetForegroundWindow() == main_hwnd, "main window is not foreground")
                    key(0x7A, 0x10)
                    reopened = wait(lambda: new_frame([first, second]),
                                    "Shift+F11 did not reopen the external editor")
                    wait(lambda: order([first, reopened, main_hwnd]) == [reopened, first, main_hwnd]
                         and u.GetForegroundWindow() == reopened,
                         "Shift+F11 editor did not come to the foreground",
                         detail=lambda: (order([first, reopened, main_hwnd]),
                                         u.GetForegroundWindow(), reopened))
                    passed("15-shortcut-foreground")
                    move_right(reopened)
                if args.case is None:
                    p = open_list(first)
                    type_text("clean")
                    key(13)
                    wait(lambda: not popup(first), "Enter did not choose Clean")
                    wait(lambda: control_text(u.GetDlgItem(first, 2)) == "Clean", "Clean did not load")
                    p = open_list(first)
                    type_text("bRi")
                    key(13)
                    wait(lambda: not popup(first), "Enter did not choose")
                    wait(lambda: "Bright" in control_text(u.GetDlgItem(first, 2)),
                         "selected preset did not reach the song")
                    passed("10-search-enter")
                    p = open_list(first)
                    type_text("no matching preset")
                    key(40)  # overwrite -> save as; footer survives filtering
                    key(13)
                    save = wait(lambda: u.FindWindowExW(0, 0, None, "Save preset"), "save form did not open")
                    wait(lambda: u.GetForegroundWindow() == save, "save form not foreground")
                    assert u.GetWindow(save, 4) == first, "save form owner is not the editor"
                    passed("11-save-form-owner")
                    key(13)
                    wait(lambda: not u.IsWindowVisible(save), "save form did not close")
                    preset_dir = Path(temp) / "presets"
                    wait(lambda: any(preset_dir.rglob("Bright 2.xpreset")),
                         "save form did not write the new preset")
                    wait(lambda: control_text(u.GetDlgItem(first, 2)) == "Bright 2",
                         "saved preset did not reach the editor")
                    p = open_list(first)
                    type_text("no matching preset")
                    key(40)
                    key(40)  # save as -> file open; footer survives filtering
                    key(13)
                    dialog = wait(foreground_dialog, "file dialog did not become foreground")
                    assert u.GetWindow(dialog, 4) == first, "file dialog owner is not the editor"
                    passed("12-file-dialog-owner")
                    key(27)
                    wait(lambda: not u.IsWindowVisible(dialog), "file dialog did not close")
            try:
                run_checks()
            except BaseException as error:
                screenshot("failure")
                checks.append({"result":"failed", "error":str(error)})
                raise
            finally:
                (out/"result.json").write_text(json.dumps(checks, indent=2), encoding="utf-8")
                (out/"input-done").write_text("done", encoding="utf-8")
                try:
                    app.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    app.terminate()
                    app.wait(timeout=10)
        assert app.returncode == 0, "fixture app failed; read app.log"
    print(f"{len(checks)} input checks passed; {out / 'result.json'}")


if __name__ == "__main__":
    main()

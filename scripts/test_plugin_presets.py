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
        "GetDlgItem": (w.HWND, [w.HWND, c.c_int]),
        "GetWindow": (w.HWND, [w.HWND, w.UINT]),
        "GetTopWindow": (w.HWND, [w.HWND]),
        "GetWindowRect": (w.BOOL, [w.HWND, c.POINTER(w.RECT)]),
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
    u.SendInput.argtypes = [w.UINT, c.POINTER(Input), c.c_int]

    def wait(test, label, seconds=15, detail=None):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            value = test()
            if value:
                return value
            time.sleep(0.04)
        raise AssertionError(f"{label}: {detail()}" if detail else label)

    def send(events):
        batch = (Input * len(events))(*events)
        assert u.SendInput(len(events), batch, c.sizeof(Input)) == len(events), "SendInput failed"
        time.sleep(0.12)

    def click(x, y):
        left, top, width, height = [u.GetSystemMetrics(n) for n in (76, 77, 78, 79)]
        dx, dy = round((x-left)*65535/(width-1)), round((y-top)*65535/(height-1))
        send([Input(0, Payload(mouse=Mouse(dx, dy, 0, flags, 0, 0)))
              for flags in (0xC001, 0x0002, 0x0004)])

    def rect(hwnd):
        r = w.RECT()
        assert u.GetWindowRect(hwnd, c.byref(r))
        return r

    def click_main(hwnd):
        r = rect(hwnd)
        x, y = r.right - 120, (r.top + r.bottom) // 2
        assert u.GetAncestor(u.WindowFromPoint(w.POINT(x, y)), 2) == hwnd, "fixture main window is covered"
        click(x, y)

    def button(hwnd, control=2):
        child = u.GetDlgItem(hwnd, control)
        r = rect(child)
        x, y = (r.left+r.right)//2, (r.top+r.bottom)//2
        hit = u.WindowFromPoint(w.POINT(x, y))
        assert hit == child, f"button {control} is covered: expected {child}, found {hit} at {(x, y)}"
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
    with isolated_data(args.work) as temp:
        env = os.environ.copy()
        env.update({"LOCALAPPDATA":temp, "APPDATA":temp, "XIRIUS_CONFIG_DIR":temp+"/config",
                    "XIRIUS_PLUGIN_CACHE":temp+"/plugins.json", "XIRIUS_KEEP_DIR":temp+"/keep",
                    "XIRIUS_CRASH_DIR":temp+"/crashes", "XIRIUS_PEAK_CACHE":temp+"/peaks",
                    "XIRIUS_VIEW_FILE":temp+"/view.json", "XIRIUS_DESKTOP_INPUT":"1"})
        env.pop("XIRIUS_EDITOR_OFFSCREEN", None)
        for name in ("input-ready.json", "input-done"):
            (out/name).unlink(missing_ok=True)
        with (out/"app.log").open("w", encoding="utf-8") as log:
            app = subprocess.Popen([str(root/"target/debug/xirius-ui.exe"), "--silent", "--at", "10,10",
                                    "--size", "1200,850", "--shot", str(out), "--script", "preset-input"],
                                   cwd=temp, env=env, stdout=log, stderr=log)
            try:
                wait(lambda: (out/"input-ready.json").exists() or app.poll() is not None, "fixture setup", 90)
                assert (out/"input-ready.json").exists(), "fixture app exited during setup; read app.log"
                info = json.loads((out/"input-ready.json").read_text(encoding="utf-8"))
                main_hwnd, (first, second) = info["main"], info["frames"]
                wait(lambda: order([main_hwnd, first, second]) == [second, first, main_hwnd],
                     "new editor opened behind the pinned editor or main window",
                     detail=lambda: (order([main_hwnd, first, second]), info, u.GetForegroundWindow()))
                for frame, x in [(first, 420), (second, u.GetSystemMetrics(0)- (rect(second).right-rect(second).left)-40)]:
                    u.SetWindowPos(frame, 0, x, 180, 0, 0, 0x15)  # layout only: no activation/z-order
                time.sleep(0.5)
                screenshot("00-desktop")
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
                click(*info["builtin"])
                key(0x7A, 0x10)  # Shift+F11 opens the selected track's instrument.
                builtin = wait(lambda: u.FindWindowExW(0, 0, None, info["builtin_title"]), "built-in editor did not open")
                wait(lambda: order([builtin, first, second])[-1] == builtin, "pinned editor behind built-in editor")
                assert u.IsWindowVisible(first) and u.IsWindowVisible(second)
                passed("07-builtin-pinned")
                button(first, 6)
                channel = wait(lambda: u.FindWindowExW(0, 0, None, "Xirius · Channel Editor"), "channel editor did not open")
                wait(lambda: order([channel, first, second])[-1] == channel, "pinned editor behind channel editor")
                assert u.IsWindowVisible(first) and u.IsWindowVisible(second)
                passed("08-channel-pinned")
                # Leave the studio windows open and place their bars clear of the host controls.
                for palette, x in [(builtin, 35), (channel, 60)]:
                    assert u.SetWindowPos(palette, 0, x, 650, 0, 0, 0x15)
                open_list(second)
                key(27)
                click_main(main_hwnd)
                windows = {second:"second", first:"first", main_hwnd:"main", builtin:"built-in", channel:"channel"}
                wait(lambda: order(windows)[:2] == [second, first],
                     "last touched editor order above the studio windows",
                     detail=lambda: [windows[h] for h in order(windows)])
                passed("09-touch-order")
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
                key(27)
                wait(lambda: not u.IsWindowVisible(save), "save form did not close")
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

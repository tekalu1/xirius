"""What a test run leaves behind: files in the person's Xirius folders, and files in the temporary
folder.

`ci_tests.py` runs the tests with the temporary folder (`TMP`, `TEMP`, `TMPDIR`) pointed at a
folder of the run's own, and lists the Xirius folders of the person running them before and
after. A test writes neither: it keeps its files in a folder it removes, and gives the programs
it starts their folders (`XIRIUS_CRASH_DIR`, `XIRIUS_CONFIG_DIR`, ...). After the run, anything
new, changed or gone in the Xirius folders, and anything left in the run's temporary folder,
fails the run by name.

The programs keep the jobs running in any of their processes in one folder under the temporary
folder (`xirius_core::running::folder`, `Xirius/jobs`), and leave the folder for the next. The
watch makes it before the run, so it counts only for what is left in it.

An Xirius that is not this checkout's (the installed app, or a test run in another checkout)
may write to those folders while the tests run; when one is seen, what changed there is printed
as a warning instead of failing the run. The temporary folder is the run's own and always
counts.

Standard library only.
"""
import ctypes
import os
import shutil
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

# How many names a failure prints of each kind.
SHOWN = 40

# The folders under the run's temporary folder the programs share between their processes and
# leave (`xirius_core::running::folder`); made before the run, they count only for what is in them.
SHARED = (Path("Xirius") / "jobs",)


def user_places(environ=None, platform=None):
    """The folders the programs keep the person's things in when no variable names another: the
    settings (`xirius_prefs::config_dir`), the cache with the plugin list, the kept songs and the
    presets (`xirius_plugins::default_cache_path`), the peaks and the crash records."""
    environ = os.environ if environ is None else environ
    platform = sys.platform if platform is None else platform
    places = []
    if platform == "win32":
        for name in ("LOCALAPPDATA", "APPDATA"):
            if environ.get(name):
                places.append(Path(environ[name]) / "Xirius")
    elif environ.get("HOME"):
        home = Path(environ["HOME"])
        if platform == "darwin":
            places += [home / "Library/Application Support/Xirius", home / "Library/Caches/Xirius",
                       home / "Library/Logs/Xirius"]
        else:
            config = Path(environ.get("XDG_CONFIG_HOME") or home / ".config")
            cache = Path(environ.get("XDG_CACHE_HOME") or home / ".cache")
            places += [config / "xirius", cache / "xirius"]
    return places


def listing(roots):
    """Every file and folder under `roots`: the path, then the size (None for a folder) and the
    time it was last written. Names only; nothing is opened."""
    found = {}
    for root in roots:
        for top, folders, files in os.walk(root):
            for name in folders + files:
                path = os.path.join(top, name)
                try:
                    status = os.stat(path)
                except OSError:
                    continue
                folder = name in folders
                found[path] = (None if folder else status.st_size, None if folder else status.st_mtime_ns)
    return found


def differences(before, after):
    """`+` a new path, `~` a file written again, `-` a path gone; a folder whose files changed is
    not itself a change."""
    lines = [f"+ {path}" for path in sorted(after.keys() - before.keys())]
    lines += [f"~ {path}" for path in sorted(before.keys() & after.keys()) if before[path] != after[path]]
    lines += [f"- {path}" for path in sorted(before.keys() - after.keys())]
    return lines


def left_in(folder, shared=()):
    """The names directly in `folder`, a folder's with how many paths are under it; a `shared`
    folder (a path relative to `folder`) and the folders on the way to it count only for what
    else is in them."""
    folder = Path(folder)
    on_the_way = {part for path in shared for part in [Path(path), *Path(path).parents] if part != Path(".")}

    def walk(here, relative):
        found = []
        for entry in sorted(here.iterdir()):
            inner = relative / entry.name
            if entry.is_dir() and inner in on_the_way:
                found += walk(entry, inner)
            elif entry.is_dir():
                found.append(f"{inner}{os.sep} ({sum(1 for _ in entry.rglob('*'))} inside)")
            else:
                found.append(str(inner))
        return found

    return walk(folder, Path("."))


def _windows_processes():
    """(name, full path or None) of every process, by the Tool Help snapshot."""
    from ctypes import wintypes

    class Entry(ctypes.Structure):
        _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD),
                    ("th32ProcessID", wintypes.DWORD), ("th32DefaultHeapID", ctypes.c_size_t),
                    ("th32ModuleID", wintypes.DWORD), ("cntThreads", wintypes.DWORD),
                    ("th32ParentProcessID", wintypes.DWORD), ("pcPriClassBase", ctypes.c_long),
                    ("dwFlags", wintypes.DWORD), ("szExeFile", wintypes.WCHAR * 260)]

    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel.OpenProcess.restype = wintypes.HANDLE
    snapshot = kernel.CreateToolhelp32Snapshot(0x2, 0)  # TH32CS_SNAPPROCESS
    if snapshot in (None, wintypes.HANDLE(-1).value):
        return []
    found = []
    try:
        entry = Entry()
        entry.dwSize = ctypes.sizeof(Entry)
        more = kernel.Process32FirstW(snapshot, ctypes.byref(entry))
        while more:
            path = None
            process = kernel.OpenProcess(0x1000, False, entry.th32ProcessID)  # QUERY_LIMITED_INFORMATION
            if process:
                buffer = ctypes.create_unicode_buffer(32768)
                size = wintypes.DWORD(len(buffer))
                if kernel.QueryFullProcessImageNameW(process, 0, buffer, ctypes.byref(size)):
                    path = buffer.value
                kernel.CloseHandle(process)
            found.append((entry.szExeFile, path))
            more = kernel.Process32NextW(snapshot, ctypes.byref(entry))
    finally:
        kernel.CloseHandle(snapshot)
    return found


def _posix_processes():
    try:
        out = subprocess.run(["ps", "-A", "-o", "comm="], capture_output=True, text=True, timeout=30).stdout
    except (OSError, subprocess.SubprocessError):
        return []
    found = []
    for line in out.splitlines():
        line = line.strip()
        if line:
            found.append((os.path.basename(line), line if os.path.isabs(line) else None))
    return found


def foreign_xirius(checkout, temporary, processes=None):
    """The Xirius programs running that are not this checkout's: a process whose name starts
    with `xirius` and whose file is neither under `checkout` nor under the run's `temporary`
    folder (a test's copy). One whose file cannot be read is left out: it is a test's program
    that ended between the list and the look (an Xirius the person started is theirs and can be
    read)."""
    if processes is None:
        processes = _windows_processes() if sys.platform == "win32" else _posix_processes()
    own = [os.path.normcase(str(Path(folder).resolve())) + os.sep for folder in (checkout, temporary)]
    found = set()
    for name, path in processes:
        if not name.lower().startswith("xirius") or not path:
            continue
        if any(os.path.normcase(str(Path(path).resolve())).startswith(o) for o in own):
            continue
        found.add(path)
    return sorted(found)


class Watch:
    """Around a test run: the run's temporary folder, the listings, and the Xirius programs not
    this checkout's seen every few seconds. `environment` is what the tests run with."""

    def __init__(self, checkout, environment, places=None, every=2.0):
        self.checkout = Path(checkout)
        self.places = user_places() if places is None else places
        self.temporary = Path(tempfile.mkdtemp(prefix="xirius-test-run-"))
        for shared in SHARED:
            (self.temporary / shared).mkdir(parents=True)
        self.environment = dict(environment)
        for name in ("TMP", "TEMP", "TMPDIR"):
            self.environment[name] = str(self.temporary)
        self.every = every
        self.foreign = set()
        self._stop = threading.Event()
        self._thread = None
        self.before = {}

    def _look(self):
        self.foreign.update(foreign_xirius(self.checkout, self.temporary))

    def _sample(self):
        while not self._stop.wait(self.every):
            self._look()

    def __enter__(self):
        try:
            self.before = listing(self.places)
            self._look()
        except BaseException:
            shutil.rmtree(self.temporary, ignore_errors=True)
            raise
        self._thread = threading.Thread(target=self._sample, daemon=True)
        self._thread.start()
        return self

    def finish(self):
        """The lines that fail the run (empty: it left nothing), after printing the warnings.
        Removes the run's temporary folder."""
        self._stop.set()
        if self._thread:
            self._thread.join()
        self._look()
        changed = differences(self.before, listing(self.places))
        left = left_in(self.temporary, SHARED)
        shutil.rmtree(self.temporary, ignore_errors=True)
        failures = []
        if changed and self.foreign:
            print("warning: the tests ran beside another Xirius ("
                  + ", ".join(sorted(self.foreign)) + "); what changed in its folders is not counted:")
            for line in changed[:SHOWN]:
                print("  " + line)
        elif changed:
            failures.append("the tests wrote to the person's Xirius folders ("
                            + ", ".join(map(str, self.places)) + "):")
            failures += ["  " + line for line in changed[:SHOWN]]
            if len(changed) > SHOWN:
                failures.append(f"  ... and {len(changed) - SHOWN} more")
        if left:
            failures.append(f"the tests left files in the temporary folder ({self.temporary}):")
            failures += ["  " + path for path in left[:SHOWN]]
            if len(left) > SHOWN:
                failures.append(f"  ... and {len(left) - SHOWN} more")
        return failures

    def __exit__(self, kind, *_):
        if not self._stop.is_set():
            if kind is None or not issubclass(kind, Exception):
                self._stop.set()
                if self._thread:
                    self._thread.join()
                shutil.rmtree(self.temporary, ignore_errors=True)
            else:
                # Failed tests are named by their own output; what they left is named here.
                failures = self.finish()
                if failures:
                    print("\n".join(failures), file=sys.stderr)
        return False

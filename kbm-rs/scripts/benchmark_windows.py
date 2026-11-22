"""Compare opt-in no-device release UIs; own process trees only, no HID/USB/NIC.

Working set is a sum (shared pages can be counted more than once); private
commit is separate, and neither includes all GPU allocations. CPU is kernel +
user time measured with the same Windows APIs for both applications.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
import os
from pathlib import Path
import statistics
import struct
import subprocess
import sys
import time


class Entry(c.Structure):
    _fields_ = [("size", w.DWORD), ("usage", w.DWORD), ("pid", w.DWORD),
                ("heap", c.c_size_t), ("module", w.DWORD), ("threads", w.DWORD),
                ("parent", w.DWORD), ("priority", w.LONG), ("flags", w.DWORD),
                ("exe", w.WCHAR * 260)]


class Memory(c.Structure):
    _fields_ = [("size", w.DWORD), ("faults", w.DWORD)] + [
        (name, c.c_size_t) for name in ("peak_ws", "ws", "peak_paged", "paged",
                                       "peak_nonpaged", "nonpaged", "pagefile", "peak_pagefile", "private")]


def ft(value):
    return value.dwLowDateTime | (value.dwHighDateTime << 32)


def cpu_percent(start, end, elapsed, logical):
    if elapsed <= 0 or logical < 1 or end < start:
        raise ValueError("invalid process time interval")
    one = (end - start) / 10_000_000 / elapsed * 100
    return {"one_core_percent": one, "machine_percent": one / logical}


def png_size(path):
    with path.open("rb") as stream:
        header = stream.read(24)
    if len(header) != 24 or header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
        raise ValueError("missing PNG frame")
    return list(struct.unpack(">II", header[16:24]))


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def electron_payload(exe):
    # Electron's launcher EXE hash alone does not identify the application code.
    root = exe.parent / "resources" / "app"
    archive = root.with_suffix(".asar")
    if archive.is_file():
        return {"app.asar": digest(archive)}
    files = [root / name for name in ("electron-main.js", "electron-preload.cjs", "index.html", "package.json", "package-lock.json")]
    files.extend(sorted((root / "assets").rglob("*")))
    result = {p.relative_to(root).as_posix(): digest(p) for p in files if p.is_file()}
    if len(result) < 5:
        raise ValueError("Electron application payload missing")
    return result


class Windows:
    def __init__(self):
        if sys.platform != "win32":
            raise RuntimeError("Windows only")
        self.k = c.WinDLL("kernel32", use_last_error=True)
        self.u = c.WinDLL("user32", use_last_error=True)
        specs = {
            "CreateToolhelp32Snapshot": ([w.DWORD, w.DWORD], w.HANDLE),
            "Process32FirstW": ([w.HANDLE, c.POINTER(Entry)], w.BOOL),
            "Process32NextW": ([w.HANDLE, c.POINTER(Entry)], w.BOOL),
            "OpenProcess": ([w.DWORD, w.BOOL, w.DWORD], w.HANDLE),
            "GetProcessTimes": ([w.HANDLE] + [c.POINTER(w.FILETIME)] * 4, w.BOOL),
            "GetExitCodeProcess": ([w.HANDLE, c.POINTER(w.DWORD)], w.BOOL),
            "K32GetProcessMemoryInfo": ([w.HANDLE, c.POINTER(Memory), w.DWORD], w.BOOL),
            "TerminateProcess": ([w.HANDLE, w.UINT], w.BOOL),
            "CloseHandle": ([w.HANDLE], w.BOOL),
        }
        for name, (args, result) in specs.items():
            fn = getattr(self.k, name)
            fn.argtypes, fn.restype = args, result
        self.u.GetForegroundWindow.argtypes, self.u.GetForegroundWindow.restype = [], w.HWND
        self.u.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
        self.u.GetWindowThreadProcessId.restype = w.DWORD

    def times(self, handle):
        created, exited, kernel, user = (w.FILETIME() for _ in range(4))
        if not self.k.GetProcessTimes(handle, c.byref(created), c.byref(exited), c.byref(kernel), c.byref(user)):
            raise c.WinError(c.get_last_error())
        return ft(created), ft(kernel) + ft(user)

    def processes(self):
        snapshot = self.k.CreateToolhelp32Snapshot(2, 0)
        if snapshot == c.c_void_p(-1).value:
            raise c.WinError(c.get_last_error())
        try:
            entry = Entry(size=c.sizeof(Entry))
            valid = self.k.Process32FirstW(snapshot, c.byref(entry))
            if not valid:
                raise c.WinError(c.get_last_error())
            result = []
            while valid:
                result.append((entry.pid, entry.parent))
                valid = self.k.Process32NextW(snapshot, c.byref(entry))
            if c.get_last_error() != 18:  # ERROR_NO_MORE_FILES
                raise c.WinError(c.get_last_error())
            return result
        finally:
            self.k.CloseHandle(snapshot)


class Tree:
    def __init__(self, api, pid):
        self.api, self.owned = api, {}
        self.add(pid)

    def add(self, pid, parent=None):
        # Keep handles until the end: a PID cannot be recycled underneath us.
        handle = self.api.k.OpenProcess(0x1000 | 0x10 | 0x1, False, pid)
        if not handle:
            raise c.WinError(c.get_last_error())
        try:
            created, cpu = self.api.times(handle)
            if parent is not None and created < self.owned[parent][1]:
                raise RuntimeError("child predates owned parent")
        except BaseException:
            self.api.k.CloseHandle(handle)
            raise
        self.owned[pid] = [handle, created, cpu]

    def sample(self):
        pending = self.api.processes()
        while True:
            children = [(pid, parent) for pid, parent in pending if parent in self.owned and pid not in self.owned]
            if not children:
                break
            for pid, parent in children:
                if len(self.owned) >= 64:
                    raise RuntimeError("unexpected process tree size")
                try:
                    self.add(pid, parent)
                except OSError as error:
                    # A short-lived child may disappear before OpenProcess.
                    if error.winerror != 87:
                        raise
            pending = [(pid, parent) for pid, parent in pending if (pid, parent) not in children]
        ws = private = alive = 0
        for handle, _created, _cpu in self.owned.values():
            code = w.DWORD()
            if not self.api.k.GetExitCodeProcess(handle, c.byref(code)):
                raise c.WinError(c.get_last_error())
            if code.value != 259:
                continue
            alive += 1
            memory = Memory(size=c.sizeof(Memory))
            if not self.api.k.K32GetProcessMemoryInfo(handle, c.byref(memory), c.sizeof(memory)):
                raise c.WinError(c.get_last_error())
            ws += memory.ws
            private += memory.private
        for value in self.owned.values():
            value[2] = self.api.times(value[0])[1]
        foreground = w.DWORD()
        self.api.u.GetWindowThreadProcessId(self.api.u.GetForegroundWindow(), c.byref(foreground))
        return {"cpu_100ns": sum(v[2] for v in self.owned.values()), "working_set_bytes": ws,
                "private_commit_bytes": private, "live_processes": alive,
                "owns_foreground": foreground.value in self.owned}

    def close(self):
        for handle, _created, _cpu in self.owned.values():
            self.api.k.CloseHandle(handle)

    def terminate_owned(self):
        # Error recovery affects only still-held handles, never a process name/PID search.
        for handle, _created, _cpu in reversed(list(self.owned.values())):
            code = w.DWORD()
            if self.api.k.GetExitCodeProcess(handle, c.byref(code)) and code.value == 259:
                self.api.k.TerminateProcess(handle, 1)


def run(api, name, exe, directory, settle=5, seconds=8):
    if directory.exists():
        raise ValueError("run directory must be new")
    started = time.perf_counter()
    with directory.with_suffix(".stderr.txt").open("xb") as log:
        env = os.environ.copy()
        env.pop("ELECTRON_RUN_AS_NODE", None)
        process = subprocess.Popen([str(exe), "--ui-benchmark", str(directory)], cwd=exe.parent,
                                   stdin=subprocess.DEVNULL, stdout=log, stderr=log, env=env,
                                   creationflags=subprocess.CREATE_NO_WINDOW)
        tree = None
        try:
            tree = Tree(api, process.pid)
            while not directory.joinpath("ready.json").exists():
                tree.sample()
                if process.poll() is not None:
                    raise RuntimeError(f"{name} exited before ready: {process.returncode}")
                if time.perf_counter() - started > 15:
                    raise TimeoutError(f"{name} UI ready timeout")
                time.sleep(.025)
            # ready is written after PNG; tolerate an in-progress small JSON write.
            for attempt in range(20):
                try:
                    ready = json.loads(directory.joinpath("ready.json").read_text("utf-8"))
                    break
                except json.JSONDecodeError:
                    if attempt == 19:
                        raise
                    time.sleep(.01)
            elapsed_ready = time.perf_counter() - started
            if ready["app"] != name or ready["devices"] != "disabled" or ready["content_dip"] != [1280, 820]:
                raise ValueError("unexpected measurement UI mode")
            if png_size(directory / "frame.png") != [ready["width_px"], ready["height_px"]]:
                raise ValueError("frame geometry mismatch")
            # No other build/test workload is started by this runner.
            until = time.perf_counter() + settle
            while time.perf_counter() < until:
                tree.sample()
                time.sleep(.25)
            baseline = tree.sample()
            beginning = time.perf_counter()
            samples = []
            while time.perf_counter() - beginning < seconds:
                time.sleep(.5)
                if process.poll() is not None:
                    raise RuntimeError("UI exited during idle measurement")
                sample = tree.sample()
                sample["elapsed_seconds"] = time.perf_counter() - beginning
                samples.append(sample)
            duration = samples[-1]["elapsed_seconds"]
            result = {"app": name, "first_snapshot_ms": elapsed_ready * 1000, "ready": ready,
                      "idle_seconds": duration, "settle_seconds": settle,
                      "cpu": cpu_percent(baseline["cpu_100ns"], samples[-1]["cpu_100ns"], duration, os.cpu_count() or 1),
                      "median_working_set_bytes": statistics.median(s["working_set_bytes"] for s in samples),
                      "median_private_commit_bytes": statistics.median(s["private_commit_bytes"] for s in samples),
                      "processes_seen": len(tree.owned), "samples": samples,
                      "frame_sha256": digest(directory / "frame.png")}
            if baseline["owns_foreground"] or any(s["owns_foreground"] for s in samples):
                raise ValueError("measurement app unexpectedly owned foreground")
            directory.joinpath("stop").touch(exist_ok=False)
            process.wait(timeout=10)
            if process.returncode != 0:
                raise RuntimeError(f"{name} nonzero exit: {process.returncode}")
            after = tree.sample()
            if after["live_processes"]:
                raise RuntimeError("owned child processes survived normal exit")
            result["exit_code"] = process.returncode
            return result
        finally:
            if tree:
                tree.terminate_owned()
                tree.close()
            elif process.poll() is None:
                process.kill()
            process.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native", required=True, type=Path)
    parser.add_argument("--electron", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path, help="new absolute report directory")
    parser.add_argument("--pairs", type=int, choices=range(1, 6), default=3)
    args = parser.parse_args()
    if not args.output.is_absolute() or not all(p.is_file() and p.is_absolute() for p in (args.native, args.electron)):
        parser.error("absolute existing executables and a new absolute output directory required")
    api = Windows()
    args.output.mkdir()
    report = {"schema": 1, "scope": "actual empty channel UI; devices disabled; fresh profile; visible inactive window",
              "startup_metric": "process launch through settled UI PNG readback/encoding; warm filesystem, not cold boot or photon latency",
              "os_build": list(sys.getwindowsversion()[:3]), "logical_processors": os.cpu_count(),
              "exe_sha256": {"native": digest(args.native), "electron": digest(args.electron)},
              "electron_payload_sha256": electron_payload(args.electron),
              "excluded": ["device discovery", "video latency/FPS", "transfer throughput", "GPU allocations", "OS DPI transitions"],
              "runs": [], "complete": False}
    try:
        executables = {"native": args.native, "electron": args.electron}
        for pair in range(args.pairs):
            order = ["native", "electron"] if pair % 2 == 0 else ["electron", "native"]
            for name in order:
                print(f"Measuring {pair + 1}/{args.pairs}: {name}", flush=True)
                result = run(api, name, executables[name], args.output / f"{pair + 1}-{name}")
                result["pair"] = pair + 1
                report["runs"].append(result)
                (args.output / "report.json").write_text(json.dumps(report, indent=2), "utf-8")
        scales = {r["ready"]["scale"] for r in report["runs"]}
        sizes = {(r["ready"]["width_px"], r["ready"]["height_px"]) for r in report["runs"]}
        if len(scales) != 1 or len(sizes) != 1:
            raise ValueError("non-comparable display scale/frame dimensions")
        report["complete"] = True
    finally:
        (args.output / "report.json").write_text(json.dumps(report, indent=2), "utf-8")


if __name__ == "__main__":
    main()

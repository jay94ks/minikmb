"""Rebuild the GPL helper from this source archive in Linux/WSL.

Requires Python 3, Meson, Ninja, patch and x86_64-w64-mingw32 GCC/binutils.
This is a developer build, never a runtime download or flashing command.
"""
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent


def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)


def main():
    source = ROOT / "build/flashrom"
    if source.exists():
        raise SystemExit("Use a fresh extracted source directory; build/ already exists")
    shutil.copytree(ROOT / "flashrom", source)
    subprocess.run(["patch", "-p1", "-i", str(ROOT / "serial-timeout.patch")], cwd=source, check=True)
    run("meson", "setup", "build/library", str(source), "--cross-file", "flashrom-mingw.ini",
        "-Dprogrammer=serprog", "-Dclassic_cli=disabled", "-Dtests=disabled", "-Ddocumentation=disabled",
        "-Dman-pages=disabled", "-Drpmc=disabled", "-Ddefault_library=static", "-Dich_descriptors_tool=disabled",
        "-Dbash_completion=disabled", "-Dni845x_search_path=/nonexistent", "-Duse_git_version=disabled", "-Dlog_message_length_limit=1024")
    run("meson", "compile", "-C", "build/library")
    run("x86_64-w64-mingw32-gcc", "-std=c99", "-O2", "-D_WIN32_WINNT=0x0601", "-DWINVER=0x0601",
        "-D__USE_MINGW_ANSI_STDIO=1", "-DIS_WINDOWS=1", "-D__FLASHROM_LITTLE_ENDIAN__=1", "-I", str(source / "include"),
        "helper.c", "bridge.c", "build/library/libflashrom.a", "-static", "-municode", "-Wl,--no-insert-timestamp",
        "-o", "build/minikmb-ms2130-helper.exe")
    print("Built build/minikmb-ms2130-helper.exe; no device was accessed")


if __name__ == "__main__":
    main()

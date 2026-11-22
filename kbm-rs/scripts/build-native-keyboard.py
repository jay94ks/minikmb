"""Build the in-process Win32 DLL and no-hook test executable; no downloads."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "native-keyboard/keyboard.cpp"
if not SOURCE.is_file():
    SOURCE = Path(__file__).with_name("minikmb-keyboard.cpp")


def main():
    vswhere = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Microsoft Visual Studio/Installer/vswhere.exe"
    install = subprocess.check_output([str(vswhere), "-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"], text=True).strip()
    if not install:
        raise SystemExit("Local MSVC x64 build tools required")
    setup = Path(install) / "VC/Auxiliary/Build/vcvars64.bat"
    out = ROOT / "target/native-keyboard"
    out.mkdir(parents=True, exist_ok=True)
    bootstrap = out / "msvc-environment.cmd"
    bootstrap.write_text(f'@call "{setup}" >nul\n@if errorlevel 1 exit /b 1\n@set\n', encoding="utf-8")
    raw = subprocess.check_output(["cmd.exe", "/d", "/c", str(bootstrap)], text=True)
    env = dict(os.environ)
    env.update(line.split("=", 1) for line in raw.splitlines() if "=" in line and not line.startswith("="))
    executable = out / "minikmb-keyboard.exe"
    library = out / "minikmb-keyboard.dll"
    compiler = shutil.which("cl.exe", path=env.get("Path", env.get("PATH", "")))
    if not compiler:
        raise SystemExit("MSVC environment has no cl.exe")
    inputs = hashlib.sha256(SOURCE.read_bytes() + Path(__file__).read_bytes() + compiler.encode("utf-8")).hexdigest()
    manifest_path = out / "manifest.json"
    previous = json.loads(manifest_path.read_text(encoding="utf-8")) if manifest_path.is_file() else {}
    if not (executable.is_file() and library.is_file() and previous.get("build_inputs_sha256") == inputs and previous.get("sha256") == hashlib.sha256(library.read_bytes()).hexdigest()):
        subprocess.run([compiler, "/nologo", "/std:c++20", "/O2", "/MT", "/EHsc", "/W4", "/WX", "/utf-8", "/D_CRT_SECURE_NO_WARNINGS", str(SOURCE), f"/Fo{out / 'keyboard.obj'}", f"/Fe{executable}", "/link", "/Brepro", "user32.lib", "kernel32.lib"], env=env, cwd=ROOT, check=True)
        subprocess.run([compiler, "/nologo", "/std:c++20", "/O2", "/MT", "/EHsc", "/W4", "/WX", "/utf-8", "/LD", "/DMINIKMB_KEYBOARD_DLL", "/D_CRT_SECURE_NO_WARNINGS", str(SOURCE), f"/Fo{out / 'keyboard-dll.obj'}", f"/Fe{library}", "/link", "/Brepro", "user32.lib", "kernel32.lib"], env=env, cwd=ROOT, check=True)
    subprocess.run([str(executable), "--self-test"], check=True)
    manifest = {"file": library.name, "sha256": hashlib.sha256(library.read_bytes()).hexdigest(), "test_executable_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(), "source_sha256": hashlib.sha256(SOURCE.read_bytes()).hexdigest(), "build_inputs_sha256": inputs, "origin": "In-process Win32 DLL with dedicated thread; no runtime child process or pipe"}
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    for relative in ["target/release/tools", "target/debug/tools", "target/debug/deps/tools"]:
        destination = ROOT / relative
        destination.mkdir(parents=True, exist_ok=True)
        for artifact in ([library] if relative == "target/release/tools" else [library, executable]):
            staged = destination / artifact.name
            if not staged.is_file() or hashlib.sha256(staged.read_bytes()).hexdigest() != hashlib.sha256(artifact.read_bytes()).hexdigest():
                shutil.copy2(artifact, staged)
    print(json.dumps(manifest))


if __name__ == "__main__":
    main()

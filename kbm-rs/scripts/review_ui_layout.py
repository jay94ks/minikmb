"""Render inert real UI previews at controlled egui scales (not OS DPI tests)."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import shutil
from benchmark_windows import png_size

PAGES = {
    "firmware": ["--style-preview", "--firmware-preview"],
    "channels": [], "control": ["--control-preview"],
    "keys": ["--style-preview", "--keys-preview"],
    "storage": ["--style-preview", "--storage-preview"],
    "network": ["--style-preview", "--network-preview"],
    "tty": ["--style-preview", "--tty-preview"],
    "preferences": ["--style-preview", "--preferences-preview"],
    "api": ["--style-preview", "--api-preview"],
    "mcp": ["--style-preview", "--mcp-preview"],
    "logs": ["--style-preview", "--logs-preview"],
    "diagnostics": ["--style-preview", "--diagnostics-preview"],
    "wake": ["--style-preview", "--wake-preview"],
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--pages", nargs="+", choices=list(PAGES), default=list(PAGES))
    args = parser.parse_args()
    if not args.exe.is_absolute() or not args.exe.is_file() or not args.output.is_absolute():
        parser.error("absolute existing executable and new absolute output directory required")
    args.output.mkdir()
    # Keep a fixed binary for every case and allow unrelated subsequent builds.
    executable = args.output / "preview.exe"
    shutil.copyfile(args.exe, executable)
    report = {"scope": "controlled egui layout scale; no OS DPI transition/IME or real HID verification",
              "exe_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(), "runs": [], "complete": False}
    try:
        for page in args.pages:
            for size in ("840x560", "1280x820"):
                for scale in (100, 150, 200):
                    name = f"{page}-{size}-{scale}"
                    print(name, flush=True)
                    image = args.output / f"{name}.png"
                    run = subprocess.run([str(executable), "--screenshot", str(image),
                        "--preview-scale", str(scale), "--preview-size", size, *PAGES[page]],
                        stdin=subprocess.DEVNULL, capture_output=True, timeout=20,
                        creationflags=subprocess.CREATE_NO_WINDOW)
                    image.with_suffix(".stderr.txt").write_bytes(run.stderr)
                    if run.returncode or run.stderr or not image.exists():
                        raise RuntimeError(f"{name}: exit {run.returncode}; {run.stderr.decode('utf-8','replace')[:1024]}")
                    info = json.loads(image.with_suffix(".json").read_text("utf-8"))
                    logical = [int(n) for n in size.split("x")]
                    expected = [round(n * scale / 100) for n in logical]
                    if not (info["hardware_disabled"] and info["logical_size"] == logical and
                            abs(info["actual_scale"] - scale / 100) < .001 and
                            info["pixels"] == png_size(image) == expected and
                            all(abs(a-b) < 1 for a,b in zip(info["viewport_size"],logical))):
                        raise ValueError(f"{name}: incorrect preview geometry: {info}")
                    report["runs"].append({"page": page, "size": logical, "scale": scale,
                        "pixels": expected, "exit_code": run.returncode,
                        "png_sha256": hashlib.sha256(image.read_bytes()).hexdigest(),
                        "metadata": info})
                    (args.output / "report.json").write_text(json.dumps(report, indent=2), "utf-8")
        report["complete"] = True
    finally:
        (args.output / "report.json").write_text(json.dumps(report, indent=2), "utf-8")


if __name__ == "__main__":
    main()

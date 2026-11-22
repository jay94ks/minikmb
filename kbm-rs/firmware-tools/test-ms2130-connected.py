"""Explicit one-shot test of the new helper; default is read-only.

Requires the exact user-prepared writer serial. --execute records the bundled ROM
once, preserving the tail in RAM and verifying the entire external flash.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import serial.tools.list_ports

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--writer", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    if args.output.exists() or not args.output.is_absolute():
        raise ValueError("An absolute new report file is required")
    writers = [p for p in serial.tools.list_ports.comports() if p.vid == 0x2e8a and p.pid == 10 and p.serial_number == args.writer]
    if len(writers) != 1:
        raise ValueError("The exact frwr writer is not uniquely connected")
    helper = ROOT / "target/firmware-tools/ms2130-helper/minikmb-ms2130-helper.exe"
    result = subprocess.run([str(helper), "--probe", writers[0].device], stdin=subprocess.DEVNULL, capture_output=True, timeout=40)
    events = [json.loads(s) for s in result.stdout.decode("utf-8").splitlines() if s.startswith("{")]
    if result.returncode:
        raise RuntimeError("frwr preflight failed")
    chip = next(e["chip"] for e in events if e["event"] == "identified")
    if not chip:
        raise ValueError("A compatible flash capacity/driver was not resolved")
    report = {"writer": args.writer, "preflight": events, "programmed": False}
    if args.execute:
        manifest = json.loads((ROOT / "firmware-tools/images.lock.json").read_text(encoding="utf-8"))
        image = next(i for i in manifest["images"] if i["target"] == "ms2130")
        source = ROOT / "target/firmware-tools/package/firmware" / image["file"]
        data = source.read_bytes()
        if len(data) != image["bytes"] or hashlib.sha256(data).hexdigest() != image["sha256"]:
            raise ValueError("Bundled ROM integrity check failed")
        if len(data) > chip["capacity"]:
            raise ValueError("Flash is smaller than the ROM; do not erase/write")
        result = subprocess.run([str(helper), "--write", writers[0].device, str(chip["capacity"]), str(source)],
                                input=b"Y", capture_output=True, timeout=180)
        events = [json.loads(s) for s in result.stdout.decode("utf-8").splitlines() if s.startswith("{")]
        report.update({"image_sha256": image["sha256"], "image_bytes": len(data), "events": events, "exit_code": result.returncode,
                       "helper_sha256": hashlib.sha256(helper.read_bytes()).hexdigest()})
        completed = next((e for e in events if e["event"] == "completed"), None)
        report["programmed"] = bool(result.returncode == 0 and completed and completed["verified"] and completed["tail_preserved"]
                                    and completed["verified_bytes"] == chip["capacity"])
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"programmed": report["programmed"], "capacity": chip["capacity"], "report": str(args.output)}))
    if args.execute and not report["programmed"]:
        raise RuntimeError("ROM programming/readback did not pass; see report")


if __name__ == "__main__":
    main()

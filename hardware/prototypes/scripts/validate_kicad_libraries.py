from __future__ import annotations

import csv
import re
import sys
from pathlib import Path

sys.dont_write_bytecode = True
import generate_kicad_libraries as gen


ROOT = Path(__file__).resolve().parents[1]


def balanced_sexpr(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    depth = 0
    quoted = False
    escaped = False
    for char in text:
        if escaped:
            escaped = False
        elif char == "\\" and quoted:
            escaped = True
        elif char == '"':
            quoted = not quoted
        elif not quoted and char == "(":
            depth += 1
        elif not quoted and char == ")":
            depth -= 1
            if depth < 0:
                raise AssertionError(f"{path}: closing parenthesis without opener")
    assert not quoted, f"{path}: unterminated quoted string"
    assert depth == 0, f"{path}: unbalanced parentheses ({depth})"


def validate_symbols() -> list[str]:
    path = ROOT / "symbols" / "Future_ICS.kicad_sym"
    balanced_sexpr(path)
    text = path.read_text(encoding="utf-8")
    results = []
    for symbol in gen.SYMBOLS:
        assert f'(symbol "{symbol["name"]}"' in text
        csv_path = ROOT / "pinouts" / f'{symbol["name"]}.csv'
        with csv_path.open(encoding="utf-8-sig", newline="") as handle:
            rows = list(csv.DictReader(handle))
        actual = [(row["pin"], row["name"], row["electrical_type"]) for row in rows]
        expected = [(pin["number"], pin["name"], pin["type"]) for pin in symbol["pins"]]
        assert actual == expected, f"{symbol['name']}: CSV and generator pin data differ"
        results.append(f"PASS symbol {symbol['name']}: {len(expected)} pins")
    return results


def validate_footprints() -> list[str]:
    expected = {
        "QFN-64-1EP_9x9mm_P0.5mm_EP5.7x5.7mm_MS2130": set(range(1, 66)),
        "QFN-76-1EP_9x9mm_P0.4mm_EP5.6x5.6mm_GL3523": set(range(1, 78)),
        "QFN-32-1EP_5x5mm_P0.5mm_EP3.4x3.4mm_LDR6023Q": set(range(1, 34)),
        "SOT-23-6_DBV": set(range(1, 7)),
        "WSON-6-1EP_2x2mm_P0.65mm_DRV": set(range(1, 8)),
        "TSOT-23-8_MP2315": set(range(1, 9)),
        "SOIC-8_3.9x4.9mm_P1.27mm_MP2482DS": set(range(1, 9)),
        "SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.62x3.51mm_MP2482DN": set(range(1, 10)),
        gen.HDMI_FOOTPRINT: set(range(1, 21)),
    }
    actual_files = {path.stem for path in (ROOT / "footprints" / "Future_ICS.pretty").glob("*.kicad_mod")}
    assert actual_files == set(expected), f"footprint file set differs: {sorted(actual_files ^ set(expected))}"
    results = []
    for name, wanted in expected.items():
        path = ROOT / "footprints" / "Future_ICS.pretty" / f"{name}.kicad_mod"
        balanced_sexpr(path)
        text = path.read_text(encoding="utf-8")
        numbered = [int(value) for value in re.findall(r'^\s*\(pad "(\d+)"', text, re.MULTILINE)]
        if name == gen.HDMI_FOOTPRINT:
            assert numbered.count(20) == 4, "HDMI: four mounting tabs must share SHELL pad 20"
            assert len(numbered) == 23 and all(numbered.count(i) == 1 for i in range(1, 20))
        else:
            assert len(numbered) == len(set(numbered)), f"{name}: duplicate numbered pads"
        assert set(numbered) == wanted, f"{name}: pads {sorted(numbered)} != {sorted(wanted)}"
        assert '(layer "F.CrtYd")' in text and '(layer "F.Fab")' in text
        results.append(f"PASS footprint {name}: {len(numbered)} numbered pads")
    dimension_tokens = {
        "QFN-64-1EP_9x9mm_P0.5mm_EP5.7x5.7mm_MS2130": ['(at -3.75 4.407)', '(size 0.28 0.665)', '(size 5.7 5.7)'],
        "QFN-76-1EP_9x9mm_P0.4mm_EP5.6x5.6mm_GL3523": ['(at -3.6 4.4)', '(size 0.2 0.8)', '(size 5.6 5.6)'],
        "QFN-32-1EP_5x5mm_P0.5mm_EP3.4x3.4mm_LDR6023Q": ['(at -1.75 2.35)', '(size 0.25 0.7)', '(size 3.4 3.4)'],
        "SOT-23-6_DBV": ['(at -1.3 0.95)', '(size 1.1 0.6)'],
        "WSON-6-1EP_2x2mm_P0.65mm_DRV": ['(at -0.975 -0.65)', '(size 0.45 0.3)', '(size 1 1.6)'],
        "TSOT-23-8_MP2315": ['(at -1.3 0.975)', '(size 1.2 0.4)'],
        "SOIC-8_3.9x4.9mm_P1.27mm_MP2482DS": ['(at -2.7 1.905)', '(size 1.6 0.61)'],
        "SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.62x3.51mm_MP2482DN": ['(size 2.62 3.51)', '(size 1.6 0.61)'],
        gen.HDMI_FOOTPRINT: ['(at -4.5 0.76)', '(at 4.5 0.76)', '(size 0.28 2.6)', '(at -7.25 0)', '(at 7.25 -5.96)', '(drill oval 0.9 2.7)', '(drill oval 0.9 2.1)'],
    }
    for name, tokens in dimension_tokens.items():
        text = (ROOT / "footprints" / "Future_ICS.pretty" / f"{name}.kicad_mod").read_text(encoding="utf-8")
        for token in tokens:
            assert token in text, f"{name}: missing verified geometry token {token}"
    results.append("PASS verified footprint geometry tokens")
    return results


def validate_tables() -> list[str]:
    results = []
    for name in ("sym-lib-table", "fp-lib-table"):
        path = ROOT / name
        balanced_sexpr(path)
        assert "${KIPRJMOD}" in path.read_text(encoding="utf-8")
        results.append(f"PASS {name}")
    return results


def main() -> None:
    results = validate_symbols() + validate_footprints() + validate_tables()
    results.append("NOTE: structural checks only. Native KiCad verification for the reused HDMI connector is recorded in docs/HDMI_A_REUSED.md.")
    report = "\n".join(results) + "\n"
    (ROOT / "validation-report.txt").write_text(report, encoding="utf-8")
    print(report, end="")


if __name__ == "__main__":
    main()

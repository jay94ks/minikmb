"""Run with KiCad's bundled Python (pcbnew), not the ordinary Python runtime."""
from collections import Counter
from pathlib import Path
import re
import pcbnew

ROOT = Path(__file__).resolve().parents[1]
NAME = "HDMI_A_19P_Reused_WFD6_Compatible"


def mm(value):
    return round(pcbnew.ToMM(value), 6)


def main():
    fp = pcbnew.FootprintLoad(str(ROOT / "footprints/Future_ICS.pretty"), NAME)
    assert fp is not None, "KiCad could not parse footprint"
    pads = list(fp.Pads())
    counts = Counter(p.GetNumber() for p in pads)
    assert counts == Counter({**{str(n): 1 for n in range(1, 20)}, "20": 4})
    signals = {int(p.GetNumber()): p for p in pads if p.GetNumber() != "20"}
    for n, pad in signals.items():
        assert pad.GetAttribute() == pcbnew.PAD_ATTRIB_SMD
        assert (mm(pad.GetPosition().x), mm(pad.GetPosition().y)) == (-4.5 + (n - 1) * 0.5, 0.76)
        assert (mm(pad.GetSize().x), mm(pad.GetSize().y)) == (0.28, 2.6)
    expected = {
        (-7.25, 0.0): ((1.5, 3.3), (0.9, 2.7)),
        (7.25, 0.0): ((1.5, 3.3), (0.9, 2.7)),
        (-7.25, -5.96): ((1.5, 2.7), (0.9, 2.1)),
        (7.25, -5.96): ((1.5, 2.7), (0.9, 2.1)),
    }
    for pad in pads:
        if pad.GetNumber() != "20":
            continue
        assert pad.GetAttribute() == pcbnew.PAD_ATTRIB_PTH
        assert pad.GetDrillShape() == pcbnew.PAD_DRILL_SHAPE_OBLONG
        position = (mm(pad.GetPosition().x), mm(pad.GetPosition().y))
        size, drill = expected.pop(position)
        assert (mm(pad.GetSize().x), mm(pad.GetSize().y)) == size
        assert (mm(pad.GetDrillSize().x), mm(pad.GetDrillSize().y)) == drill
    assert not expected
    text = (ROOT / "symbols/Future_ICS.kicad_sym").read_text(encoding="utf-8")
    connector = text.split('(symbol "HDMI_A_19P_Reused"', 1)[1]
    assert '(property "Reference" "J"' in connector
    assert f'Future_ICS:{NAME}' in connector
    assert re.findall(r'\(number "([^"]+)"', connector) == [str(n) for n in range(1, 21)]
    assert connector.count('(pin passive line') == 20
    report = [
        f"KiCad native parser: {pcbnew.Version()}",
        "PASS footprint parse: 19 SMD contacts + 4 plated oblong mounting slots",
        "PASS signal pitch 0.50mm; contact 1 at (-4.5,0.76); contact 19 at (+4.5,0.76)",
        "PASS shell centers 14.50 x 5.96mm; rear drills 0.9x2.7; front drills 0.9x2.1",
        "PASS one passive SHELL pin 20 maps to all four shell pads; contacts 1..19 stay separate",
        "NOTE physical fit and connector contact continuity still require the user's real part.",
    ]
    out = "\n".join(report) + "\n"
    (ROOT / "docs/HDMI_A_NATIVE_CHECK.txt").write_text(out, encoding="utf-8")
    print(out)


if __name__ == "__main__":
    main()

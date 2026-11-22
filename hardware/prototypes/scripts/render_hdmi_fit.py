"""Print-scale SVG and enlarged PNG from the actual generated footprint pads.

Requires Pillow for the PNG; use the bundled workspace Python runtime.
"""
from pathlib import Path
import re
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
NAME = "HDMI_A_19P_Reused_WFD6_Compatible"
OUT = ROOT / "docs"


def read_pads():
    text = (ROOT / "footprints/Future_ICS.pretty" / (NAME + ".kicad_mod")).read_text(encoding="utf-8")
    result = []
    for line in text.splitlines():
        if '(pad "' not in line:
            continue
        number = int(re.search(r'\(pad "(\d+)"', line)[1])
        x, y = map(float, re.search(r'\(at ([\d.-]+) ([\d.-]+)\)', line).groups())
        sx, sy = map(float, re.search(r'\(size ([\d.-]+) ([\d.-]+)\)', line).groups())
        drill_match = re.search(r'\(drill oval ([\d.-]+) ([\d.-]+)\)', line)
        result.append((number, x, y, sx, sy, tuple(map(float, drill_match.groups())) if drill_match else None))
    assert len(result) == 23
    return result


def svg_pattern(pads, tx, ty, scale):
    lines = [f'<g transform="translate({tx} {ty}) scale({scale})">',
             '<rect x="-7.5" y="-10.14" width="15" height="12.2" fill="none" stroke="#666" stroke-width="0.1"/>',
             '<path d="M-9,-10.09 H9" stroke="#16834b" stroke-width="0.1" stroke-dasharray="0.6 0.3"/>']
    for number, x, y, sx, sy, drill in pads:
        radius = sx / 2 if drill else 0
        lines.append(f'<rect x="{x-sx/2}" y="{y-sy/2}" width="{sx}" height="{sy}" rx="{radius}" fill="#e4bd76" stroke="#111" stroke-width="0.035"/>')
        if drill:
            dx, dy = drill
            lines.append(f'<rect x="{x-dx/2}" y="{y-dy/2}" width="{dx}" height="{dy}" rx="{dx/2}" fill="white" stroke="#111" stroke-width="0.035"/>')
    for n, x in ((1, -4.5), (19, 4.5)):
        lines.append(f'<text x="{x}" y="3.1" font-size="0.8" text-anchor="middle">{n}</text>')
    lines.append('</g>')
    return lines


def main():
    pads = read_pads()
    OUT.mkdir(exist_ok=True)
    svg = ['<svg xmlns="http://www.w3.org/2000/svg" width="210mm" height="148mm" viewBox="0 0 210 148">',
           '<rect width="210" height="148" fill="white"/>',
           '<g font-family="Arial,sans-serif" fill="#111">',
           '<text x="10" y="12" font-size="5">HDMI-A reused connector: physical fit sheet</text>',
           '<text x="10" y="20" font-size="3.2">Component side / top view. Opening faces UP. No mirroring.</text>',
           '<text x="27" y="42" font-size="4">ACTUAL SIZE 1:1</text>',
           '<text x="118" y="29" font-size="4">ENLARGED 4:1</text>']
    svg += svg_pattern(pads, 45, 70, 1)
    svg += svg_pattern(pads, 145, 80, 4)
    svg += ['<text x="45" y="55" font-size="2.5" text-anchor="middle">CABLE / MATING</text>',
            '<text x="145" y="36" font-size="3" text-anchor="middle">CABLE / MATING</text>',
            '<path d="M40,90 H50 M40,88.5 V91.5 M50,88.5 V91.5" stroke="black" stroke-width="0.2"/>',
            '<text x="45" y="97" font-size="3" text-anchor="middle">10 mm calibration</text>',
            '<text x="10" y="114" font-size="3.2">Print at 100% / Actual size. Disable Fit to page. Verify the 10 mm bar.</text>',
            '<text x="10" y="121" font-size="3.2">Use the LEFT 1:1 pattern to check all 4 tabs and all 19 contact tails.</text>',
            '<text x="10" y="128" font-size="3.2">Pitch 0.50 mm | shell centers 14.50 x 5.96 mm | reference fit, not yet test fitted.</text>',
            '<text x="10" y="135" font-size="3.2">The green dashed line is a suggested PCB edge, not a board outline.</text>',
            '</g></svg>']
    (OUT / "HDMI_A_REUSED_fit_1to1.svg").write_text("\n".join(svg) + "\n", encoding="utf-8")

    image = Image.new("RGB", (1600, 1100), "white")
    draw = ImageDraw.Draw(image)
    font_path = "C:/Windows/Fonts/segoeui.ttf"
    title = ImageFont.truetype(font_path, 32)
    font = ImageFont.truetype(font_path, 23)
    small = ImageFont.truetype(font_path, 19)
    draw.text((55, 25), "HDMI-A 19P - reused connector / component-side view", font=title, fill="#14253d")
    draw.text((55, 74), "Reference land pattern: Foxconn DWG 301-0000-2217, rev A. Physical fit pending.", font=font, fill="#444444")
    cx, cy, scale = 840, 780, 60

    def box(x, y, sx, sy):
        return (cx+(x-sx/2)*scale, cy+(y-sy/2)*scale,
                cx+(x+sx/2)*scale, cy+(y+sy/2)*scale)

    draw.rectangle((cx-7.5*scale, cy-10.14*scale, cx+7.5*scale, cy+2.06*scale), outline="#777777", width=3)
    draw.text((cx-105, cy-10.14*scale-35), "CABLE / MATING", font=font, fill="#12643e")
    for number, x, y, sx, sy, drill in pads:
        bounds = box(x, y, sx, sy)
        if drill:
            draw.rounded_rectangle(bounds, radius=sx*scale/2, fill="#e9c576", outline="#6f4d00", width=2)
            dx, dy = drill
            draw.rounded_rectangle(box(x, y, dx, dy), radius=dx*scale/2, fill="white", outline="#333333", width=2)
        else:
            draw.rectangle(bounds, fill="#d88626", outline="#6f4d00", width=1)
    draw.text((cx-165, cy-5*scale), "19 contacts / 0.50 mm pitch", font=font, fill="#333333")
    draw.text((cx-140, cy-4.4*scale), "All 4 shell pads = pin 20", font=font, fill="#333333")
    for number, x in ((1, -4.5), (19, 4.5)):
        draw.text((cx+x*scale-8, cy+2.4*scale), str(number), font=font, fill="#ad3000")
    y = 995
    draw.line((cx-7.25*scale, y, cx+7.25*scale, y), fill="#14253d", width=2)
    for x in (cx-7.25*scale, cx+7.25*scale):
        draw.line((x,y-10,x,y+10), fill="#14253d", width=2)
    draw.text((cx-120, y-33), "14.50 mm shell centers", font=font, fill="#14253d")
    draw.text((65, 440), "Front slots:", font=font, fill="#333333")
    draw.text((65, 478), "0.9 x 2.1 mm", font=font, fill="#333333")
    draw.text((65, 690), "Rear slots:", font=font, fill="#333333")
    draw.text((65, 728), "0.9 x 2.7 mm", font=font, fill="#333333")
    draw.text((65, 1050), "This PNG is enlarged. For physical comparison, print HDMI_A_REUSED_fit_1to1.svg at 100%.", font=small, fill="#555555")
    image.save(OUT / "HDMI_A_REUSED_preview.png")
    print("Generated print-scale SVG and enlarged footprint PNG")


if __name__ == "__main__":
    main()

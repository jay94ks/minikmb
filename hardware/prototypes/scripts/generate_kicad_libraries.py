from __future__ import annotations

import csv
import math
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SYM_DIR = ROOT / "symbols"
FP_DIR = ROOT / "footprints" / "Future_ICS.pretty"
PIN_DIR = ROOT / "pinouts"


def p(number: str | int, name: str, kind: str, description: str = "") -> dict[str, str]:
    return {"number": str(number), "name": name, "type": kind, "description": description}


MS2130 = [
    p(1, "DVDD33_1", "power_in"), p(2, "AVDD33_USB1", "power_in"),
    p(3, "DM", "bidirectional", "USB 2.0 D-"), p(4, "DP", "bidirectional", "USB 2.0 D+"),
    p(5, "AVDD33_USB2", "power_in"), p(6, "AVDD11_USB1", "power_in"),
    p(7, "USB_REXT", "passive", "USB external reference resistor"), p(8, "AVDD33_USB3", "power_in"),
    p(9, "SSTXM", "output", "USB 3.x SuperSpeed TX-"), p(10, "AVDD11_USB2", "power_in"),
    p(11, "SSTXP", "output", "USB 3.x SuperSpeed TX+"), p(12, "AVDD11_USB3", "power_in"),
    p(13, "SSRXM", "input", "USB 3.x SuperSpeed RX-"), p(14, "SSRXP", "input", "USB 3.x SuperSpeed RX+"),
    p(15, "AVSS_1", "power_in"), p(16, "DVDD33_2", "power_in"), p(17, "DVSS_1", "power_in"),
    p(18, "DVDD11_1", "power_in"), p(19, "AVSS_2", "power_in"), p(20, "AVDD25_1", "power_in"),
    p(21, "XTIN", "input", "Crystal/clock input"), p(22, "XTOUT", "output", "Crystal output"),
    p(23, "AVDD25_2", "power_in"), p(24, "AVDD11_1", "power_in"), p(25, "AVSS_3", "power_in"),
    p(26, "RXDDCSCL", "input", "HDMI DDC clock"), p(27, "RXDDCSDA", "bidirectional", "HDMI DDC data"),
    p(28, "HDRXDET", "input", "HDMI receiver detect"), p(29, "HDRXHPD", "output", "HDMI hot-plug detect"),
    p(30, "DVSS_2", "power_in"), p(31, "DVDD33_3", "power_in"), p(32, "DVSS_3", "power_in"),
    p(33, "DVDD33_4", "power_in"), p(34, "DVSS_4", "power_in"),
    p(35, "GPIO4/SPIMOSI/SCL", "bidirectional"), p(36, "GPIO3/SPISCLK/URRX/SDA", "bidirectional"),
    p(37, "GPIO5/SPIMISO", "bidirectional"), p(38, "GPIO2/SPICS/URTX", "bidirectional"),
    p(39, "DVDD11_2", "power_in"), p(40, "DVSS_5", "power_in"), p(41, "DVDD33_5", "power_in"),
    p(42, "GPIO0", "bidirectional"), p(43, "GPIO1", "bidirectional"), p(44, "MCU_SEL", "input"),
    p(45, "HDRX_REXT", "passive", "HDMI receiver external reference resistor"),
    p(46, "HDRXCN", "input", "HDMI receiver clock-"), p(47, "AVDD25_3", "power_in"),
    p(48, "AVDD33_1", "power_in"), p(49, "HDRXCP", "input", "HDMI receiver clock+"),
    p(50, "AVSS_4", "power_in"), p(51, "AVDD11_2", "power_in"), p(52, "AVDD33_2", "power_in"),
    p(53, "HDRX0N", "input"), p(54, "HDRX0P", "input"), p(55, "AVDD11_3", "power_in"),
    p(56, "AVDD33_3", "power_in"), p(57, "HDRX1N", "input"), p(58, "HDRX1P", "input"),
    p(59, "AVDD11_4", "power_in"), p(60, "AVDD33_4", "power_in"), p(61, "HDRX2N", "input"),
    p(62, "HDRX2P", "input"), p(63, "DVO_D7", "output"), p(64, "DVSS_6", "power_in"),
    p(65, "EP", "power_in", "Exposed pad; connect to analog ground"),
]


GL3523 = [
    p(1, "PAMBER_P4", "bidirectional"), p(2, "PGREEN_P3", "bidirectional"),
    p(3, "PAMBER_P3", "bidirectional"), p(4, "AVDD12", "power_in"),
    p(5, "TXN1_P4", "output"), p(6, "TXP1_P4", "output"), p(7, "VP12_P4", "power_in"),
    p(8, "RXN1_P4", "input"), p(9, "RXP1_P4", "input"), p(10, "DP_P4", "bidirectional"),
    p(11, "DM_P4", "bidirectional"), p(12, "AVDD33_2", "power_in"),
    p(13, "DP_P3", "bidirectional"), p(14, "DM_P3", "bidirectional"),
    p(15, "TXN1_P3", "output"), p(16, "TXP1_P3", "output"), p(17, "VP12_P3", "power_in"),
    p(18, "RXN1_P3", "input"), p(19, "RXP1_P3", "input"), p(20, "TXN1_P2", "output"),
    p(21, "TXP1_P2", "output"), p(22, "VP12_P2", "power_in"), p(23, "RXN1_P2", "input"),
    p(24, "RXP1_P2", "input"), p(25, "DP_P2", "bidirectional"), p(26, "DM_P2", "bidirectional"),
    p(27, "AVDD33_3", "power_in"), p(28, "DVDD12", "power_in"),
    p(29, "PGREEN_P2", "bidirectional"), p(30, "PAMBER_P2", "bidirectional"),
    p(31, "OVCUR4J/SMD", "input"), p(32, "PWREN4J", "bidirectional"),
    p(33, "PWREN3J", "bidirectional"), p(34, "OVCUR3J/SMC", "input"),
    p(35, "OVCUR2J", "input"), p(36, "PWREN2J", "bidirectional"),
    p(37, "PWREN1J", "bidirectional"), p(38, "OVCUR1J", "input"),
    p(39, "FB", "input"), p(40, "VSSP", "power_in"), p(41, "SW", "power_out"),
    p(42, "VDDP", "power_in"), p(43, "V5", "power_in"), p(44, "V33", "power_out"),
    p(45, "CHIPEN", "input"), p(46, "RTERM", "passive"), p(47, "DVDD12", "power_in"),
    p(48, "RESETJ", "input"), p(49, "P_SPI_DO", "bidirectional"),
    p(50, "P_SPI_CK", "bidirectional"), p(51, "P_SPI_DI", "bidirectional"),
    p(52, "P_SPI_CZ", "bidirectional"), p(53, "PGANG", "input"), p(54, "VBUS", "power_in"),
    p(55, "DVDD33", "power_in"), p(56, "X2", "output"), p(57, "X1", "input"),
    p(58, "TXN1_P1", "output"), p(59, "TXP1_P1", "output"), p(60, "VP12_P1", "power_in"),
    p(61, "RXN1_P1", "input"), p(62, "RXP1_P1", "input"), p(63, "DP_P1", "bidirectional"),
    p(64, "DM_P1", "bidirectional"), p(65, "AVDD33_1", "power_in"),
    p(66, "DP_P0", "bidirectional"), p(67, "DM_P0", "bidirectional"),
    p(68, "TXN1_P0", "output"), p(69, "TXP1_P0", "output"), p(70, "VP12_P0", "power_in"),
    p(71, "RXN1_P0", "input"), p(72, "RXP1_P0", "input"), p(73, "AVDD33", "power_in"),
    p(74, "PGREEN_P1", "bidirectional"), p(75, "PAMBER_P1", "bidirectional"),
    p(76, "PGREEN_P4", "bidirectional"), p(77, "EP", "power_in", "Exposed pad; connect to ground"),
]


TPS_DBV = [
    p(1, "IN", "power_in"), p(2, "GND", "power_in"), p(3, "EN", "input"),
    p(4, "~{FAULT}", "open_collector", "Active-low open-drain fault output"),
    p(5, "ILIM", "output", "Current-limit programming resistor; current-source output"), p(6, "OUT", "power_out"),
]

TPS_DRV = [
    p(1, "OUT", "power_out"), p(2, "ILIM", "output", "Current-limit programming resistor; current-source output"),
    p(3, "~{FAULT}", "open_collector", "Active-low open-drain fault output"), p(4, "EN", "input"),
    p(5, "GND", "power_in"), p(6, "IN", "power_in"),
    p(7, "EP", "power_in", "PowerPAD; connect externally to GND"),
]

LDR6023Q = [
    p(1, "VDD", "power_in", "3.3 V power input"), p(2, "C0VD", "input", "C0 port VBUS detect"),
    p(3, "C1VD", "input", "C1 port VBUS detect"), p(4, "PDRST", "input", "External chip reset"),
    p(5, "VDDA", "power_in", "3.3 V analog power input"), p(6, "VCOD", "input", "VCONN detect"),
    p(7, "CREF", "input", "USB PD voltage reference"), p(8, "C01M", "input", "C0 port CC1 voltage detect"),
    p(9, "C1HU", "power_out", "C1 port Rp for default USB power"), p(10, "RSV1", "no_connect", "Reserved"),
    p(11, "C1CM", "input", "C1 port CC1 voltage detect"), p(12, "C1TD", "output", "C1 port CC BMC transport"),
    p(13, "C0H1", "input", "C0 port Rp for 1.5 A"), p(14, "RSV2", "no_connect", "Reserved"),
    p(15, "VBEN", "output", "VBUS enable"), p(16, "RSV3", "no_connect", "Reserved"),
    p(17, "VDD", "power_in", "3.3 V power input"), p(18, "RSV4", "no_connect", "Reserved"),
    p(19, "PTSR", "no_connect", "Reserved"), p(20, "PRST", "no_connect", "Reserved"),
    p(21, "C01D", "output", "C0 port Rd"), p(22, "C0H3", "output", "C0 port Rp for 3.0 A"),
    p(23, "TA", "bidirectional", "Firmware upgrade interface A"),
    p(24, "TB", "bidirectional", "Firmware upgrade interface B"),
    p(25, "PDEN", "output", "C0 port PD engine enable"), p(26, "CTIN", "input", "External clock input"),
    p(27, "C01TD", "output", "C0 port CC1 BMC transport"),
    p(28, "C12TD", "output", "C1 port CC2 BMC transport"),
    p(29, "C11TD", "output", "C1 port CC2 BMC transport"), p(30, "HPD", "input", "DP hot-plug detect"),
    p(31, "PDBT", "input", "System reference; connect 100 kOhm to GND"),
    p(32, "CTCO", "output", "System clock output"), p(33, "EP", "power_in", "Exposed pad ground"),
]

MP2315 = [
    p(1, "AAM", "input", "AAM threshold programming resistor"), p(2, "IN", "power_in"),
    p(3, "SW", "power_out"), p(4, "GND", "power_in"), p(5, "BST", "passive"),
    p(6, "EN/SYNC", "input"), p(7, "VCC", "power_out", "Internal regulator output; decouple to GND"),
    p(8, "FB", "input"),
]

MP2482 = [
    p(1, "SW", "power_out"), p(2, "BST", "passive"),
    p(3, "VCC", "power_out", "Bias supply; decouple to GND"), p(4, "GND", "power_in"),
    p(5, "FB", "input"), p(6, "PG", "open_collector", "Open-drain power-good output"),
    p(7, "EN/SYNC", "input"), p(8, "IN", "power_in"),
]

HDMI_A = [
    p(1, "D2+", "passive", "TMDS Data2+"),
    p(2, "D2_SHIELD", "passive", "TMDS Data2 shield"),
    p(3, "D2-", "passive", "TMDS Data2-"),
    p(4, "D1+", "passive", "TMDS Data1+"),
    p(5, "D1_SHIELD", "passive", "TMDS Data1 shield"),
    p(6, "D1-", "passive", "TMDS Data1-"),
    p(7, "D0+", "passive", "TMDS Data0+"),
    p(8, "D0_SHIELD", "passive", "TMDS Data0 shield"),
    p(9, "D0-", "passive", "TMDS Data0-"),
    p(10, "CLK+", "passive", "TMDS Clock+"),
    p(11, "CLK_SHIELD", "passive", "TMDS Clock shield"),
    p(12, "CLK-", "passive", "TMDS Clock-"),
    p(13, "CEC", "passive", "Consumer Electronics Control"),
    p(14, "UTILITY/HEAC+", "passive", "Reserved in HDMI 1.0-1.3a; Utility/HEAC+ in HDMI 1.4+"),
    p(15, "DDC_SCL", "passive", "DDC I2C clock"),
    p(16, "DDC_SDA", "passive", "DDC I2C data"),
    p(17, "DDC_CEC_GND", "passive", "DDC/CEC signal ground"),
    p(18, "+5V", "passive", "HDMI source-supplied +5 V; passive connector contact"),
    p(19, "HPD/HEAC-", "passive", "Hot Plug Detect / HEAC-"),
    p(20, "SHELL", "passive", "Local library convention: four mounting tabs share pad 20; NOT HDMI contact 20"),
]

HDMI_NAME = "HDMI_A_19P_Reused"
HDMI_FOOTPRINT = "HDMI_A_19P_Reused_WFD6_Compatible"
HDMI_DATASHEET = "https://www.fit-foxconn.com/w73/fit/image/Products/Drawing/Drawing/QJ51191-WFD6-4F_301-0000-2217_A.pdf"

# Component-side coordinates, connector opening toward negative Y.  The Foxconn
# recommended-layout view has its opening toward the bottom; rotate that view
# 180 degrees (not a mirror), so contact 1 is at X=-4.5 mm here.
HDMI_GEOMETRY = {
    "pitch": 0.5, "signal_y": 0.76, "signal_size": (0.28, 2.6),
    "shell_x": 7.25, "rear_y": 0.0, "front_y": -5.96,
    "rear_land": (1.5, 3.3), "rear_drill": (0.9, 2.7),
    "front_land": (1.5, 2.7), "front_drill": (0.9, 2.1),
    "body": (-7.5, -10.14, 7.5, 2.06),
    "pcb_edge_y": -10.09,
}


SYMBOLS = [
    {"name": "MS2130", "value": "MS2130", "footprint": "QFN-64-1EP_9x9mm_P0.5mm_EP5.7x5.7mm_MS2130", "datasheet": "https://datasheet.lcsc.com/datasheet/pdf/5e5b81537e2ff3b4c2bdcbedb88db519.pdf?productCode=C5905137", "description": "USB 3.0 HDMI video capture controller, QFN-64-EP", "pins": MS2130, "quad": 16},
    {"name": "GL3523_OTYx0", "value": "GL3523-OTYx0", "footprint": "QFN-76-1EP_9x9mm_P0.4mm_EP5.6x5.6mm_GL3523", "datasheet": "https://www.genesyslogic.com.tw/upload/prodAttach/GL3523%20Datasheet%20Rev.1.70.pdf", "description": "USB 3.1 Gen 1 hub controller, OTY10/OTY30 QFN-76 variant", "pins": GL3523, "quad": 19},
    {"name": "TPS2553_DBV", "value": "TPS2553DBV", "footprint": "SOT-23-6_DBV", "datasheet": "https://www.ti.com/lit/ds/symlink/tps2553.pdf", "description": "Adjustable current-limited power-distribution switch, DBV SOT-23-6", "pins": TPS_DBV},
    {"name": "TPS2553_DRV", "value": "TPS2553DRV", "footprint": "WSON-6-1EP_2x2mm_P0.65mm_DRV", "datasheet": "https://www.ti.com/lit/ds/symlink/tps2553.pdf", "description": "Adjustable current-limited power-distribution switch, DRV WSON-6 PowerPAD", "pins": TPS_DRV},
    {"name": "LDR6023Q", "value": "LDR6023Q", "footprint": "QFN-32-1EP_5x5mm_P0.5mm_EP3.4x3.4mm_LDR6023Q", "datasheet": "https://e2e.ti.com/cfs-file/__key/communityserver-discussions-components-files/196/LDR6023S_5F00_Legendary_5F00_IC.pdf", "description": "USB Type-C PD controller, original LDR6023 QFN-32 variant", "pins": LDR6023Q, "quad": 8},
    {"name": "MP2315GJ", "value": "MP2315GJ", "footprint": "TSOT-23-8_MP2315", "datasheet": "https://www.monolithicpower.com/pub/media/document/MP2315_r1.01.pdf", "description": "3 A 24 V 500 kHz synchronous buck converter, TSOT23-8 (NRFND)", "pins": MP2315},
    {"name": "MP2482DN", "value": "MP2482DN", "footprint": "SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.62x3.51mm_MP2482DN", "datasheet": "https://www.monolithicpower.com/en/documentview/productdocument/index/version/2/document_type/Datasheet/lang/EN/sku/MP2482", "description": "5 A 30 V 420 kHz buck converter, SOIC-8 exposed-pad DN variant (NRFND)", "pins": MP2482 + [p(9, "EP", "power_in", "Exposed pad; connect to GND")]},
    {"name": "MP2482DS", "value": "MP2482DS", "footprint": "SOIC-8_3.9x4.9mm_P1.27mm_MP2482DS", "datasheet": "https://www.monolithicpower.com/en/documentview/productdocument/index/version/2/document_type/Datasheet/lang/EN/sku/MP2482", "description": "5 A 30 V 420 kHz buck converter, SOIC-8 DS variant without exposed pad (legacy)", "pins": MP2482},
    {"name": HDMI_NAME, "value": "HDMI_A_19P_Reused", "reference": "J", "footprint": HDMI_FOOTPRINT, "datasheet": HDMI_DATASHEET, "description": "Existing unbranded HDMI-A 19-contact receptacle; WFD6 compatible reference land pattern; verify physical fit before manufacture", "pins": HDMI_A, "connector": True},
]


def esc(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')


def prop(name: str, value: str, x: float, y: float, hidden: bool = False) -> str:
    hide = " hide" if hidden else ""
    return f'    (property "{name}" "{esc(value)}" (at {x:g} {y:g} 0) (effects (font (size 1.27 1.27)){hide}))'


def pin_sexpr(pin: dict[str, str], x: float, y: float, angle: int) -> str:
    return (
        f'      (pin {pin["type"]} line (at {x:g} {y:g} {angle}) (length 2.54)\n'
        f'        (name "{esc(pin["name"])}" (effects (font (size 1.0 1.0))))\n'
        f'        (number "{pin["number"]}" (effects (font (size 1.0 1.0)))))'
    )


def make_symbol(s: dict) -> str:
    pins = s["pins"]
    quad = s.get("quad")
    body = []
    pin_text = []
    if s.get("connector"):
        # Signals are passive: this receptacle may be wired as either source or sink.
        x0, x1, y0, y1 = -12.7, 12.7, -30.48, 30.48
        body.append(f'      (rectangle (start {x0:g} {y0:g}) (end {x1:g} {y1:g}) (stroke (width 0.254) (type default)) (fill (type background)))')
        for idx, pin in enumerate(pins[:19]):
            pin_text.append(pin_sexpr(pin, x1 + 2.54, 25.4 - idx * 2.54, 180))
        pin_text.append(pin_sexpr(pins[19], 0, y0 - 2.54, 90))
        ref_y, val_y = y1 + 5.08, y1 + 2.54
    elif quad:
        n = quad
        width = max(25.4, (n - 1) * 2.54 + 5.08)
        # Reserve one extra grid row for the exposed-pad pin so it never
        # overlays the last perimeter pin on the left side.
        height = width + 5.08
        x0, x1, y0, y1 = -width / 2, width / 2, -height / 2, height / 2
        body.append(f'      (rectangle (start {x0:g} {y0:g}) (end {x1:g} {y1:g}) (stroke (width 0) (type default)) (fill (type background)))')
        perimeter = pins[: 4 * n]
        for idx, pin in enumerate(perimeter):
            side, pos = divmod(idx, n)
            delta = (pos - (n - 1) / 2) * 2.54
            if side == 0:
                pin_text.append(pin_sexpr(pin, x0 - 2.54, delta, 0))
            elif side == 1:
                pin_text.append(pin_sexpr(pin, delta, y1 + 2.54, 270))
            elif side == 2:
                pin_text.append(pin_sexpr(pin, x1 + 2.54, -delta, 180))
            else:
                pin_text.append(pin_sexpr(pin, -delta, y0 - 2.54, 90))
        for extra_i, pin in enumerate(pins[4 * n :]):
            pin_text.append(pin_sexpr(pin, x0 - 2.54, y1 - 2.54 * (extra_i + 1), 0))
        ref_y = y0 - 5.08
        val_y = y1 + 5.08
    else:
        left = pins[: math.ceil(len(pins) / 2)]
        right = pins[len(left) :]
        rows = max(len(left), len(right))
        width = 15.24
        height = max(10.16, (rows - 1) * 2.54 + 5.08)
        x0, x1, y0, y1 = -width / 2, width / 2, -height / 2, height / 2
        body.append(f'      (rectangle (start {x0:g} {y0:g}) (end {x1:g} {y1:g}) (stroke (width 0) (type default)) (fill (type background)))')
        for idx, pin in enumerate(left):
            y = (idx - (len(left) - 1) / 2) * 2.54
            pin_text.append(pin_sexpr(pin, x0 - 2.54, y, 0))
        for idx, pin in enumerate(right):
            y = ((len(right) - 1) / 2 - idx) * 2.54
            pin_text.append(pin_sexpr(pin, x1 + 2.54, y, 180))
        ref_y = y0 - 2.54
        val_y = y1 + 2.54
    return "\n".join([
        f'  (symbol "{s["name"]}"',
        '    (pin_names (offset 1.016))',
        '    (exclude_from_sim no)',
        '    (in_bom yes)',
        '    (on_board yes)',
        prop("Reference", s.get("reference", "U"), 0, ref_y),
        prop("Value", s["value"], 0, val_y),
        prop("Footprint", "Future_ICS:" + s["footprint"], 0, 0, True),
        prop("Datasheet", s["datasheet"], 0, 0, True),
        prop("Description", s["description"], 0, 0, True),
        f'    (symbol "{s["name"]}_0_1"', *body, '    )',
        f'    (symbol "{s["name"]}_1_1"', *pin_text, '    )',
        '  )',
    ])


def write_symbols() -> None:
    text = ['(kicad_symbol_lib (version 20231120) (generator kicad_symbol_editor)']
    text.extend(make_symbol(s) for s in SYMBOLS)
    text.append(')')
    (SYM_DIR / "Future_ICS.kicad_sym").write_text("\n".join(text) + "\n", encoding="utf-8")


def fp_header(name: str, description: str) -> list[str]:
    return [
        f'(footprint "{name}"',
        '  (version 20240108)',
        '  (generator pcbnew)',
        '  (layer "F.Cu")',
        f'  (descr "{description}")',
        '  (tags "QFN WSON SMD")',
        '  (attr smd)',
        '  (fp_text reference "REF**" (at 0 -6 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))',
        '  (fp_text value "VAL**" (at 0 6 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))',
    ]


def outline(lines: list[str], body_x: float, body_y: float, courtyard_x: float, courtyard_y: float) -> None:
    bx, by = body_x / 2, body_y / 2
    cx, cy = courtyard_x / 2, courtyard_y / 2
    lines += [
        f'  (fp_rect (start {-bx:g} {-by:g}) (end {bx:g} {by:g}) (stroke (width 0.1) (type default)) (fill none) (layer "F.Fab"))',
        f'  (fp_rect (start {-cx:g} {-cy:g}) (end {cx:g} {cy:g}) (stroke (width 0.05) (type default)) (fill none) (layer "F.CrtYd"))',
        # Perimeter leads reach across the package edge on all four sides.  A full
        # silkscreen box would cross solder-mask openings, so use a pin-1 marker.
        f'  (fp_circle (center {-bx - 0.6:g} {by - 0.3:g}) (end {-bx - 0.4:g} {by - 0.3:g}) (stroke (width 0.12) (type default)) (fill none) (layer "F.SilkS"))',
    ]


def pad(number: str | int, x: float, y: float, sx: float, sy: float, rr: float = 0.2, layers: str = '"F.Cu" "F.Paste" "F.Mask"') -> str:
    shape = "rect" if rr == 0 else "roundrect"
    ratio = f' (roundrect_rratio {rr:g})' if rr else ""
    return f'  (pad "{number}" smd {shape} (at {x:g} {y:g}) (size {sx:g} {sy:g}) (layers {layers}){ratio})'


def write_qfn(name: str, count: int, body: float, pitch: float, ep: float, lead_w: float, lead_l: float, center: float, description: str) -> None:
    per = count // 4
    start = -(per - 1) * pitch / 2
    lines = fp_header(name, description)
    outline(lines, body, body, body + 1.8, body + 1.8)
    for i in range(per):
        d = start + i * pitch
        lines.append(pad(1 + i, d, center, lead_w, lead_l))
        lines.append(pad(1 + per + i, center, -d, lead_l, lead_w))
        lines.append(pad(1 + 2 * per + i, -d, -center, lead_w, lead_l))
        lines.append(pad(1 + 3 * per + i, -center, d, lead_l, lead_w))
    ep_num = count + 1
    lines.append(pad(ep_num, 0, 0, ep, ep, 0, '"F.Cu" "F.Mask"'))
    windows = 3 if ep >= 5 else 2
    gap = 0.3
    aperture = (ep - gap * (windows - 1)) / windows
    for iy in range(windows):
        for ix in range(windows):
            x = (ix - (windows - 1) / 2) * (aperture + gap)
            y = (iy - (windows - 1) / 2) * (aperture + gap)
            lines.append(pad("", x, y, aperture, aperture, 0.08, '"F.Paste"'))
    lines.append(')')
    (FP_DIR / f"{name}.kicad_mod").write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_wson() -> None:
    name = "WSON-6-1EP_2x2mm_P0.65mm_DRV"
    lines = fp_header(name, "TI DRV0006A WSON-6 PowerPAD, official land pattern")
    outline(lines, 2, 2, 2.5, 2.5)
    ys = [-0.65, 0, 0.65]
    for i, y in enumerate(ys, 1):
        lines.append(pad(i, -0.975, y, 0.45, 0.3))
    for i, y in zip([6, 5, 4], ys):
        lines.append(pad(i, 0.975, y, 0.45, 0.3))
    lines.append(pad(7, 0, 0, 1.0, 1.6, 0.08, '"F.Cu" "F.Mask"'))
    lines.append(pad("", 0, -0.575, 1.0, 0.45, 0.08, '"F.Paste"'))
    lines.append(pad("", 0, 0.575, 1.0, 0.45, 0.08, '"F.Paste"'))
    lines.append(')')
    (FP_DIR / f"{name}.kicad_mod").write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_gullwing(name: str, body_x: float, body_y: float, count: int, pitch: float, row_x: float, pad_x: float, pad_y: float, ep: tuple[float, float] | None, description: str) -> None:
    lines = fp_header(name, description)
    outline(lines, body_x, body_y, row_x * 2 + pad_x + 0.5, body_y + 1.0)
    per = count // 2
    ys = [(i - (per - 1) / 2) * pitch for i in range(per)]
    for i, y in enumerate(reversed(ys), 1):
        lines.append(pad(i, -row_x, y, pad_x, pad_y))
    for i, y in enumerate(ys, per + 1):
        lines.append(pad(i, row_x, y, pad_x, pad_y))
    if ep:
        ex, ey = ep
        lines.append(pad(count + 1, 0, 0, ex, ey, 0, '"F.Cu" "F.Mask"'))
        for x in (-ex / 4, ex / 4):
            for y in (-ey / 4, ey / 4):
                lines.append(pad("", x, y, ex / 2 - 0.2, ey / 2 - 0.2, 0.08, '"F.Paste"'))
    lines.append(')')
    (FP_DIR / f"{name}.kicad_mod").write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_footprints() -> None:
    write_qfn("QFN-64-1EP_9x9mm_P0.5mm_EP5.7x5.7mm_MS2130", 64, 9, 0.5, 5.7, 0.28, 0.665, 4.407, "MS2130 QFN-64-EP; supplier land pattern, 9x9 mm, 0.5 mm pitch")
    write_qfn("QFN-76-1EP_9x9mm_P0.4mm_EP5.6x5.6mm_GL3523", 76, 9, 0.4, 5.6, 0.2, 0.8, 4.4, "GL3523 OTY10/OTY30 QFN-76-EP; supplier land pattern, 9x9 mm, 0.4 mm pitch")
    write_qfn("QFN-32-1EP_5x5mm_P0.5mm_EP3.4x3.4mm_LDR6023Q", 32, 5, 0.5, 3.4, 0.25, 0.7, 2.35, "LDR6023Q QFN-32-EP; 5x5 mm, 0.5 mm pitch, 3.4 mm exposed pad")
    write_gullwing("SOT-23-6_DBV", 1.6, 2.9, 6, 0.95, 1.3, 1.1, 0.6, None, "TI DBV0006A SOT-23-6 official 1.1x0.6 mm land pattern")
    write_wson()
    write_gullwing("TSOT-23-8_MP2315", 1.6, 2.9, 8, 0.65, 1.3, 1.2, 0.4, None, "MPS MP2315 TSOT23-8, official 1.20x0.40 mm land pattern")
    write_gullwing("SOIC-8_3.9x4.9mm_P1.27mm_MP2482DS", 3.9, 4.9, 8, 1.27, 2.7, 1.6, 0.61, None, "MPS MP2482DS SOIC-8, official 1.60x0.61 mm land pattern")
    write_gullwing("SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.62x3.51mm_MP2482DN", 3.9, 4.9, 8, 1.27, 2.7, 1.6, 0.61, (2.62, 3.51), "MPS MP2482DN SOIC-8E, official exposed-pad land pattern")
    write_hdmi()


def hdmi_pads() -> list[dict]:
    g = HDMI_GEOMETRY
    result = [dict(number=str(i), x=-4.5 + (i - 1) * g["pitch"],
                   y=g["signal_y"], size=g["signal_size"], drill=None)
              for i in range(1, 20)]
    for row in ("rear", "front"):
        for side in (-1, 1):
            result.append(dict(number="20", x=side * g["shell_x"],
                               y=g[f"{row}_y"], size=g[f"{row}_land"],
                               drill=g[f"{row}_drill"]))
    return result


def write_hdmi() -> None:
    g = HDMI_GEOMETRY
    lines = [
        f'(footprint "{HDMI_FOOTPRINT}"',
        '  (version 20240108) (generator pcbnew) (layer "F.Cu")',
        '  (descr "Reused unbranded HDMI-A receptacle; reference fit based on Foxconn DWG 301-0000-2217 rev A. 0.5mm pitch, shell centers 14.5x5.96mm. All four shell pads=20. Physical fit not yet verified.")',
        '  (tags "HDMI Type-A 19P reused compatible WFD6 hybrid")',
        '  (attr smd)',
        '  (solder_mask_margin 0.04)',
        '  (fp_text reference "REF**" (at 0 -12.4 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))',
        f'  (fp_text value "{HDMI_NAME}" (at 0 3.4 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))',
        '  (fp_rect (start -7.5 -10.14) (end 7.5 2.06) (stroke (width 0.1) (type default)) (fill none) (layer "F.Fab"))',
        '  (fp_rect (start -8.5 -11.15) (end 8.5 2.6) (stroke (width 0.05) (type default)) (fill none) (layer "F.CrtYd"))',
        # Guidance only: never put the suggested PCB edge on Edge.Cuts.
        f'  (fp_line (start -9 {g["pcb_edge_y"]:g}) (end 9 {g["pcb_edge_y"]:g}) (stroke (width 0.1) (type dash)) (layer "Dwgs.User"))',
        '  (fp_text user "PCB EDGE (REF)" (at 0 -10.7 0) (layer "Dwgs.User") (effects (font (size 0.6 0.6) (thickness 0.1))))',
        '  (fp_text user "MATING / CABLE" (at 0 -9.2 0) (layer "F.Fab") (effects (font (size 0.7 0.7) (thickness 0.1))))',
        '  (fp_line (start -6 -10.35) (end 6 -10.35) (stroke (width 0.12) (type default)) (layer "F.SilkS"))',
        '  (fp_line (start -7.65 -8.1) (end -7.65 -4.1) (stroke (width 0.12) (type default)) (layer "F.SilkS"))',
        '  (fp_line (start 7.65 -8.1) (end 7.65 -4.1) (stroke (width 0.12) (type default)) (layer "F.SilkS"))',
        '  (fp_circle (center -5.2 1.0) (end -5.0 1.0) (stroke (width 0.12) (type default)) (fill none) (layer "F.SilkS"))',
        '  (fp_text user "1" (at -4.5 2.3 0) (layer "F.SilkS") (effects (font (size 0.7 0.7) (thickness 0.1))))',
    ]
    for item in hdmi_pads():
        sx, sy = item["size"]
        if item["drill"] is None:
            lines.append(pad(item["number"], item["x"], item["y"], sx, sy, 0))
        else:
            dx, dy = item["drill"]
            lines.append(f'  (pad "20" thru_hole oval (at {item["x"]:g} {item["y"]:g}) (size {sx:g} {sy:g}) (drill oval {dx:g} {dy:g}) (layers "*.Cu" "*.Mask"))')
    lines.append(')')
    (FP_DIR / f"{HDMI_FOOTPRINT}.kicad_mod").write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_csvs() -> None:
    for symbol in SYMBOLS:
        with (PIN_DIR / f'{symbol["name"]}.csv').open("w", newline="", encoding="utf-8-sig") as f:
            writer = csv.DictWriter(f, fieldnames=["pin", "name", "electrical_type", "description"])
            writer.writeheader()
            for pin in symbol["pins"]:
                writer.writerow({"pin": pin["number"], "name": pin["name"], "electrical_type": pin["type"], "description": pin["description"]})


def write_tables() -> None:
    (ROOT / "sym-lib-table").write_text('(sym_lib_table\n  (lib (name "Future_ICS")(type "KiCad")(uri "${KIPRJMOD}/symbols/Future_ICS.kicad_sym")(options "")(descr "Future IC symbols"))\n)\n', encoding="utf-8")
    (ROOT / "fp-lib-table").write_text('(fp_lib_table\n  (lib (name "Future_ICS")(type "KiCad")(uri "${KIPRJMOD}/footprints/Future_ICS.pretty")(options "")(descr "Future IC footprints"))\n)\n', encoding="utf-8")


def main() -> None:
    for directory in (SYM_DIR, FP_DIR, PIN_DIR):
        directory.mkdir(parents=True, exist_ok=True)
    write_symbols()
    write_footprints()
    write_csvs()
    write_tables()
    print(f"Generated {len(SYMBOLS)} symbols, {len(SYMBOLS)} footprints, and {len(SYMBOLS)} pinout CSV files in {ROOT}")


if __name__ == "__main__":
    main()

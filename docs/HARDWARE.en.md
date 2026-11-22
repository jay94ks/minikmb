# Hardware

[한국어](HARDWARE.md) · [English](HARDWARE.en.md)

## STM32H7 board

Open `hardware/stm32h7/MiniKMB.kicad_pro` in KiCad 10. The schematic embeds its symbols; CH482X/GL3510 footprint and symbol tables use project-relative paths. The used standard footprint subset is also included locally; optional standard 3D models use the KiCad installation paths.

| Connector | Role |
|---|---|
| J9 | Management PC USB, upstream of GL3510 hub |
| J1 / J3 / J7 / J5 | Channel 0: HDMI input / target USB / microSD / SWD |
| J2 / J4 / J8 / J6 | Channel 1: HDMI input / target USB / microSD / SWD |

Fixed hub mapping: downstream P1 → MS2130 channel 0; P2 → MS2130 channel 1; P3 → channel 0 MCU management FS; P4 → channel 1 MCU management FS. The app groups siblings under the same physical hub and queries the MCU UID. It does not pair devices by global enumeration order or use a UID to discover arbitrary ports.

Manufactured revision: `20261104-via030`, nominal 80 × 54 mm, six copper layers, 1.6 mm board, JLC06161H-3313 stackup, 1 oz outer / 0.5 oz inner copper, ENIG. Through vias use 0.30 mm drill / 0.45 mm copper pad; the order used the 0.3 mm/(0.4/0.45 mm) option and filled/capped vias. Confirm current fabrication options with the manufacturer before reordering.

Historical impedance targets: USB differential 90 Ω, HDMI differential 100 Ω, ULPI/SDMMC single-ended 50 Ω, ±10% target. Design targets and saved calculations are not laboratory measurements of the assembled board.

`hardware/stm32h7/manufacturing/` preserves the actual historical Gerber archive and partial-assembly BOM/CPL. HDMI J1/J2 were manually assembled. R39/R41/U7/U8/U11 were DNP in that order. The MS2130 SPI flashes were subsequently fitted/programmed to obtain video; do not copy that DNP choice to a new working build without considering this change.

## Target USB-C and HS design review

**The schematic was authored directly by the creator, jay94ks.** The designer attributes unreliable HS establishment or high error rates to the omitted target USB-C plug-orientation detection/signal-selection circuit and the resulting impedance mismatch in this board's HS path. The current firmware therefore uses FS.

The designer's retrospective view is that an orientation-aware signal-selection IC would have allowed the HS path to be arranged and its impedance requirements to be met. Proceeding to PCB layout without sufficiently reviewing this portion is recorded as a design-review omission, not GPT incorrectly authoring the schematic.

This is the designer's assessment, distinct from a root cause confirmed by signal-integrity measurements or a circuit before/after comparison. Orientation here refers to the USB-C plug, not ULPI DIR. See [HS limitations and assessment scope](KNOWN-ISSUES.en.md#target-usb-hs-and-design-retrospective).

## Bring-up rework

A 1 MΩ parallel bias resistor was added across each USB3300 crystal Y1/Y2 on the working boards. This improved observed crystal start-up behavior, but it does not establish the root cause of the remaining High Speed transmit stalls. The archived schematic/Gerbers remain the manufactured revision; this rework is documented rather than silently changing the source of the original batch.

The original MS2130-only board in `hardware/prototypes/boards/ms2130-test` is an unfinished placement experiment, not a fabrication release. GL3523/LDR6023/other candidate libraries in `hardware/prototypes/` are historical design studies, not the current GL3510 BOM.

## RP2040 prototype

See `hardware/rp2040/README.md` for the hand-wired three-MCU arrangement and the prototype/dual-PC photographs. It used separate commercial capture devices; it is not the STM32H7 PCB.

# Hardware

## STM32H7 board

Open `hardware/stm32h7/MiniKMB.kicad_pro` in KiCad 10. The schematic embeds its symbols; CH482X/GL3510 footprint and symbol tables use project-relative paths. Standard footprints use the installed KiCad libraries.

| Connector | Role |
|---|---|
| J9 | Management PC USB, upstream of GL3510 hub |
| J1 / J3 / J7 / J5 | Channel 0: HDMI input / target USB / microSD / SWD |
| J2 / J4 / J8 / J6 | Channel 1: HDMI input / target USB / microSD / SWD |

Fixed hub mapping: downstream P1 → MS2130 channel 0; P2 → MS2130 channel 1; P3 → channel 0 MCU management FS; P4 → channel 1 MCU management FS. The app groups siblings under the same physical hub and queries the MCU UID. It does not pair devices by global enumeration order or use a UID to discover arbitrary ports.

Manufactured revision: `20261104-via030`, nominal 80 × 54 mm, six copper layers, 1.6 mm board, JLC06161H-3313 stackup, 1 oz outer / 0.5 oz inner copper, ENIG. Through vias use 0.30 mm drill / 0.45 mm copper pad; the order used the 0.3 mm/(0.4/0.45 mm) option and filled/capped vias. Confirm current fabrication options with the manufacturer before reordering.

Historical impedance targets: USB differential 90 Ω, HDMI differential 100 Ω, ULPI/SDMMC single-ended 50 Ω, ±10% target. Design targets and saved calculations are not laboratory measurements of the assembled board.

`hardware/stm32h7/manufacturing/` preserves the actual historical Gerber archive and partial-assembly BOM/CPL. HDMI J1/J2 were manually assembled. R39/R41/U7/U8/U11 were DNP in that order. The MS2130 SPI flashes were subsequently fitted/programmed to obtain video; do not copy that DNP choice to a new working build without considering this change.

## Bring-up rework

A 1 MΩ parallel bias resistor was added across each USB3300 crystal Y1/Y2 on the working boards. This improved observed crystal start-up behavior, but it does not establish the root cause of the remaining High Speed transmit stalls. The archived schematic/Gerbers remain the manufactured revision; this rework is documented rather than silently changing the source of the original batch.

The original MS2130-only board in `hardware/prototypes/boards/ms2130-test` is an unfinished placement experiment, not a fabrication release. GL3523/LDR6023/other candidate libraries in `hardware/prototypes/` are historical design studies, not the current GL3510 BOM.

## RP2040 prototype

See `hardware/rp2040/README.md` for the hand-wired three-MCU arrangement and the prototype/dual-PC photographs. It used separate commercial capture devices; it is not the STM32H7 PCB.

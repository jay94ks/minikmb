# MiniKMB STM32H743 firmware

STM32H743VIH6, dual USB Device stacks (management FS and target through USB3300), FreeRTOS, FatFs and SDMMC. This source corresponds to the FS-HID r63 line: separate bounded keyboard and mouse queues, CDC management protocol, target HID/CDC/MSC/NCM and SD image storage.

Import this directory as an existing STM32CubeIDE project, select Debug and build. The `.ioc`, linker scripts, HAL/CMSIS, middleware subset, TinyUSB namespace wrappers and upstream patches are included. Do not let code generation replace the two USB stack owners with simultaneous HAL PCD ownership.

A portable build plan and `tools/build.py` also build the same source list/flags using GNU Arm Embedded 13.3.rel1 without requiring a local CubeIDE workspace path. See repository `docs/BUILDING.md`. Outputs are separate from the archived release binaries.

Protocol and configuration references:

- `docs/USB-FS-PROTOCOL.md`
- `docs/MEMORY-MAP.md` / `memory-map-v1.json`
- `docs/MIDDLEWARE-CONFIG.md`
- `docs/WRITABLE-DISK.md`
- `docs/USB3300-REGISTERS.md`

The Full Speed release is the supported bring-up path. High Speed transmit stalls remain unresolved; this is distinct from working MS2130 video. No automatic hardware flashing occurs when building or running the normal host tests.

MiniKMB code is MIT; HAL/CMSIS/FreeRTOS/FatFs/TinyUSB retain their original licenses and local-patch notices.

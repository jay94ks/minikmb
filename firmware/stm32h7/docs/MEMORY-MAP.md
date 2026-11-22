# STM32H743 memory map

The FLASH linker script is authoritative. This table is derived from the current r63 source; allocated region size is not the actual amount consumed by every buffer.

| Region | Start | End inclusive | Bytes |
|---|---|---|---|
| FLASH_APP | 0x08000000 | 0x081FFFFF | 2097152 |
| ITCM_CODE | 0x00000000 | 0x0000FFFF | 65536 |
| DTCM_FAST | 0x20000000 | 0x20017FFF | 98304 |
| DTCM_GUARD | 0x20018000 | 0x2001BFFF | 16384 |
| DTCM_MSP | 0x2001C000 | 0x2001FFFF | 16384 |
| AXI_EXTENTS | 0x24000000 | 0x2400FFFF | 65536 |
| AXI_FILES | 0x24010000 | 0x2401FFFF | 65536 |
| AXI_RESERVE | 0x24020000 | 0x2405FFFF | 262144 |
| USB_FS_DMA | 0x24060000 | 0x24063FFF | 16384 |
| USB_HS_DMA | 0x24064000 | 0x2406BFFF | 32768 |
| SD_DMA | 0x2406C000 | 0x24073FFF | 32768 |
| NCM_DMA | 0x24074000 | 0x2407BFFF | 32768 |
| DMA_RESERVE | 0x2407C000 | 0x2407FFFF | 16384 |
| D2_PROTOCOL | 0x30000000 | 0x3001FFFF | 131072 |
| D2_HEAP | 0x30020000 | 0x30021FFF | 8192 |
| D2_NETWORK | 0x30022000 | 0x3002FFFF | 57344 |
| D2_STACKS | 0x30030000 | 0x3003FFFF | 65536 |
| D2_LOG | 0x30040000 | 0x30047FFF | 32768 |
| D3_CRASH | 0x38000000 | 0x38000FFF | 4096 |
| D3_RESERVE | 0x38001000 | 0x3800FFFF | 61440 |
| BKPSRAM | 0x38800000 | 0x38800FFF | 4096 |

MSP starts at the top of DTCM_MSP (0x20020000). Linker ASSERTs enforce dedicated section bounds. Startup initializes the required SRAM domains before task/DMA use; noinit crash data is a deliberate exception. USB/SD buffers have explicitly reserved AXI addresses and MPU/cache rules. Kernel heap is8KiB; application buffers and task storage are statically allocated.

See [linker](../STM32H743VIHX_FLASH.ld), [startup](../Core/Startup/startup_stm32h743vihx.s), [main/MPU initialization](../Core/Src/main.c), [JSON data](memory-map-v1.json) and [middleware settings](MIDDLEWARE-CONFIG.md). The RAM-execution linker is a separate profile; this table describes FLASH execution.

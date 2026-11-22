/*
******************************************************************************
**
**  File        : LinkerScript.ld
**
**  Author      : STM32CubeIDE
**
**  Abstract    : Linker script for STM32H7 series
**                2048Kbytes FLASH and 1056Kbytes RAM
**
**                Set heap size, stack size and stack location according
**                to application requirements.
**
**                Set memory bank area and size if external memory is used.
**
**  Target      : STMicroelectronics STM32
**
**  Distribution: The file is distributed as is, without any warranty
**                of any kind.
**
*****************************************************************************
** @attention
**
** Copyright (c) 2025 STMicroelectronics.
** All rights reserved.
**
** This software is licensed under terms that can be found in the LICENSE file
** in the root directory of this software component.
** If no LICENSE file comes with this software, it is provided AS-IS.
**
****************************************************************************
*/

/* Entry Point */
ENTRY(Reset_Handler)

/* MiniKMB memory map v1. See docs/MEMORY-MAP.md. */
MEMORY
{
  FLASH_APP       (rx) : ORIGIN = 0x08000000, LENGTH = 2048K
  ITCM_CODE       (rx) : ORIGIN = 0x00000000, LENGTH = 64K
  DTCM_FAST       (rw) : ORIGIN = 0x20000000, LENGTH = 96K
  DTCM_GUARD      (rw) : ORIGIN = 0x20018000, LENGTH = 16K
  DTCM_MSP        (rw) : ORIGIN = 0x2001C000, LENGTH = 16K
  AXI_EXTENTS     (rw) : ORIGIN = 0x24000000, LENGTH = 64K
  AXI_FILES       (rw) : ORIGIN = 0x24010000, LENGTH = 64K
  AXI_RESERVE     (rw) : ORIGIN = 0x24020000, LENGTH = 256K
  USB_FS_DMA      (rw) : ORIGIN = 0x24060000, LENGTH = 16K
  USB_HS_DMA      (rw) : ORIGIN = 0x24064000, LENGTH = 32K
  SD_DMA          (rw) : ORIGIN = 0x2406C000, LENGTH = 32K
  NCM_DMA         (rw) : ORIGIN = 0x24074000, LENGTH = 32K
  DMA_RESERVE     (rw) : ORIGIN = 0x2407C000, LENGTH = 16K
  D2_PROTOCOL     (rw) : ORIGIN = 0x30000000, LENGTH = 128K
  D2_HEAP         (rw) : ORIGIN = 0x30020000, LENGTH = 8K
  D2_NETWORK      (rw) : ORIGIN = 0x30022000, LENGTH = 56K
  D2_STACKS       (rw) : ORIGIN = 0x30030000, LENGTH = 64K
  D2_LOG          (rw) : ORIGIN = 0x30040000, LENGTH = 32K
  D3_CRASH        (rw) : ORIGIN = 0x38000000, LENGTH = 4K
  D3_RESERVE      (rw) : ORIGIN = 0x38001000, LENGTH = 60K
  BKPSRAM         (rw) : ORIGIN = 0x38800000, LENGTH = 4K
}

_estack = ORIGIN(DTCM_MSP) + LENGTH(DTCM_MSP);
_Min_Heap_Size = 0;
_Min_Stack_Size = LENGTH(DTCM_MSP);
__axi_start__ = ORIGIN(AXI_EXTENTS);
__axi_end__ = ORIGIN(DMA_RESERVE) + LENGTH(DMA_RESERVE);
__d2_start__ = ORIGIN(D2_PROTOCOL);
__d2_end__ = ORIGIN(D2_LOG) + LENGTH(D2_LOG);
__sd_dma_start__ = ORIGIN(SD_DMA);
__sd_dma_end__ = ORIGIN(SD_DMA) + LENGTH(SD_DMA);

/* Define output sections */
SECTIONS
{
  /* The startup code goes first into FLASH */
  .isr_vector :
  {
    . = ALIGN(4);
    KEEP(*(.isr_vector)) /* Startup code */
    . = ALIGN(4);
  } >FLASH_APP

  /* The program code and other data goes into FLASH */
  .text :
  {
    . = ALIGN(4);
    *(.text)           /* .text sections (code) */
    *(.text*)          /* .text* sections (code) */
    *(.glue_7)         /* glue arm to thumb code */
    *(.glue_7t)        /* glue thumb to arm code */
    *(.eh_frame)

    KEEP (*(.init))
    KEEP (*(.fini))

    . = ALIGN(4);
    _etext = .;        /* define a global symbols at end of code */
  } >FLASH_APP

  /* Constant data goes into FLASH */
  .rodata :
  {
    . = ALIGN(4);
    *(.rodata)         /* .rodata sections (constants, strings, etc.) */
    *(.rodata*)        /* .rodata* sections (constants, strings, etc.) */
    . = ALIGN(4);
  } >FLASH_APP

  .ARM.extab (READONLY) : /* The READONLY keyword is only supported in GCC11 and later, remove it if using GCC10 or earlier. */
  {
    *(.ARM.extab* .gnu.linkonce.armextab.*)
  } >FLASH_APP
  .ARM (READONLY) : /* The READONLY keyword is only supported in GCC11 and later, remove it if using GCC10 or earlier. */
  {
    __exidx_start = .;
    *(.ARM.exidx*)
    __exidx_end = .;
  } >FLASH_APP

  .preinit_array (READONLY) : /* The READONLY keyword is only supported in GCC11 and later, remove it if using GCC10 or earlier. */
  {
    PROVIDE_HIDDEN (__preinit_array_start = .);
    KEEP (*(.preinit_array*))
    PROVIDE_HIDDEN (__preinit_array_end = .);
  } >FLASH_APP

  .init_array (READONLY) : /* The READONLY keyword is only supported in GCC11 and later, remove it if using GCC10 or earlier. */
  {
    PROVIDE_HIDDEN (__init_array_start = .);
    KEEP (*(SORT(.init_array.*)))
    KEEP (*(.init_array*))
    PROVIDE_HIDDEN (__init_array_end = .);
  } >FLASH_APP

  .fini_array (READONLY) : /* The READONLY keyword is only supported in GCC11 and later, remove it if using GCC10 or earlier. */
  {
    PROVIDE_HIDDEN (__fini_array_start = .);
    KEEP (*(SORT(.fini_array.*)))
    KEEP (*(.fini_array*))
    PROVIDE_HIDDEN (__fini_array_end = .);
  } >FLASH_APP

  /* used by the startup to initialize data */
  _sidata = LOADADDR(.data);

  /* Initialized data sections goes into RAM, load LMA copy after code */
  .data :
  {
    . = ALIGN(4);
    _sdata = .;        /* create a global symbol at data start */
    *(.data)           /* .data sections */
    *(.data*)          /* .data* sections */

    . = ALIGN(4);
    _edata = .;        /* define a global symbol at data end */
  } >DTCM_FAST AT> FLASH_APP

  /* Uninitialized data section */
  . = ALIGN(4);
  .bss :
  {
    /* This is used by the startup in order to initialize the .bss section */
    _sbss = .;         /* define a global symbol at bss start */
    __bss_start__ = _sbss;
    *(.bss)
    *(.bss*)
    *(COMMON)

    . = ALIGN(4);
    _ebss = .;         /* define a global symbol at bss end */
    __bss_end__ = _ebss;
  } >DTCM_FAST

  .axi_extents (NOLOAD) : ALIGN(32)
  {
    *(.axi_extents .axi_extents.*)
    . = ALIGN(32);
  } >AXI_EXTENTS
  ASSERT(SIZEOF(.axi_extents) <= LENGTH(AXI_EXTENTS), "axi_extents exceeds its region")

  .axi_files (NOLOAD) : ALIGN(32)
  {
    *(.axi_files .axi_files.*)
    . = ALIGN(32);
  } >AXI_FILES
  ASSERT(SIZEOF(.axi_files) <= LENGTH(AXI_FILES), "axi_files exceeds its region")

  .axi_card_data (NOLOAD) : ALIGN(32)
  {
    *(.axi_card_data)
  } >AXI_RESERVE
  ASSERT(SIZEOF(.axi_card_data) <= 8K, "card user data exceeds its area")

  .axi_raw_usb (NOLOAD) : ALIGN(32)
  {
    *(.axi_raw_usb)
  } >AXI_RESERVE
  ASSERT(SIZEOF(.axi_raw_usb) <= 8K, "raw usb staging exceeds its area")

  .axi_optical (NOLOAD) : ALIGN(32)
  {
    *(.axi_optical)
  } >AXI_RESERVE
  ASSERT(SIZEOF(.axi_optical) <= 8K, "optical read staging exceeds its area")

  .axi_upload (NOLOAD) : ALIGN(32)
  {
    *(.axi_upload)
  } >AXI_RESERVE
  ASSERT(SIZEOF(.axi_upload) <= 64K, "upload staging exceeds its area")

  .axi_serial (NOLOAD) : ALIGN(32)
  {
    *(.axi_serial)
  } >AXI_RESERVE
  ASSERT(SIZEOF(.axi_serial) <= 32K, "serial queues exceed their area")

  .usb_fs_dma (NOLOAD) : ALIGN(32)
  {
    *(.usb_fs_dma .usb_fs_dma.*)
    . = ALIGN(32);
  } >USB_FS_DMA
  ASSERT(SIZEOF(.usb_fs_dma) <= LENGTH(USB_FS_DMA), "usb_fs_dma exceeds its region")

  .usb_hs_dma (NOLOAD) : ALIGN(32)
  {
    *(.usb_hs_dma .usb_hs_dma.*)
    . = ALIGN(32);
  } >USB_HS_DMA
  ASSERT(SIZEOF(.usb_hs_dma) <= LENGTH(USB_HS_DMA), "usb_hs_dma exceeds its region")

  .sd_dma (NOLOAD) : ALIGN(32)
  {
    *(.sd_dma .sd_dma.*)
    . = ALIGN(32);
  } >SD_DMA
  ASSERT(SIZEOF(.sd_dma) <= LENGTH(SD_DMA), "sd_dma exceeds its region")

  .ncm_dma (NOLOAD) : ALIGN(32)
  {
    *(.ncm_dma .ncm_dma.*)
    . = ALIGN(32);
  } >NCM_DMA
  ASSERT(SIZEOF(.ncm_dma) <= LENGTH(NCM_DMA), "ncm_dma exceeds its region")

  .protocol_rx ORIGIN(D2_PROTOCOL) (NOLOAD) : { *(.protocol.rx) } >D2_PROTOCOL
  .protocol_tx ORIGIN(D2_PROTOCOL) + 0x2000 (NOLOAD) : { *(.protocol.tx) } >D2_PROTOCOL
  .protocol_replay ORIGIN(D2_PROTOCOL) + 0x4000 (NOLOAD) : { *(.protocol.replay) } >D2_PROTOCOL
  .protocol_network ORIGIN(D2_PROTOCOL) + 0x14000 (NOLOAD) : { *(.protocol.network) } >D2_PROTOCOL
  ASSERT(SIZEOF(.protocol_network) <= 24K, "Ethernet queues exceed their area")
  .protocol_storage ORIGIN(D2_PROTOCOL) + 0x1E000 (NOLOAD) : { *(.protocol.storage) } >D2_PROTOCOL
  ASSERT(SIZEOF(.protocol_storage) <= 8K, "storage staging exceeds its area")
  ASSERT(SIZEOF(.protocol_rx) <= 8K, "protocol RX exceeds its area")
  ASSERT(SIZEOF(.protocol_tx) <= 8K, "protocol TX exceeds its area")
  ASSERT(SIZEOF(.protocol_replay) <= 48K, "protocol replay exceeds its area")

  .rtos_heap (NOLOAD) : ALIGN(32)
  {
    *(.rtos_heap .rtos_heap.*)
    . = ALIGN(32);
  } >D2_HEAP
  ASSERT(SIZEOF(.rtos_heap) <= LENGTH(D2_HEAP), "rtos_heap exceeds its region")

  .d2_network (NOLOAD) : ALIGN(32)
  {
    *(.d2_network)
  } >D2_NETWORK
  ASSERT(SIZEOF(.d2_network) <= LENGTH(D2_NETWORK), "network queues exceed their area")

  .rtos_stacks (NOLOAD) : ALIGN(32)
  {
    *(.rtos_stacks .rtos_stacks.*)
    . = ALIGN(32);
  } >D2_STACKS
  ASSERT(SIZEOF(.rtos_stacks) <= LENGTH(D2_STACKS), "rtos_stacks exceeds its region")

  .debug_log (NOLOAD) : ALIGN(32)
  {
    *(.debug_log .debug_log.*)
    . = ALIGN(32);
  } >D2_LOG
  ASSERT(SIZEOF(.debug_log) <= LENGTH(D2_LOG), "debug_log exceeds its region")

  .crash_noinit (NOLOAD) : ALIGN(32)
  {
    *(.crash_noinit .crash_noinit.*)
    . = ALIGN(32);
  } >D3_CRASH
  ASSERT(SIZEOF(.crash_noinit) <= LENGTH(D3_CRASH), "crash_noinit exceeds its region")

  .ramfunc_disabled : { *(.RamFunc .RamFunc.* .itcm_code .itcm_code.*) } >FLASH_APP
  ASSERT(SIZEOF(.ramfunc_disabled) == 0, "Executable RAM sections require a copy and MPU policy")
  ASSERT(_ebss <= ORIGIN(DTCM_GUARD), "DTCM data exceeds CPU data region")
  ASSERT(LOADADDR(.data) >= ORIGIN(FLASH_APP) &&
         LOADADDR(.data) + SIZEOF(.data) <= ORIGIN(FLASH_APP) + LENGTH(FLASH_APP),
         "DTCM initial data must be stored in flash")
  ASSERT(SIZEOF(.rtos_heap) == LENGTH(D2_HEAP), "Unexpected FreeRTOS heap size")
  PROVIDE(end = _ebss);
  PROVIDE(_end = _ebss);

  /* Remove information from the standard libraries */
  /DISCARD/ :
  {
    libc.a ( * )
    libm.a ( * )
    libgcc.a ( * )
  }

  .ARM.attributes 0 : { *(.ARM.attributes) }
}



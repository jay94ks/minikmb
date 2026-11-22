# TinyUSB for MiniKMB

Origin https://github.com/hathach/tinyusb

Tag 0.21.0, commit dae3f9a366bfcddbf9dcf1b48d7500286a849539.
Files imported from the device core, CDC ACM, HID, MSC, CDC-NCM, FreeRTOS OSAL and DWC2 port.
The bundled LICENSE covers the imported files.

The patch in `patches/0001-stm32h7-minimum-in-fifo.patch` applies to the
upstream repository root. It enforces the RM0433 minimum IN FIFO depth
on both STM32H7 controllers.

Patch 0002 honors an explicit Full Speed request while keeping DWC2's selected HS PHY.

Patch 0003 extends the target's MSC BOT handling for optical READ(12), full READ range validation, device VPD and mode pages, and BOT reset of pending reads.

Patch 0004 isolates SCSI sense data per LUN, validates a complete WRITE(10)
before receiving data, and adds task-side async I/O tickets so a BOT or USB
reset cannot deliver an old read or write result to a later command.

Patch 0005 separates NCM USB buffers, exposes the NCM data alternate setting,
allows an effective link speed, and validates NTB16 table and datagram ranges.
Patch 0006 advertises 4-byte NTB divisors for Windows, zeroes datagram padding,
and uses the actual USB port for NCM data IN.

Patch 0007 accepts padded MSC CDB lengths in 1..16 bytes, including the
12-byte READ(10) used by SeaBIOS. READ/WRITE direction and range checks remain.

Patch 0008 applies STM32H743 ES0392 §2.25.7 to IN ZLP scheduling on OTG_HS.

Patch 0009 prevents a full target-device event queue from breaking into
HardFault and affecting the management controller. It stops the target IRQ
and lets the target task retire the queue and reconnect the external PHY.
The management TinyUSB instance retains upstream queue behavior.

Patch 0010 stops an armed STM32H743 target bulk IN transfer when the host clears HALT, then clears the FIFO and resets the data toggle. This discards the obsolete response during MSC BOT reset recovery.

Patch 0011 reports the actual DWC2 bulk IN cancellation to the target core. Only an aborted transfer releases its software claim; un-aborted HID and OUT transfers retain ownership. The management core and upstream void DCD API are unchanged.

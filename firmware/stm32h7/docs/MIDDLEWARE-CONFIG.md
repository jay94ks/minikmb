# Middleware configuration

The archived FS-HID r63 source is the configuration authority. The Debug/default profile uses target Full Speed, HID/CDC/MSC/NCM and management FAST_CDC; MINIKMB_RELEASE_PROFILE/HIGH_SPEED are explicit alternate build options, not proof of physical HS reliability.

- TinyUSB exclusively owns each USB controller. Management and target have separate namespace wrappers and OSAL ownership; do not enable HAL PCD on the same core concurrently.
- Management CDC uses static TX/RX buffers and finite queues. A fast management endpoint staging buffer does not change the physical Full Speed max packet size.
- The normal DWC2 path is PIO. Buffer lifetime, endpoint completion and control-state transitions are separate from application queue acceptance.
- FreeRTOS static allocation is enabled. Application tasks and packet/image/network storage use explicit static buffers. Dynamic allocation remains enabled only for bounded RTOS/kernel requirements; configTOTAL_HEAP_SIZE is 8192 bytes, not a general-purpose application buffer pool.
- Keyboard and mouse each reserve16 pending slots and progress independently. Down/up order is retained within each interface; completion is matched by ticket.
- SD/FatFs operations run in the storage worker rather than in USB IRQ processing. Image jobs, raw leases and optical serving coordinate ownership. BOARD.ID is internal metadata and is loaded/created on card initialization, not exposed as an ordinary image.
- USB/SD buffers use explicitly placed AXI regions. Startup initialization, MPU/cache settings and linker boundaries must agree; DTCM is not assumed DMA-accessible.
- FatFs includes the project exFAT directory fix; original permission/copyright text is retained. Use the in-tree source and patch rather than replacing it blindly with a newly generated CubeMX copy.

See [target_features.h](../Core/Inc/target_features.h), [tusb_config.h](../Core/Inc/tusb_config.h), [FreeRTOSConfig.h](../Core/Inc/FreeRTOSConfig.h), [memory map](MEMORY-MAP.md) and [wire protocol](USB-FS-PROTOCOL.md). Host regression tests and builds are documented in the repository docs; physical HS limitations are separate.

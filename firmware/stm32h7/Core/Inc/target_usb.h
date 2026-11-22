#ifndef MINIKMB_TARGET_USB_H
#define MINIKMB_TARGET_USB_H

#include "management_protocol.h"

#include "target_features.h"

const ManagementSerialBackend *TargetSerial_Backend(void);
void UsbBoard_InitClock(void);
void TargetUsb_Start(void);

/* USB FS management status values: 0 unconfigured, 1 FS, 2 HS, 3 suspended. */
uint8_t TargetUsb_State(void);

#endif

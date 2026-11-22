#ifndef MINIKMB_TARGET_HID_USB_H
#define MINIKMB_TARGET_HID_USB_H

#include "management_protocol.h"

enum { TARGET_HID_KEYBOARD_INSTANCE = 0, TARGET_HID_MOUSE_INSTANCE = 1 };

/* Initialize before the scheduler starts. Poll runs in the target USB task. */
void TargetHidUsb_Init(void);
const ManagementHidBackend *TargetHidUsb_Backend(void);
void TargetHidUsb_Poll(uint32_t now);

#endif

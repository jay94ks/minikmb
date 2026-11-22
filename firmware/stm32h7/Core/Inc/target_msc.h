#ifndef MINIKMB_TARGET_MSC_H
#define MINIKMB_TARGET_MSC_H

#include <stdbool.h>
#include <stdint.h>

/* The endpoint buffer and completion belong to the target TinyUSB task. */
void *TargetMsc_ReadBuffer(void);
bool TargetMsc_OpticalVisible(void);
void TargetMsc_SetOpticalVisible(bool enabled);
void TargetMsc_Poll(bool configured);

#endif

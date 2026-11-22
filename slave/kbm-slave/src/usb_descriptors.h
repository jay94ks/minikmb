#ifndef KBM_SLAVE_USB_DESCRIPTORS_H
#define KBM_SLAVE_USB_DESCRIPTORS_H
#include <stdbool.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
// Core 1 only, while the USB device is disconnected.
enum { KBM_USB_NCM = 1, KBM_USB_DISK = 2 };
void kbm_usb_set_function_mask(uint8_t mask);
uint8_t kbm_usb_function_mask(void);
#ifdef __cplusplus
}
#endif

enum {
    KBM_REPORT_ID_KEYBOARD = 1,
    KBM_REPORT_ID_MOUSE = 2,
    KBM_REPORT_ID_SYSTEM = 3,
};

#endif


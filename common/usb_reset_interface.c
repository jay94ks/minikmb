/*
 * RP2040 standard USB reset interface, adapted from the Pico SDK's
 * pico_stdio_usb/reset_interface.c (BSD-3-Clause).
 */
#include "pico/bootrom.h"
#include "pico/usb_reset_interface.h"
#include "hardware/watchdog.h"
#include "device/usbd_pvt.h"
#include "tusb.h"

#ifndef KBM_RESET_INTERFACE_NUMBER
#error KBM_RESET_INTERFACE_NUMBER must match the reset interface descriptor
#endif

#define RESET_MS_OS_20_DESC_LEN 166
#define RESET_BOS_TOTAL_LEN (TUD_BOS_DESC_LEN + TUD_BOS_MICROSOFT_OS_DESC_LEN)

static uint8_t reset_interface_number;

static const uint8_t reset_bos_descriptor[] = {
    TUD_BOS_DESCRIPTOR(RESET_BOS_TOTAL_LEN, 1),
    TUD_BOS_MS_OS_20_DESCRIPTOR(RESET_MS_OS_20_DESC_LEN, 1),
};

static const uint8_t reset_ms_os_20_descriptor[] = {
    U16_TO_U8S_LE(0x000a), U16_TO_U8S_LE(MS_OS_20_SET_HEADER_DESCRIPTOR),
    U32_TO_U8S_LE(0x06030000), U16_TO_U8S_LE(RESET_MS_OS_20_DESC_LEN),
    U16_TO_U8S_LE(0x0008), U16_TO_U8S_LE(MS_OS_20_SUBSET_HEADER_FUNCTION),
    KBM_RESET_INTERFACE_NUMBER, 0, U16_TO_U8S_LE(0x009c),
    U16_TO_U8S_LE(0x0014), U16_TO_U8S_LE(MS_OS_20_FEATURE_COMPATBLE_ID),
    'W', 'I', 'N', 'U', 'S', 'B', 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0,
    U16_TO_U8S_LE(0x0080), U16_TO_U8S_LE(MS_OS_20_FEATURE_REG_PROPERTY),
    U16_TO_U8S_LE(0x0001), U16_TO_U8S_LE(0x0028),
    'D', 0, 'e', 0, 'v', 0, 'i', 0, 'c', 0, 'e', 0, 'I', 0, 'n', 0,
    't', 0, 'e', 0, 'r', 0, 'f', 0, 'a', 0, 'c', 0, 'e', 0, 'G', 0,
    'U', 0, 'I', 0, 'D', 0, 0, 0,
    U16_TO_U8S_LE(0x004e),
    '{', 0, 'b', 0, 'c', 0, '7', 0, '3', 0, '9', 0, '8', 0, 'c', 0,
    '1', 0, '-', 0, '7', 0, '3', 0, 'c', 0, 'd', 0, '-', 0, '4', 0,
    'c', 0, 'b', 0, '7', 0, '-', 0, '9', 0, '8', 0, 'b', 0, '8', 0,
    '-', 0, '9', 0, '1', 0, '3', 0, 'a', 0, '8', 0, 'f', 0, 'c', 0,
    'a', 0, '7', 0, 'b', 0, 'f', 0, '6', 0, '}', 0, 0, 0,
};

TU_VERIFY_STATIC(sizeof(reset_bos_descriptor) == RESET_BOS_TOTAL_LEN,
                 "Incorrect reset BOS size");
TU_VERIFY_STATIC(sizeof(reset_ms_os_20_descriptor) == RESET_MS_OS_20_DESC_LEN,
                 "Incorrect reset MS OS 2.0 size");

const uint8_t *tud_descriptor_bos_cb(void) {
    return reset_bos_descriptor;
}

bool tud_vendor_control_xfer_cb(uint8_t rhport, uint8_t stage,
                                const tusb_control_request_t *request) {
    if (stage != CONTROL_STAGE_SETUP) return true;
    if (request->bRequest == 1 && request->wIndex == 7) {
        return tud_control_xfer(rhport, request,
                                (void *)(uintptr_t)reset_ms_os_20_descriptor,
                                sizeof(reset_ms_os_20_descriptor));
    }
    return false;
}

static void resetd_init(void) {}

static void resetd_reset(uint8_t rhport) {
    (void)rhport;
    reset_interface_number = 0;
}

static uint16_t resetd_open(uint8_t rhport,
                            const tusb_desc_interface_t *descriptor,
                            uint16_t max_length) {
    (void)rhport;
    TU_VERIFY(descriptor->bInterfaceClass == TUSB_CLASS_VENDOR_SPECIFIC &&
              descriptor->bInterfaceSubClass == RESET_INTERFACE_SUBCLASS &&
              descriptor->bInterfaceProtocol == RESET_INTERFACE_PROTOCOL, 0);
    TU_VERIFY(max_length >= sizeof(tusb_desc_interface_t), 0);
    reset_interface_number = descriptor->bInterfaceNumber;
    return sizeof(tusb_desc_interface_t);
}

static bool resetd_control(uint8_t rhport, uint8_t stage,
                           const tusb_control_request_t *request) {
    (void)rhport;
    if (stage != CONTROL_STAGE_SETUP) return true;
    if (request->wIndex != reset_interface_number) return false;
    if (request->bRequest == RESET_REQUEST_BOOTSEL) {
        rom_reset_usb_boot_extra(-1, request->wValue & 0x7fu, false);
    }
    if (request->bRequest == RESET_REQUEST_FLASH) {
        watchdog_reboot(0, 0, 100);
        return true;
    }
    return false;
}

static bool resetd_transfer(uint8_t rhport, uint8_t endpoint,
                            xfer_result_t result, uint32_t transferred) {
    (void)rhport;
    (void)endpoint;
    (void)result;
    (void)transferred;
    return true;
}

static const usbd_class_driver_t reset_driver = {
#if CFG_TUSB_DEBUG >= 2
    .name = "RESET",
#endif
    .init = resetd_init,
    .reset = resetd_reset,
    .open = resetd_open,
    .control_xfer_cb = resetd_control,
    .xfer_cb = resetd_transfer,
    .sof = NULL,
};

const usbd_class_driver_t *usbd_app_driver_get_cb(uint8_t *driver_count) {
    *driver_count = 1;
    return &reset_driver;
}

#if KBM_RESET_VIA_BAUD_RATE
void tud_cdc_line_coding_cb(uint8_t interface,
                            const cdc_line_coding_t *coding) {
    (void)interface;
    if (coding->bit_rate == 1200) {
        rom_reset_usb_boot_extra(-1, 0, false);
    }
}
#endif

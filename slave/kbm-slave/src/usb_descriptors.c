#include "tusb_config.h"
#include "usb_descriptors.h"

#include "pico/unique_id.h"
#include "tusb.h"

#include <stddef.h>
#include <string.h>

#ifndef KBM_USB_VID
#define KBM_USB_VID 0x8857
#endif
#ifndef KBM_USB_PID
#define KBM_USB_PID 0x1222
#endif

static uint8_t function_mask; // Both optional functions are hidden at boot.
void kbm_usb_set_function_mask(uint8_t mask) { function_mask = mask & 3u; }
uint8_t kbm_usb_function_mask(void) { return function_mask; }
static tusb_desc_device_t device_descriptor = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0210,
    .bDeviceClass = TUSB_CLASS_MISC,
    .bDeviceSubClass = MISC_SUBCLASS_COMMON,
    .bDeviceProtocol = MISC_PROTOCOL_IAD,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,
    .idVendor = KBM_USB_VID,
    .idProduct = KBM_USB_PID,
    .bcdDevice = 0x0300,
    .iManufacturer = 0x01,
    .iProduct = 0x02,
    .iSerialNumber = 0x03,
    .bNumConfigurations = 0x01,
};

uint8_t const *tud_descriptor_device_cb(void) {
    device_descriptor.bcdDevice = 0x0800 | function_mask;
    return (uint8_t const *)&device_descriptor;
}

static const uint8_t hid_report_descriptor[] = {
    TUD_HID_REPORT_DESC_KEYBOARD(HID_REPORT_ID(KBM_REPORT_ID_KEYBOARD)),
    TUD_HID_REPORT_DESC_ABSMOUSE(HID_REPORT_ID(KBM_REPORT_ID_MOUSE)),
    // Separate System Control collection:40 explicitly listed input bits.
    HID_USAGE_PAGE(HID_USAGE_PAGE_DESKTOP),
    HID_USAGE(HID_USAGE_DESKTOP_SYSTEM_CONTROL),
    HID_COLLECTION(HID_COLLECTION_APPLICATION),
      HID_REPORT_ID(KBM_REPORT_ID_SYSTEM)
      HID_LOGICAL_MIN(0), HID_LOGICAL_MAX(1),
#define KBM_SYSTEM_USAGE(value) HID_USAGE(value),
#include "system_controls.def"
#undef KBM_SYSTEM_USAGE
      HID_REPORT_COUNT(40), HID_REPORT_SIZE(1),
      HID_INPUT(HID_DATA | HID_VARIABLE | HID_ABSOLUTE),
    HID_COLLECTION_END,
};

uint8_t const *tud_hid_descriptor_report_cb(uint8_t instance) {
    (void)instance;
    return hid_report_descriptor;
}

enum {
    ITF_NUM_HID = 0,
    ITF_NUM_CDC,
    ITF_NUM_CDC_DATA,
    ITF_NUM_RPI_RESET,
    ITF_NUM_MSC,
    ITF_NUM_NCM,
    ITF_NUM_NCM_DATA,
    ITF_NUM_TOTAL,
};

#define TUD_RPI_RESET_DESC_LEN 9
#define TUD_RPI_RESET_DESCRIPTOR(_itf, _str) \
    9, TUSB_DESC_INTERFACE, _itf, 0, 0, TUSB_CLASS_VENDOR_SPECIFIC, 0, 1, _str
#define CONFIG_BASE_LEN \
    (TUD_CONFIG_DESC_LEN + TUD_HID_DESC_LEN + TUD_CDC_DESC_LEN + TUD_RPI_RESET_DESC_LEN)
#define EPNUM_HID 0x81
#define BASE_INTERFACES \
    TUD_HID_DESCRIPTOR(ITF_NUM_HID, 0, HID_ITF_PROTOCOL_NONE, \
                       sizeof(hid_report_descriptor), EPNUM_HID, CFG_TUD_HID_EP_BUFSIZE, 1), \
    TUD_CDC_DESCRIPTOR(ITF_NUM_CDC, 5, 0x82, 8, 0x03, 0x83, CFG_TUD_CDC_EP_BUFSIZE), \
    TUD_RPI_RESET_DESCRIPTOR(ITF_NUM_RPI_RESET, 4)

static const uint8_t configuration_both[] = {
    TUD_CONFIG_DESCRIPTOR(1, ITF_NUM_TOTAL, 0, CONFIG_BASE_LEN + TUD_MSC_DESC_LEN + TUD_CDC_NCM_DESC_LEN,
                          TUSB_DESC_CONFIG_ATT_REMOTE_WAKEUP, 100),
    BASE_INTERFACES,
    TUD_MSC_DESCRIPTOR(ITF_NUM_MSC, 8, 0x06, 0x86, 64),
    TUD_CDC_NCM_DESCRIPTOR(ITF_NUM_NCM, 6, 7, 0x84, 16, 0x05, 0x85, 64, 1514),
};
static const uint8_t configuration_disk[] = {
    TUD_CONFIG_DESCRIPTOR(1, 5, 0, CONFIG_BASE_LEN + TUD_MSC_DESC_LEN,
                          TUSB_DESC_CONFIG_ATT_REMOTE_WAKEUP, 100),
    BASE_INTERFACES,
    TUD_MSC_DESCRIPTOR(ITF_NUM_MSC, 8, 0x06, 0x86, 64),
};
static const uint8_t configuration_ncm[] = {
    TUD_CONFIG_DESCRIPTOR(1, 6, 0, CONFIG_BASE_LEN + TUD_CDC_NCM_DESC_LEN,
                          TUSB_DESC_CONFIG_ATT_REMOTE_WAKEUP, 100),
    BASE_INTERFACES,
    // Keep interface numbers contiguous when MSC is absent.
    TUD_CDC_NCM_DESCRIPTOR(4, 6, 7, 0x84, 16, 0x05, 0x85, 64, 1514),
};
static const uint8_t configuration_base[] = {
    TUD_CONFIG_DESCRIPTOR(1, 4, 0, CONFIG_BASE_LEN,
                          TUSB_DESC_CONFIG_ATT_REMOTE_WAKEUP, 100),
    BASE_INTERFACES,
};

uint8_t const *tud_descriptor_configuration_cb(uint8_t index) {
    (void)index;
    switch (function_mask) {
        case KBM_USB_NCM: return configuration_ncm;
        case KBM_USB_DISK: return configuration_disk;
        case KBM_USB_NCM | KBM_USB_DISK: return configuration_both;
        default: return configuration_base;
    }
}

enum {
    STRING_LANGID = 0,
    STRING_MANUFACTURER,
    STRING_PRODUCT,
    STRING_SERIAL,
    STRING_RESET,
};

static const char *const string_descriptors[] = {
    NULL,
    "KBM",
    "KBM Slave Keyboard Mouse",
    NULL,
    "Reset",
    "KBM Slave Serial Relay",
    "KBM Low Speed Ethernet",
    NULL,
    "KBM Read Only Folder Disk",
};

static uint16_t descriptor_string[33];

uint16_t const *tud_descriptor_string_cb(uint8_t index, uint16_t langid) {
    (void)langid;
    size_t count = 0;

    if (index == STRING_LANGID) {
        descriptor_string[1] = 0x0409;
        count = 1;
    } else if (index == 7) {
        extern uint8_t tud_network_mac_address[6];
        const char hex[] = "0123456789ABCDEF";
        count = 12;
        for (size_t i = 0; i < 6; ++i) {
            descriptor_string[1 + i * 2] = hex[tud_network_mac_address[i] >> 4];
            descriptor_string[2 + i * 2] = hex[tud_network_mac_address[i] & 15];
        }
    } else if (index == STRING_SERIAL) {
        char serial[PICO_UNIQUE_BOARD_ID_SIZE_BYTES * 2u + 1u];
        pico_get_unique_board_id_string(serial, sizeof(serial));
        count = strlen(serial);
        if (count > 32) {
            count = 32;
        }
        for (size_t i = 0; i < count; ++i) {
            descriptor_string[i + 1] = (uint8_t)serial[i];
        }
    } else {
        if (index >= sizeof(string_descriptors) / sizeof(string_descriptors[0]) ||
            string_descriptors[index] == NULL) {
            return NULL;
        }
        const char *text = string_descriptors[index];
        count = strlen(text);
        if (count > 32) {
            count = 32;
        }
        for (size_t i = 0; i < count; ++i) {
            descriptor_string[i + 1] = (uint8_t)text[i];
        }
    }

    descriptor_string[0] =
        (uint16_t)((TUSB_DESC_STRING << 8u) | (2u * count + 2u));
    return descriptor_string;
}


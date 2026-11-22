#include "tusb_config.h"
#include "native_network.h"

#include "pico/unique_id.h"
#include "tusb.h"

#include <stddef.h>
#include <string.h>

#ifndef KBM_USB_VID
#define KBM_USB_VID 0x8857
#endif
#ifndef KBM_USB_PID
#define KBM_USB_PID 0x1223
#endif

static bool ncm_visible;
void kbm_usb_set_ncm_visible(bool visible) { ncm_visible = visible; }
bool kbm_usb_ncm_visible(void) { return ncm_visible; }
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
    .bcdDevice = 0x0100,
    .iManufacturer = 0x01,
    .iProduct = 0x02,
    .iSerialNumber = 0x03,
    .bNumConfigurations = 0x01,
};

uint8_t const *tud_descriptor_device_cb(void) {
    device_descriptor.bcdDevice = ncm_visible ? 0x0201 : 0x0200;
    return (uint8_t const *)&device_descriptor;
}

enum {
    ITF_NUM_CDC = 0,
    ITF_NUM_CDC_DATA,
    ITF_NUM_RPI_RESET,
    ITF_NUM_NCM,
    ITF_NUM_NCM_DATA,
    ITF_NUM_TOTAL,
};

#define TUD_RPI_RESET_DESC_LEN 9
#define TUD_RPI_RESET_DESCRIPTOR(_itf, _str) \
    9, TUSB_DESC_INTERFACE, _itf, 0, 0, TUSB_CLASS_VENDOR_SPECIFIC, 0, 1, _str
#define CONFIG_TOTAL_LEN \
    (TUD_CONFIG_DESC_LEN + TUD_CDC_DESC_LEN + TUD_RPI_RESET_DESC_LEN)
#define EPNUM_CDC_NOTIF 0x81
#define EPNUM_CDC_OUT 0x02
#define EPNUM_CDC_IN 0x82

static const uint8_t configuration_descriptor[] = {
    TUD_CONFIG_DESCRIPTOR(1, ITF_NUM_NCM, 0, CONFIG_TOTAL_LEN, 0, 100),
    TUD_CDC_DESCRIPTOR(ITF_NUM_CDC, 0, EPNUM_CDC_NOTIF, 8,
                       EPNUM_CDC_OUT, EPNUM_CDC_IN,
                       CFG_TUD_CDC_EP_BUFSIZE),
    TUD_RPI_RESET_DESCRIPTOR(ITF_NUM_RPI_RESET, 4),
};
static const uint8_t configuration_with_ncm[] = {
    TUD_CONFIG_DESCRIPTOR(1, ITF_NUM_TOTAL, 0, CONFIG_TOTAL_LEN + TUD_CDC_NCM_DESC_LEN, 0, 100),
    TUD_CDC_DESCRIPTOR(ITF_NUM_CDC, 0, EPNUM_CDC_NOTIF, 8,
                       EPNUM_CDC_OUT, EPNUM_CDC_IN, CFG_TUD_CDC_EP_BUFSIZE),
    TUD_RPI_RESET_DESCRIPTOR(ITF_NUM_RPI_RESET, 4),
    TUD_CDC_NCM_DESCRIPTOR(ITF_NUM_NCM, 5, 6, 0x83, 16, 0x04, 0x84, 64, 1514),
};

uint8_t const *tud_descriptor_configuration_cb(uint8_t index) {
    (void)index;
    return ncm_visible ? configuration_with_ncm : configuration_descriptor;
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
    "KBM Master CDC Bridge",
    NULL,
    "Reset",
    "KBM Native Ethernet",
    NULL,
};

static uint16_t descriptor_string[33];

uint16_t const *tud_descriptor_string_cb(uint8_t index, uint16_t langid) {
    (void)langid;
    size_t count = 0;
    if (index == STRING_LANGID) {
        descriptor_string[1] = 0x0409;
        count = 1;
    } else if (index == 6) {
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
        if (count > 32) count = 32;
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
        if (count > 32) count = 32;
        for (size_t i = 0; i < count; ++i) {
            descriptor_string[i + 1] = (uint8_t)text[i];
        }
    }
    descriptor_string[0] =
        (uint16_t)((TUSB_DESC_STRING << 8u) | (2u * count + 2u));
    return descriptor_string;
}


#include "target_tusb_namespace.h"
#include "tusb.h"
#include "stm32h7xx.h"
#include "target_usb.h"
#include "management_protocol.h"
#if MINIKMB_TARGET_HID
#include "target_hid_usb.h"
#endif

#ifndef MINIKMB_HS_VID
#define MINIKMB_HS_VID 0xCAFE
#endif
#ifndef MINIKMB_HS_PID
#if MINIKMB_TARGET_NCM
#define MINIKMB_HS_PID (0x4020 + (MINIKMB_TARGET_HID ? 2 : 0) + (MINIKMB_TARGET_MSC ? 1 : 0))
#elif MINIKMB_TARGET_MSC
#define MINIKMB_HS_PID (MINIKMB_TARGET_HID ? 0x4016 : 0x4015)
#elif MINIKMB_TARGET_HID
#define MINIKMB_HS_PID 0x4012
#else
#define MINIKMB_HS_PID 0x4011
#endif
#endif

static const tusb_desc_device_t device = {
  .bLength = sizeof(tusb_desc_device_t),
  .bDescriptorType = TUSB_DESC_DEVICE,
  .bcdUSB = 0x0200,
  .bDeviceClass = TUSB_CLASS_MISC,
  .bDeviceSubClass = MISC_SUBCLASS_COMMON,
  .bDeviceProtocol = MISC_PROTOCOL_IAD,
  .bMaxPacketSize0 = 64,
  .idVendor = MINIKMB_HS_VID,
  .idProduct = MINIKMB_HS_PID,
  .bcdDevice = MINIKMB_TARGET_HIGH_SPEED ? 0x0100 : 0x0101,
  .iManufacturer = 1,
  .iProduct = 2,
  .iSerialNumber = 3,
  .bNumConfigurations = 1,
};

#if MINIKMB_TARGET_HIGH_SPEED
static const tusb_desc_device_qualifier_t qualifier = {
  .bLength = sizeof(tusb_desc_device_qualifier_t),
  .bDescriptorType = TUSB_DESC_DEVICE_QUALIFIER,
  .bcdUSB = 0x0200,
  .bDeviceClass = TUSB_CLASS_MISC,
  .bDeviceSubClass = MISC_SUBCLASS_COMMON,
  .bDeviceProtocol = MISC_PROTOCOL_IAD,
  .bMaxPacketSize0 = 64,
  .bNumConfigurations = 1,
  .bReserved = 0,
};
#endif

#if MINIKMB_TARGET_HID
static const uint8_t keyboard_report[] = {TUD_HID_REPORT_DESC_KEYBOARD()};
static const uint8_t mouse_report[] = {TUD_HID_REPORT_DESC_ABSMOUSE()};

enum { OPTICAL_INTERFACE = 4, HID_DESC_SIZE = 2 * TUD_HID_DESC_LEN };
#define TARGET_HID_INTERFACES(interval) \
  , TUD_HID_DESCRIPTOR(2, 0, HID_ITF_PROTOCOL_KEYBOARD, sizeof(keyboard_report), 0x83, 8, interval) \
  , TUD_HID_DESCRIPTOR(3, 0, HID_ITF_PROTOCOL_NONE, sizeof(mouse_report), 0x84, 8, interval)

const uint8_t *tud_hid_descriptor_report_cb(uint8_t instance)
{
  if (instance == TARGET_HID_KEYBOARD_INSTANCE) return keyboard_report;
  if (instance == TARGET_HID_MOUSE_INSTANCE) return mouse_report;
  return NULL;
}
#else
enum { OPTICAL_INTERFACE = 2, HID_DESC_SIZE = 0 };
#define TARGET_HID_INTERFACES(interval)
#endif

#if MINIKMB_TARGET_MSC
#define TARGET_MSC_INTERFACE(packet_size) , TUD_MSC_DESCRIPTOR(OPTICAL_INTERFACE, 5, 0x05, 0x85, packet_size)
enum { NCM_INTERFACE = OPTICAL_INTERFACE + 1, MSC_DESC_SIZE = TUD_MSC_DESC_LEN };
#else
#define TARGET_MSC_INTERFACE(packet_size)
enum { NCM_INTERFACE = OPTICAL_INTERFACE, MSC_DESC_SIZE = 0 };
#endif
#if MINIKMB_TARGET_NCM
#define TARGET_NCM_INTERFACES(packet_size, interval) \
  , TUD_CDC_NCM_DESCRIPTOR(NCM_INTERFACE, 6, 7, 0x86, 16, 0x07, 0x87, packet_size, \
                           MANAGEMENT_ETH_MAX, interval, 0x01)
enum { TARGET_ITF_COUNT = NCM_INTERFACE + 2, NCM_DESC_SIZE = TUD_CDC_NCM_DESC_LEN };
#else
#define TARGET_NCM_INTERFACES(packet_size, interval)
enum { TARGET_ITF_COUNT = NCM_INTERFACE, NCM_DESC_SIZE = 0 };
#endif
enum { TARGET_CONFIG_SIZE = 9 + TUD_CDC_DESC_LEN + HID_DESC_SIZE + MSC_DESC_SIZE + NCM_DESC_SIZE };

/* Interrupt intervals: 8 microframes at HS, 1 frame at FS. */
#define TARGET_CONFIGURATION(type, packet_size, interval) \
  9, (type), U16_TO_U8S_LE(TARGET_CONFIG_SIZE), TARGET_ITF_COUNT, 1, 0, 0xC0, 0, \
  8, TUSB_DESC_INTERFACE_ASSOCIATION, 0, 2, 2, 2, 0, 0, \
  9, TUSB_DESC_INTERFACE, 0, 0, 1, 2, 2, 0, 4, \
  5, TUSB_DESC_CS_INTERFACE, CDC_FUNC_DESC_HEADER, 0x20, 0x01, \
  5, TUSB_DESC_CS_INTERFACE, CDC_FUNC_DESC_CALL_MANAGEMENT, 0, 1, \
  4, TUSB_DESC_CS_INTERFACE, CDC_FUNC_DESC_ABSTRACT_CONTROL_MANAGEMENT, 2, \
  5, TUSB_DESC_CS_INTERFACE, CDC_FUNC_DESC_UNION, 0, 1, \
  7, TUSB_DESC_ENDPOINT, 0x81, TUSB_XFER_INTERRUPT, 16, 0, (interval), \
  9, TUSB_DESC_INTERFACE, 1, 0, 2, TUSB_CLASS_CDC_DATA, 0, 0, 0, \
  7, TUSB_DESC_ENDPOINT, 0x02, TUSB_XFER_BULK, U16_TO_U8S_LE(packet_size), 0, \
  7, TUSB_DESC_ENDPOINT, 0x82, TUSB_XFER_BULK, U16_TO_U8S_LE(packet_size), 0 \
  TARGET_HID_INTERFACES(interval) \
  TARGET_MSC_INTERFACE(packet_size) \
  TARGET_NCM_INTERFACES(packet_size, interval)

static const uint8_t configuration_fs[] = {TARGET_CONFIGURATION(TUSB_DESC_CONFIGURATION, 64, 1)};
_Static_assert(sizeof(configuration_fs) == TARGET_CONFIG_SIZE, "FS descriptor length");
#if MINIKMB_TARGET_HIGH_SPEED
static const uint8_t configuration_hs[] = {TARGET_CONFIGURATION(TUSB_DESC_CONFIGURATION, 512, 4)};
static const uint8_t other_fs[] = {TARGET_CONFIGURATION(TUSB_DESC_OTHER_SPEED_CONFIG, 64, 1)};
static const uint8_t other_hs[] = {TARGET_CONFIGURATION(TUSB_DESC_OTHER_SPEED_CONFIG, 512, 4)};
_Static_assert(sizeof(configuration_hs) == TARGET_CONFIG_SIZE, "HS descriptor length");
_Static_assert(sizeof(other_fs) == TARGET_CONFIG_SIZE, "Other-speed FS descriptor length");
_Static_assert(sizeof(other_hs) == TARGET_CONFIG_SIZE, "Other-speed HS descriptor length");
#endif

const uint8_t *tud_descriptor_device_cb(void)
{
  return (const uint8_t *)&device;
}

const uint8_t *tud_descriptor_configuration_cb(uint8_t index)
{
  if (index != 0U) return NULL;
#if MINIKMB_TARGET_HIGH_SPEED
  return tud_speed_get() == TUSB_SPEED_HIGH ? configuration_hs : configuration_fs;
#else
  return configuration_fs;
#endif
}

const uint8_t *tud_descriptor_device_qualifier_cb(void)
{
#if MINIKMB_TARGET_HIGH_SPEED
  return (const uint8_t *)&qualifier;
#else
  return NULL;
#endif
}

const uint8_t *tud_descriptor_other_speed_configuration_cb(uint8_t index)
{
#if MINIKMB_TARGET_HIGH_SPEED
  if (index != 0U) return NULL;
  return tud_speed_get() == TUSB_SPEED_HIGH ? other_fs : other_hs;
#else
  (void)index;
  return NULL;
#endif
}

const uint16_t *tud_descriptor_string_cb(uint8_t index, uint16_t language)
{
  static const char *const strings[] = {NULL, "MiniKMB",
#if MINIKMB_TARGET_MSC
                                       "MiniKMB Target",
#elif MINIKMB_TARGET_HID
                                       "MiniKMB Target HID",
#else
                                       "MiniKMB Target Diagnostics",
#endif
                                       NULL, "Target CDC", "Virtual optical drive", "MiniKMB NCM"};
  static uint16_t result[32];
  size_t length = 0;
  if (index == 0U) {
    result[1] = 0x0409;
    length = 1;
  } else if (language != 0x0409U && language != 0U) {
    return NULL;
  } else if (index == 3U) {
    static const char hex[] = "0123456789ABCDEF";
    const volatile uint8_t *uid = (const volatile uint8_t *)UID_BASE;
    for (size_t i = 0; i < 12U; ++i) {
      result[1 + 2 * i] = hex[uid[i] >> 4];
      result[2 + 2 * i] = hex[uid[i] & 15U];
    }
    length = 24;
#if MINIKMB_TARGET_NCM
  } else if (index == 7U) {
    static const char hex[] = "0123456789ABCDEF";
    const volatile uint8_t *uid = (const volatile uint8_t *)UID_BASE;
    uint64_t hash = 14695981039346656037ULL;
    for (size_t i = 0; i < MANAGEMENT_UID_BYTES; ++i) hash = (hash ^ uid[i]) * 1099511628211ULL;
    const uint64_t mac = 0x020000000000ULL | (hash & 0xFFFFFFFFFFULL);
    for (size_t i = 0; i < 12; ++i) result[1 + i] = (uint16_t)hex[(mac >> (44U - 4U * i)) & 15U];
    length = 12;
#endif
  } else if (index < TU_ARRAY_SIZE(strings)) {
    for (const char *text = strings[index]; *text; ++text) {
      result[++length] = (uint8_t)*text;
    }
  } else {
    return NULL;
  }
  result[0] = (TUSB_DESC_STRING << 8) | (uint16_t)(2 + 2 * length);
  return result;
}

#include "tusb.h"
#include "stm32h7xx.h"

/* TinyUSB example identifiers for local hardware testing only. */
#ifndef MINIKMB_FS_VID
#define MINIKMB_FS_VID 0xCAFE
#endif
#ifndef MINIKMB_FS_PID
#if MINIKMB_MANAGEMENT_FAST_CDC
#define MINIKMB_FS_PID 0x4004
#else
#define MINIKMB_FS_PID 0x4002
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
  .idVendor = MINIKMB_FS_VID,
  .idProduct = MINIKMB_FS_PID,
  .bcdDevice = 0x0100,
  .iManufacturer = 1,
  .iProduct = 2,
  .iSerialNumber = 3,
  .bNumConfigurations = 1,
};

/* This CDC function is internal to the separately powered USB hub. */
static const uint8_t configuration[] = {
  TUD_CONFIG_DESCRIPTOR(1, 3, 0, 9 + TUD_CDC_DESC_LEN + TUD_MSC_DESC_LEN, TUSB_DESC_CONFIG_ATT_SELF_POWERED, 0),
  8, TUSB_DESC_INTERFACE_ASSOCIATION, 0, 2, 2, 2, 0, 0,
  9, TUSB_DESC_INTERFACE, 0, 0, 1, 2, 2, 0, 4,
  5, TUSB_DESC_CS_INTERFACE, CDC_FUNC_DESC_HEADER, 0x20, 0x01,
  5, TUSB_DESC_CS_INTERFACE, CDC_FUNC_DESC_CALL_MANAGEMENT, 0, 1,
  4, TUSB_DESC_CS_INTERFACE, CDC_FUNC_DESC_ABSTRACT_CONTROL_MANAGEMENT, 2,
  5, TUSB_DESC_CS_INTERFACE, CDC_FUNC_DESC_UNION, 0, 1,
  7, TUSB_DESC_ENDPOINT, 0x81, TUSB_XFER_INTERRUPT, 16, 0, 16,
  9, TUSB_DESC_INTERFACE, 1, 0, 2, TUSB_CLASS_CDC_DATA, 0, 0, 0,
  7, TUSB_DESC_ENDPOINT, 0x02, TUSB_XFER_BULK, 64, 0, 0,
  7, TUSB_DESC_ENDPOINT, 0x82, TUSB_XFER_BULK, 64, 0, 0,
  TUD_MSC_DESCRIPTOR(2, 5, 0x03, 0x83, 64),
};
_Static_assert(sizeof(configuration) == 9 + TUD_CDC_DESC_LEN + TUD_MSC_DESC_LEN, "FS descriptor length");

uint8_t const *tud_descriptor_device_cb(void)
{
  return (const uint8_t *)&device;
}

uint8_t const *tud_descriptor_configuration_cb(uint8_t index)
{
  return index == 0U ? configuration : NULL;
}

uint16_t const *tud_descriptor_string_cb(uint8_t index, uint16_t language)
{
  static const char *const strings[] = {NULL, "MiniKMB", "MiniKMB Management", NULL, "Management CDC", "Virtual Disk"};
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

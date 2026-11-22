#include <assert.h>
#include <stdio.h>

#include "target_hid_usb.h"
#include "target_hid_report.h"
#include "target_usb.h"
#include "target_tusb_namespace.h"
#include "tusb.h"

const uint8_t test_uid[12] = {0};
static int lock_depth;
static bool ready;
static bool reject;
static uint8_t sent_instance;
static uint16_t sent_length;
static uint8_t sent_report[TARGET_HID_KEYBOARD_BYTES];
static tusb_speed_t speed;

void test_lock(void) { assert(++lock_depth == 1); }
void test_unlock(void) { assert(lock_depth-- == 1); }
bool tud_mounted(void) { return ready; }
bool tud_suspended(void) { return false; }
tusb_speed_t tud_speed_get(void) { return speed; }
bool tud_hid_n_ready(uint8_t instance) { return instance < 2U && sent_length == 0U; }

bool tud_hid_n_report(uint8_t instance, uint8_t id, const void *report, uint16_t length)
{
  assert(lock_depth == 0 && id == 0 && length <= sizeof(sent_report) && sent_length == 0U);
  if (reject) return false;
  sent_instance = instance;
  sent_length = length;
  memcpy(sent_report, report, length);
  return true;
}

static void Ack(void)
{
  assert(sent_length != 0U);
  const uint16_t length = sent_length;
  sent_length = 0U;
  tud_hid_report_complete_cb(sent_instance, sent_report, length);
}

static void CheckConfig(const uint8_t *config, bool high_speed, uint8_t type)
{
  assert(config && config[1] == type && config[4] == 7);
  const uint16_t config_size = (uint16_t)((uint16_t)config[3] << 8U | config[2]);
  assert(config_size == 9 + TUD_CDC_DESC_LEN + 2 * TUD_HID_DESC_LEN + TUD_MSC_DESC_LEN + TUD_CDC_NCM_DESC_LEN);
  const uint8_t *keyboard = config + 9 + TUD_CDC_DESC_LEN;
  const uint8_t *mouse = keyboard + TUD_HID_DESC_LEN;
  assert(keyboard[2] == 2 && keyboard[6] == HID_SUBCLASS_BOOT && keyboard[7] == HID_ITF_PROTOCOL_KEYBOARD);
  assert(mouse[2] == 3 && mouse[6] == 0 && mouse[7] == HID_ITF_PROTOCOL_NONE);
  assert(keyboard[20] == 0x83 && keyboard[22] == 8 && keyboard[24] == (high_speed ? 4 : 1));
  assert(mouse[20] == 0x84 && mouse[22] == 8 && mouse[24] == (high_speed ? 4 : 1));
  const uint8_t *msc = mouse + TUD_HID_DESC_LEN;
  const uint8_t *ncm = msc + TUD_MSC_DESC_LEN;
  assert(msc[2] == 4 && msc[5] == TUSB_CLASS_MSC && msc[11] == 0x05 && msc[18] == 0x85);
  assert(ncm[1] == TUSB_DESC_INTERFACE_ASSOCIATION && ncm[2] == 5 && ncm[3] == 2);
  assert(ncm[13] == TUSB_CLASS_CDC && ncm[14] == CDC_COMM_SUBCLASS_NETWORK_CONTROL_MODEL);
  assert(tud_hid_descriptor_report_cb(0) && tud_hid_descriptor_report_cb(1));
  assert(tud_hid_descriptor_report_cb(2) == NULL);
}

static void Expect(const ManagementHidBackend *backend, uint32_t ticket, TargetHidStatus status)
{
  ManagementHidEvent event;
  assert(backend->take_event(backend->context, &event));
  assert(event.ticket == ticket && event.status == status && event.session == 1U);
  assert(!backend->take_event(backend->context, &event));
}

int main(void)
{
  speed = TUSB_SPEED_FULL;
  CheckConfig(tud_descriptor_configuration_cb(0), false, TUSB_DESC_CONFIGURATION);
  assert(tud_descriptor_configuration_cb(1) == NULL);
  if (MINIKMB_TARGET_HIGH_SPEED) {
    CheckConfig(tud_descriptor_other_speed_configuration_cb(0), true, TUSB_DESC_OTHER_SPEED_CONFIG);
    speed = TUSB_SPEED_HIGH;
    CheckConfig(tud_descriptor_configuration_cb(0), true, TUSB_DESC_CONFIGURATION);
    CheckConfig(tud_descriptor_other_speed_configuration_cb(0), false, TUSB_DESC_OTHER_SPEED_CONFIG);
  } else {
    assert(tud_descriptor_other_speed_configuration_cb(0) == NULL);
  }

  TargetHidUsb_Init();
  const ManagementHidBackend *backend = TargetHidUsb_Backend();
  const uint8_t key[] = {4, 0};
  uint32_t ticket;
  assert(backend->submit(backend->context, 1, 0x0101, key, sizeof(key), &ticket) == TARGET_HID_NOT_READY);
  ready = true;
  TargetHidUsb_Poll(0);
  assert(sent_instance == 0 && sent_length == 8 && sent_report[2] == 0);
  Ack();
  TargetHidUsb_Poll(1);
  assert(sent_instance == 1 && sent_length == 7 && sent_report[0] == 0);
  Ack();

  for (uint32_t i = 0; i < 1000; ++i) {
    assert(backend->submit(backend->context, 1, 0x0101, key, sizeof(key), &ticket) == TARGET_HID_OK);
    TargetHidUsb_Poll(i * 2 + 2);
    assert(sent_instance == 0 && sent_report[2] == 4);
    ManagementHidEvent event;
    assert(!backend->take_event(backend->context, &event));
    Ack();
    Expect(backend, ticket, TARGET_HID_OK);
    assert(backend->submit(backend->context, 1, 0x0102, key, sizeof(key), &ticket) == TARGET_HID_OK);
    TargetHidUsb_Poll(i * 2 + 3);
    assert(sent_instance == 0 && sent_report[2] == 0);
    Ack();
    Expect(backend, ticket, TARGET_HID_OK);
  }

  const uint8_t scroll[] = {1, 0, 0xff, 0xff};
  assert(backend->submit(backend->context, 1, 0x0106, scroll, sizeof(scroll), &ticket) == TARGET_HID_OK);
  TargetHidUsb_Poll(2002);
  assert(sent_instance == 1 && sent_report[5] == 1 && sent_report[6] == 0xff);
  Ack();
  Expect(backend, ticket, TARGET_HID_OK);
  uint8_t report[TARGET_HID_KEYBOARD_BYTES];
  assert(tud_hid_get_report_cb(1, 0, HID_REPORT_TYPE_INPUT, report, sizeof(report)) == 7);
  assert(report[5] == 0 && report[6] == 0);

  assert(backend->submit(backend->context, 1, 0x0101, key, sizeof(key), &ticket) == TARGET_HID_OK);
  reject = true;
  TargetHidUsb_Poll(2003);
  Expect(backend, ticket, TARGET_HID_IO_ERROR);
  reject = false;
  TargetHidUsb_Poll(2004);
  assert(sent_instance == 0 && sent_report[2] == 0);
  Ack();
  TargetHidUsb_Poll(2005);
  assert(sent_instance == 1 && sent_report[0] == 0);
  Ack();

  assert(backend->submit(backend->context, 1, 0x0101, key, sizeof(key), &ticket) == TARGET_HID_OK);
  TargetHidUsb_Poll(2006);
  ready = false;
  tud_umount_cb();
  Expect(backend, ticket, TARGET_HID_CANCELLED);
  Ack(); /* Discard completion after a reset. */
  ready = true;
  tud_mount_cb();
  TargetHidUsb_Poll(2007);
  assert(sent_instance == 0 && sent_report[2] == 0);
  Ack();
  TargetHidUsb_Poll(2008);
  assert(sent_instance == 1 && sent_report[0] == 0);
  Ack();
  assert(tud_hid_set_idle_cb(0, 1));
  TargetHidUsb_Poll(2011);
  assert(sent_length == 0);
  TargetHidUsb_Poll(2012);
  assert(sent_instance == 0 && sent_report[2] == 0);
  Ack();
  assert(tud_hid_set_idle_cb(0, 0) && tud_hid_set_idle_cb(1, 1));
  TargetHidUsb_Poll(2016);
  assert(sent_instance == 1 && sent_report[5] == 0 && sent_report[6] == 0);
  Ack();
  puts(MINIKMB_TARGET_HIGH_SPEED ? "HS HID adapter passed" : "FS HID adapter passed");
}

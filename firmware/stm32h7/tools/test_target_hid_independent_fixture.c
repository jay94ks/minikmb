#include <assert.h>
#include <stdio.h>
#include <string.h>
#include "target_hid_usb.h"
#include "target_hid_report.h"
#include "management_hid.h"
#include "target_usb.h"
#include "target_tusb_namespace.h"
#include "tusb.h"

const uint8_t test_uid[12] = {0};
static unsigned lock_depth;
static bool connected;
static uint8_t reports[2][8];
static uint16_t lengths[2];
static uint32_t now;

void test_lock(void) { assert(++lock_depth == 1); }
void test_unlock(void) { assert(lock_depth-- == 1); }
bool tud_mounted(void) { return connected; }
bool tud_suspended(void) { return false; }
tusb_speed_t tud_speed_get(void) { return MINIKMB_TARGET_HIGH_SPEED ? TUSB_SPEED_HIGH : TUSB_SPEED_FULL; }
bool tud_hid_n_ready(uint8_t instance) { return instance < 2 && lengths[instance] == 0; }
bool tud_hid_n_report(uint8_t instance, uint8_t id, const void *report, uint16_t length)
{
  assert(lock_depth == 0 && id == 0 && instance < 2 && length <= 8 && lengths[instance] == 0);
  memcpy(reports[instance], report, length);
  lengths[instance] = length;
  return true;
}

static void Poll(void) { TargetHidUsb_Poll(++now); }
static void Ack(uint8_t instance)
{
  assert(lengths[instance] != 0);
  const uint16_t size = lengths[instance];
  lengths[instance] = 0;
  tud_hid_report_complete_cb(instance, reports[instance], size);
}
static void Empty(const ManagementHidBackend *backend)
{
  ManagementHidEvent event;
  assert(!backend->take_event(backend->context, &event));
}
static void Expect(const ManagementHidBackend *backend, uint64_t session, uint32_t ticket,
                   TargetHidStatus status)
{
  ManagementHidEvent event;
  assert(backend->take_event(backend->context, &event));
  assert(event.session == session && event.ticket == ticket && event.status == status);
  Empty(backend);
}
static uint32_t Submit(const ManagementHidBackend *backend, uint64_t session, uint16_t op)
{
  const uint8_t key[] = {4, 0};
  uint32_t ticket;
  const bool release = op == TARGET_HID_RELEASE_ALL;
  assert(backend->submit(backend->context, session, op, release ? NULL : key,
                         release ? 0 : sizeof(key), &ticket) == TARGET_HID_OK);
  return ticket;
}

int main(void)
{
  TargetHidUsb_Init();
  const ManagementHidBackend *backend = TargetHidUsb_Backend();
  const ManagementHid *hid = backend->context;
  connected = true;
  Poll();
  assert(lengths[0] == 8 && reports[0][2] == 0);
  Ack(0);
  Poll();
  assert(lengths[1] == 7);
  /* The simulated boot host never polls the absolute mouse. */
  for (unsigned index = 0; index < 1000; ++index) {
    uint32_t ticket = Submit(backend, 1, 0x0101);
    Poll();
    assert(lengths[0] == 8 && reports[0][2] == 4 && lengths[1] == 7);
    Empty(backend);
    Ack(0);
    Expect(backend, 1, ticket, TARGET_HID_OK);
    ticket = Submit(backend, 1, 0x0102);
    Poll();
    assert(lengths[0] == 8 && reports[0][2] == 0);
    Ack(0);
    Expect(backend, 1, ticket, TARGET_HID_OK);
  }
  uint32_t ticket = Submit(backend, 1, 0x0101);
  Poll(); Ack(0); Expect(backend, 1, ticket, TARGET_HID_OK);
  Ack(1); /* A late mouse completion must not overwrite committed keyboard keys. */
  assert(hid->queue.committed.keys[0] == 4);

  assert(tud_hid_set_idle_cb(1, 1));
  now += 4;
  Poll();
  assert(lengths[1] == 7);
  backend->end_session(backend->context, 1);
  Poll();
  assert(lengths[0] == 8 && reports[0][2] == 0);
  Ack(0);
  Empty(backend);
  ticket = Submit(backend, 2, 0x0101);
  Poll(); Ack(0); Expect(backend, 2, ticket, TARGET_HID_OK);
  Ack(1);
  assert(tud_hid_set_idle_cb(1, 0));
  Poll(); Ack(1);
  assert(hid->queue.committed.keys[0] == 4);

  /* RELEASE_ALL completion still requires both endpoints, but a neutralized
   * keyboard can resume while the host leaves the mouse unpolled. */
  const uint32_t release = Submit(backend, 2, TARGET_HID_RELEASE_ALL);
  Poll(); Poll();
  assert(lengths[0] == 8 && lengths[1] == 7);
  Ack(0);
  Empty(backend);
  ticket = Submit(backend, 2, 0x0101);
  Poll();
  assert(lengths[0] == 8);
  Ack(0); Expect(backend, 2, ticket, TARGET_HID_OK);
  Ack(1);
  Expect(backend, 2, release, TARGET_HID_OK);

  /* A real mouse command, its full pending FIFO, and a queued release must
   * never prevent the independent keyboard DOWN/UP sequence from completing. */
  const uint8_t position[] = {0x22, 0x11, 0x44, 0x33};
  uint32_t move, queued;
  assert(backend->submit(backend->context, 2, 0x0103, position, sizeof(position), &move) == TARGET_HID_OK);
  Poll();
  assert(lengths[1] == 7);
  for (unsigned i = 0; i < TARGET_HID_QUEUE_DEPTH; ++i) {
    assert(backend->submit(backend->context, 2, 0x0103, position, sizeof(position), &queued) == TARGET_HID_OK);
  }
  assert(backend->submit(backend->context, 2, 0x0103, position, sizeof(position), &queued) == TARGET_HID_BUSY);
  for (unsigned i = 0; i < 1000; ++i) {
    ticket = Submit(backend, 2, 0x0101);
    Poll(); Ack(0); Expect(backend, 2, ticket, TARGET_HID_OK);
    ticket = Submit(backend, 2, 0x0102);
    Poll(); Ack(0); Expect(backend, 2, ticket, TARGET_HID_OK);
  }
  backend->end_session(backend->context, 2);
  Empty(backend);
  Poll(); Ack(0);
  ticket = Submit(backend, 3, 0x0101);
  Poll(); Ack(0); Expect(backend, 3, ticket, TARGET_HID_OK);
  Ack(1); Empty(backend); /* old session's normal mouse completion */
  Poll(); Ack(1); Empty(backend); /* neutral mouse, never clears keyboard keys */
  assert(hid->queue.committed.keys[0] == 4);

  /* Short completion fails the ticket and schedules fresh neutral reports. */
  ticket = Submit(backend, 3, 0x0102);
  Poll();
  assert(lengths[0] == 8);
  lengths[0] = 0;
  tud_hid_report_complete_cb(0, reports[0], 1);
  Expect(backend, 3, ticket, TARGET_HID_IO_ERROR);
  Poll(); Ack(0);
  Poll();
  assert(lengths[1] == 7);
  ticket = Submit(backend, 3, 0x0101);
  Poll();
  assert(lengths[0] == 8 && reports[0][2] == 4);
  backend->end_session(backend->context, 3);
  Empty(backend);
  Ack(0);
  Poll();
  assert(lengths[0] == 8 && reports[0][2] == 0);
  Ack(0);
  ticket = Submit(backend, 4, 0x0101);
  Poll(); Ack(0); Expect(backend, 4, ticket, TARGET_HID_OK);
  Ack(1);
  assert(hid->queue.committed.keys[0] == 4);

  /* Reset cancels active input and ignores the pre-reset completion. */
  ticket = Submit(backend, 4, 0x0102);
  Poll();
  connected = false;
  tud_umount_cb();
  Expect(backend, 4, ticket, TARGET_HID_CANCELLED);
  Ack(0);
  connected = true;
  tud_mount_cb();
  Poll();
  assert(lengths[0] == 8 && reports[0][2] == 0);
  Ack(0);
  Poll();
  assert(lengths[1] == 7 && reports[1][0] == 0);
  Ack(1);
  puts(MINIKMB_TARGET_HIGH_SPEED ? "HS independent HID regression passed" : "FS independent HID regression passed");
}

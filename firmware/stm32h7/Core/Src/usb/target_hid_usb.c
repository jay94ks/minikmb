#include "target_usb.h"

#if MINIKMB_TARGET_HID
#include "target_hid_usb.h"
#include "management_hid.h"
#include "FreeRTOS.h"
#include "task.h"
#include "target_tusb_namespace.h"
#include "tusb.h"

static ManagementHid hid;
static ManagementHidBackend backend;

/* Only the target USB task owns the transfer and report snapshots. */
static struct {
  struct {
    uint64_t transfer;
    uint16_t length;
    uint8_t report[TARGET_HID_KEYBOARD_BYTES];
  } pending[2];
  uint8_t keyboard[TARGET_HID_KEYBOARD_BYTES];
  uint8_t mouse[TARGET_HID_MOUSE_BYTES];
  uint8_t idle[2];
  uint32_t now;
  uint32_t last_report[2];
} usb;

static uint16_t Submit(void *context, uint64_t session, uint16_t opcode,
                       const uint8_t *payload, size_t length, uint32_t *ticket)
{
  taskENTER_CRITICAL();
  const uint16_t status = backend.submit(context, session, opcode, payload, length, ticket);
  taskEXIT_CRITICAL();
  return status;
}

static void EndSession(void *context, uint64_t session)
{
  taskENTER_CRITICAL();
  backend.end_session(context, session);
  taskEXIT_CRITICAL();
}

static bool TakeEvent(void *context, ManagementHidEvent *event)
{
  taskENTER_CRITICAL();
  const bool ready = backend.take_event(context, event);
  taskEXIT_CRITICAL();
  return ready;
}

static uint32_t DroppedEvents(void *context)
{
  taskENTER_CRITICAL();
  const uint32_t dropped = backend.dropped_events(context);
  taskEXIT_CRITICAL();
  return dropped;
}

static const ManagementHidBackend locked_backend = {
  .context = &hid,
  .submit = Submit,
  .end_session = EndSession,
  .take_event = TakeEvent,
  .dropped_events = DroppedEvents,
};

void TargetHidUsb_Init(void)
{
  ManagementHid_Init(&hid);
  backend = ManagementHid_Backend(&hid);
}

const ManagementHidBackend *TargetHidUsb_Backend(void)
{
  return &locked_backend;
}

static void SetConnected(bool connected)
{
  taskENTER_CRITICAL();
  ManagementHid_SetConnected(&hid, connected);
  taskEXIT_CRITICAL();
}

static void Reset(void)
{
  SetConnected(false);
  memset(usb.pending, 0, sizeof(usb.pending));
  memset(usb.idle, 0, sizeof(usb.idle));
  memset(usb.keyboard, 0, sizeof(usb.keyboard));
  memset(usb.mouse, 0, sizeof(usb.mouse));
}

static void Complete(uint8_t instance, bool delivered)
{
  const uint64_t transfer = usb.pending[instance].transfer;
  usb.pending[instance].length = 0U;
  usb.pending[instance].transfer = 0U;
  taskENTER_CRITICAL();
  ManagementHid_Complete(&hid, transfer, delivered);
  taskEXIT_CRITICAL();
}

static void Send(uint8_t instance, const uint8_t *report, uint16_t length, uint64_t transfer)
{
  usb.pending[instance].transfer = transfer;
  usb.pending[instance].length = length;
  memcpy(usb.pending[instance].report, report, length);
  usb.last_report[instance] = usb.now;
  if (!tud_hid_n_report(instance, 0, usb.pending[instance].report, length)) {
    Complete(instance, false);
  }
}

static bool IdleDue(uint8_t instance)
{
  return usb.idle[instance] != 0U &&
         usb.now - usb.last_report[instance] >= (uint32_t)usb.idle[instance] * 4U;
}

void TargetHidUsb_Poll(uint32_t now)
{
  usb.now = now;
  const bool ready = tud_ready();
  SetConnected(ready);
  if (!ready) {
    return;
  }
  uint8_t available = 0U;
  for (uint8_t instance = 0; instance < 2U; ++instance) {
    if (usb.pending[instance].length == 0U && tud_hid_n_ready(instance)) {
      available |= (uint8_t)(1U << instance);
    }
  }
  if (available == 0U) {
    return;
  }

  TargetHidReports reports;
  taskENTER_CRITICAL();
  const uint64_t transfer = ManagementHid_TakeAvailableReport(&hid, available, &reports);
  taskEXIT_CRITICAL();
  if (transfer != 0U) {
    if (reports.mask == TARGET_HID_REPORT_KEYBOARD) {
      Send(TARGET_HID_KEYBOARD_INSTANCE, reports.keyboard, sizeof(reports.keyboard), transfer);
    } else if (reports.mask == TARGET_HID_REPORT_MOUSE) {
      Send(TARGET_HID_MOUSE_INSTANCE, reports.mouse, sizeof(reports.mouse), transfer);
    } else {
      taskENTER_CRITICAL();
      ManagementHid_Complete(&hid, transfer, false);
      taskEXIT_CRITICAL();
    }
  } else if ((available & TARGET_HID_REPORT_KEYBOARD) != 0U && IdleDue(TARGET_HID_KEYBOARD_INSTANCE)) {
    Send(TARGET_HID_KEYBOARD_INSTANCE, usb.keyboard, sizeof(usb.keyboard), 0U);
  } else if ((available & TARGET_HID_REPORT_MOUSE) != 0U && IdleDue(TARGET_HID_MOUSE_INSTANCE)) {
    Send(TARGET_HID_MOUSE_INSTANCE, usb.mouse, sizeof(usb.mouse), 0U);
  }
}

void tud_mount_cb(void)
{
  /* A bus reset can clear configuration and then remount in one task pass. */
  Reset();
}

void tud_umount_cb(void)
{
  Reset();
}

void tud_suspend_cb(bool remote_wakeup_en)
{
  (void)remote_wakeup_en;
  SetConnected(false);
}

void tud_hid_report_complete_cb(uint8_t instance, const uint8_t *report, uint16_t length)
{
  if (instance > TARGET_HID_MOUSE_INSTANCE || usb.pending[instance].length == 0U) {
    return;
  }
  if (length != usb.pending[instance].length) {
    Complete(instance, false);
    return;
  }
  if (instance == TARGET_HID_KEYBOARD_INSTANCE) {
    memcpy(usb.keyboard, report, sizeof(usb.keyboard));
  } else {
    memcpy(usb.mouse, report, sizeof(usb.mouse));
    /* Relative wheel and pan deltas are consumed by this transfer. */
    usb.mouse[5] = 0;
    usb.mouse[6] = 0;
  }
  Complete(instance, true);
}

void tud_hid_report_failed_cb(uint8_t instance, hid_report_type_t type,
                              const uint8_t *report, uint16_t length)
{
  (void)report;
  (void)length;
  if (type == HID_REPORT_TYPE_INPUT && instance <= TARGET_HID_MOUSE_INSTANCE &&
      usb.pending[instance].length != 0U) {
    Complete(instance, false);
  }
}

bool tud_hid_set_idle_cb(uint8_t instance, uint8_t idle_rate)
{
  if (instance > TARGET_HID_MOUSE_INSTANCE) {
    return false;
  }
  usb.idle[instance] = idle_rate;
  usb.last_report[instance] = usb.now;
  return true;
}

uint16_t tud_hid_get_report_cb(uint8_t instance, uint8_t id, hid_report_type_t type,
                               uint8_t *buffer, uint16_t requested)
{
  if (id != 0U || type != HID_REPORT_TYPE_INPUT || instance > TARGET_HID_MOUSE_INSTANCE) {
    return 0U;
  }
  const bool keyboard = instance == TARGET_HID_KEYBOARD_INSTANCE;
  const uint16_t size = keyboard ? sizeof(usb.keyboard) : sizeof(usb.mouse);
  const uint16_t length = requested < size ? requested : size;
  memcpy(buffer, keyboard ? usb.keyboard : usb.mouse, length);
  return length;
}

void tud_hid_set_report_cb(uint8_t instance, uint8_t id, hid_report_type_t type,
                           const uint8_t *buffer, uint16_t length)
{
  /* The keyboard descriptor exposes standard LEDs. The board has no LED outputs for them. */
  (void)instance;
  (void)id;
  (void)type;
  (void)buffer;
  (void)length;
}
#endif

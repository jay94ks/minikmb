#include "management_hid.h"

static void OnDone(void *context, uint32_t ticket, TargetHidStatus status)
{
  ManagementHid *hid = context;
  if (hid->session == 0U) {
    return;
  }
  if (hid->count == MANAGEMENT_HID_EVENT_DEPTH) {
    ++hid->dropped;
    return;
  }

  const size_t tail = (hid->head + hid->count) % MANAGEMENT_HID_EVENT_DEPTH;
  hid->events[tail] = (ManagementHidEvent){
    .session = hid->session,
    .ticket = ticket,
    .status = (uint16_t)status,
  };
  ++hid->count;
}

static uint16_t Submit(void *context, uint64_t session, uint16_t opcode,
                       const uint8_t *payload, size_t length, uint32_t *ticket)
{
  ManagementHid *hid = context;
  if (hid->session != 0U && hid->session != session) {
    return TARGET_HID_BUSY;
  }
  hid->session = session;
  return (uint16_t)TargetHidQueue_Submit(&hid->queue, opcode, payload, length, ticket);
}

static void EndSession(void *context, uint64_t session)
{
  ManagementHid *hid = context;
  if (hid->session == session) {
    hid->session = 0U;
    hid->head = 0U;
    hid->count = 0U;
    TargetHidQueue_EndSession(&hid->queue);
  }
}

static bool TakeEvent(void *context, ManagementHidEvent *event)
{
  ManagementHid *hid = context;
  if (hid->count == 0U) {
    return false;
  }
  *event = hid->events[hid->head];
  hid->head = (uint8_t)((hid->head + 1U) % MANAGEMENT_HID_EVENT_DEPTH);
  --hid->count;
  return true;
}

static uint32_t DroppedEvents(void *context)
{
  const ManagementHid *hid = context;
  return hid->dropped;
}

void ManagementHid_Init(ManagementHid *hid)
{
  *hid = (ManagementHid){0};
  TargetHidQueue_Init(&hid->queue, OnDone, hid);
}

ManagementHidBackend ManagementHid_Backend(ManagementHid *hid)
{
  return (ManagementHidBackend){
    .context = hid,
    .submit = Submit,
    .end_session = EndSession,
    .take_event = TakeEvent,
    .dropped_events = DroppedEvents,
  };
}

void ManagementHid_SetConnected(ManagementHid *hid, bool connected)
{
  TargetHidQueue_SetConnected(&hid->queue, connected);
}

uint64_t ManagementHid_TakeReport(ManagementHid *hid, TargetHidReports *reports)
{
  return TargetHidQueue_Take(&hid->queue, reports);
}

void ManagementHid_Complete(ManagementHid *hid, uint64_t transfer, bool delivered)
{
  TargetHidQueue_Complete(&hid->queue, transfer, delivered);
}

uint64_t ManagementHid_TakeAvailableReport(ManagementHid *hid, uint8_t available,
                                         TargetHidReports *reports)
{
  return TargetHidQueue_TakeAvailable(&hid->queue, available, reports);
}

#include "target_hid_queue.h"

#include <assert.h>
#include <stdio.h>
#include <string.h>

typedef struct {
  struct {
    uint32_t ticket;
    TargetHidStatus status;
  } done[128];
  size_t count;
} Events;

static void OnDone(void *context, uint32_t ticket, TargetHidStatus status)
{
  Events *events = context;
  assert(events->count < sizeof(events->done) / sizeof(events->done[0]));
  events->done[events->count].ticket = ticket;
  events->done[events->count].status = status;
  ++events->count;
}

static void Expect(const Events *events, size_t index, uint32_t ticket, TargetHidStatus status)
{
  assert(index < events->count);
  assert(events->done[index].ticket == ticket && events->done[index].status == status);
}

static uint64_t Take(TargetHidQueue *queue, TargetHidReports *reports, uint8_t mask)
{
  const uint64_t transfer = TargetHidQueue_Take(queue, reports);
  assert(transfer != 0U && reports->mask == mask);
  return transfer;
}

static void Neutral(TargetHidQueue *queue, const uint8_t mouse[7])
{
  TargetHidReports report;
  const uint64_t keyboard = Take(queue, &report, TARGET_HID_REPORT_KEYBOARD);
  assert(memcmp(report.keyboard, (uint8_t[8]){0}, 8) == 0);
  assert(TargetHidQueue_Take(queue, &report) == 0U);
  TargetHidQueue_Complete(queue, keyboard, true);
  const uint64_t pointer = Take(queue, &report, TARGET_HID_REPORT_MOUSE);
  assert(memcmp(report.mouse, mouse, 7) == 0);
  TargetHidQueue_Complete(queue, pointer, true);
}

static void Start(TargetHidQueue *queue, Events *events)
{
  TargetHidQueue_Init(queue, OnDone, events);
  TargetHidQueue_SetConnected(queue, true);
  Neutral(queue, (uint8_t[7]){0});
  assert(events->count == 0U);
}

static void FifoAndFullRelease(void)
{
  TargetHidQueue queue;
  Events events = {0};
  TargetHidReports report;
  Start(&queue, &events);
  const uint8_t a[] = {4, 0};
  uint32_t down, up, ticket;
  assert(TargetHidQueue_Submit(&queue, 0x0101, a, 2, &down) == TARGET_HID_OK);
  assert(TargetHidQueue_Submit(&queue, 0x0102, a, 2, &up) == TARGET_HID_OK);
  assert(events.count == 0);
  uint64_t transfer = Take(&queue, &report, TARGET_HID_REPORT_KEYBOARD);
  assert(report.keyboard[2] == 4);
  TargetHidQueue_Complete(&queue, transfer, true);
  Expect(&events, 0, down, TARGET_HID_OK);
  transfer = Take(&queue, &report, TARGET_HID_REPORT_KEYBOARD);
  assert(report.keyboard[2] == 0);
  TargetHidQueue_Complete(&queue, transfer, true);
  Expect(&events, 1, up, TARGET_HID_OK);

  for (unsigned i = 0; i < TARGET_HID_QUEUE_DEPTH; ++i) {
    assert(TargetHidQueue_Submit(&queue, 0x0106, (uint8_t[4]){1}, 4, &ticket) == TARGET_HID_OK);
    assert(ticket == 3U + i);
  }
  assert(TargetHidQueue_Submit(&queue, 0x0106, (uint8_t[4]){1}, 4, &ticket) == TARGET_HID_BUSY);
  assert(ticket == 2 + TARGET_HID_QUEUE_DEPTH);
  assert(TargetHidQueue_Submit(&queue, TARGET_HID_RELEASE_ALL, NULL, 0, &ticket) == TARGET_HID_OK);
  assert(ticket == 3 + TARGET_HID_QUEUE_DEPTH);
  for (unsigned i = 0; i < TARGET_HID_QUEUE_DEPTH; ++i) {
    Expect(&events, 2 + i, 3 + i, TARGET_HID_CANCELLED);
  }
  uint32_t release = ticket;
  assert(TargetHidQueue_Submit(&queue, 0x0101, a, 2, &ticket) == TARGET_HID_OK);
  Neutral(&queue, (uint8_t[7]){0});
  Expect(&events, 2 + TARGET_HID_QUEUE_DEPTH, release, TARGET_HID_OK);
  transfer = Take(&queue, &report, TARGET_HID_REPORT_KEYBOARD);
  assert(report.keyboard[2] == 4);
  TargetHidQueue_Complete(&queue, transfer, true);
  Expect(&events, 3 + TARGET_HID_QUEUE_DEPTH, ticket, TARGET_HID_OK);
}

static void ReleaseWaitsForActiveReport(void)
{
  TargetHidQueue queue;
  Events events = {0};
  TargetHidReports report;
  Start(&queue, &events);
  uint32_t move, discarded, release;
  const uint8_t position[] = {0x22, 0x11, 0x44, 0x33};
  assert(TargetHidQueue_Submit(&queue, 0x0103, position, 4, &move) == TARGET_HID_OK);
  const uint64_t active = Take(&queue, &report, TARGET_HID_REPORT_MOUSE);
  assert(memcmp(report.mouse, (uint8_t[]){0, 0x22, 0x11, 0x44, 0x33, 0, 0}, 7) == 0);
  assert(TargetHidQueue_Submit(&queue, 0x0104, (uint8_t[]){1}, 1, &discarded) == TARGET_HID_OK);
  assert(TargetHidQueue_Submit(&queue, TARGET_HID_RELEASE_ALL, NULL, 0, &release) == TARGET_HID_OK);
  Expect(&events, 0, discarded, TARGET_HID_CANCELLED);
  assert(TargetHidQueue_Take(&queue, &report) == 0U);
  TargetHidQueue_Complete(&queue, active, true);
  Expect(&events, 1, move, TARGET_HID_OK);
  Neutral(&queue, (uint8_t[]){0, 0x22, 0x11, 0x44, 0x33, 0, 0});
  Expect(&events, 2, release, TARGET_HID_OK);
  assert(TargetHidQueue_Take(&queue, &report) == 0U);
}

static void DisconnectAndStaleCompletion(void)
{
  TargetHidQueue queue;
  Events events = {0};
  TargetHidReports report;
  Start(&queue, &events);
  const uint8_t a[] = {4, 0};
  uint32_t down, pending, offline_release;
  assert(TargetHidQueue_Submit(&queue, 0x0101, a, 2, &down) == TARGET_HID_OK);
  const uint64_t old = Take(&queue, &report, TARGET_HID_REPORT_KEYBOARD);
  assert(TargetHidQueue_Submit(&queue, 0x0102, a, 2, &pending) == TARGET_HID_OK);
  TargetHidQueue_SetConnected(&queue, false);
  Expect(&events, 0, down, TARGET_HID_CANCELLED);
  Expect(&events, 1, pending, TARGET_HID_CANCELLED);
  assert(TargetHidQueue_Submit(&queue, 0x0101, a, 2, &offline_release) == TARGET_HID_NOT_READY);
  assert(TargetHidQueue_Submit(&queue, TARGET_HID_RELEASE_ALL, NULL, 0, &offline_release) == TARGET_HID_OK);
  assert(offline_release > pending);
  assert(TargetHidQueue_Take(&queue, &report) == 0);
  TargetHidQueue_SetConnected(&queue, true);
  const uint64_t keyboard = Take(&queue, &report, TARGET_HID_REPORT_KEYBOARD);
  assert(keyboard != old && report.keyboard[2] == 0);
  TargetHidQueue_Complete(&queue, old, true);
  assert(events.count == 2 && TargetHidQueue_Take(&queue, &report) == 0U);
  TargetHidQueue_Complete(&queue, keyboard, true);
  assert(events.count == 2);
  TargetHidQueue_Complete(&queue, Take(&queue, &report, TARGET_HID_REPORT_MOUSE), true);
  Expect(&events, 2, offline_release, TARGET_HID_OK);
}

static void TransferFailureAndSessionEnd(void)
{
  TargetHidQueue queue;
  Events events = {0};
  TargetHidReports report;
  Start(&queue, &events);
  uint32_t down, pending;
  const uint8_t a[] = {4, 0};
  assert(TargetHidQueue_Submit(&queue, 0x0101, a, 2, &down) == TARGET_HID_OK);
  const uint64_t failed = Take(&queue, &report, TARGET_HID_REPORT_KEYBOARD);
  assert(TargetHidQueue_Submit(&queue, 0x0102, a, 2, &pending) == TARGET_HID_OK);
  TargetHidQueue_Complete(&queue, failed, false);
  Expect(&events, 0, pending, TARGET_HID_CANCELLED);
  Expect(&events, 1, down, TARGET_HID_IO_ERROR);
  Neutral(&queue, (uint8_t[7]){0});

  assert(TargetHidQueue_Submit(&queue, 0x0101, a, 2, &down) == TARGET_HID_OK);
  const uint64_t stale_session = Take(&queue, &report, TARGET_HID_REPORT_KEYBOARD);
  assert(TargetHidQueue_Submit(&queue, 0x0102, a, 2, &pending) == TARGET_HID_OK);
  TargetHidQueue_EndSession(&queue);
  Expect(&events, 2, pending, TARGET_HID_CANCELLED);
  Expect(&events, 3, down, TARGET_HID_CANCELLED);
  assert(TargetHidQueue_Take(&queue, &report) == 0U);
  TargetHidQueue_Complete(&queue, stale_session, true);
  Neutral(&queue, (uint8_t[7]){0});
  assert(events.count == 4);
  uint32_t release;
  assert(TargetHidQueue_Submit(&queue, TARGET_HID_RELEASE_ALL, NULL, 0, &release) == TARGET_HID_OK);
  assert(TargetHidQueue_Submit(&queue, TARGET_HID_RELEASE_ALL, NULL, 0, &pending) == TARGET_HID_BUSY);
  const uint64_t release_transfer = Take(&queue, &report, TARGET_HID_REPORT_KEYBOARD);
  TargetHidQueue_EndSession(&queue);
  Expect(&events, 4, release, TARGET_HID_CANCELLED);
  TargetHidQueue_Complete(&queue, release_transfer, true);
  TargetHidQueue_Complete(&queue, Take(&queue, &report, TARGET_HID_REPORT_MOUSE), true);
  assert(events.count == 5U);

  assert(TargetHidQueue_Submit(&queue, TARGET_HID_RELEASE_ALL, NULL, 0, &release) == TARGET_HID_OK);
  TargetHidQueue_Complete(&queue, Take(&queue, &report, TARGET_HID_REPORT_KEYBOARD), false);
  Expect(&events, 5, release, TARGET_HID_IO_ERROR);
  Neutral(&queue, (uint8_t[7]){0});
  assert(events.count == 6U);

  queue.last_ticket = UINT32_MAX;
  assert(TargetHidQueue_Submit(&queue, 0x0101, a, 2, &down) == TARGET_HID_BUSY);
  TargetHidQueue_EndSession(&queue);
  Neutral(&queue, (uint8_t[7]){0});
}

int main(void)
{
  FifoAndFullRelease();
  ReleaseWaitsForActiveReport();
  DisconnectAndStaleCompletion();
  TransferFailureAndSessionEnd();
  puts("HID FIFO, reserved release, failed transfer, session and stale completion tests passed.");
  return 0;
}

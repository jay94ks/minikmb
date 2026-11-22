#include "target_hid_queue.h"

#include <string.h>

enum { NEUTRAL_REPORTS = TARGET_HID_REPORT_KEYBOARD | TARGET_HID_REPORT_MOUSE };

static void Notify(const TargetHidQueue *queue, uint32_t ticket, TargetHidStatus status)
{
  if (ticket != 0U && queue->done != NULL) {
    queue->done(queue->context, ticket, status);
  }
}

static void CancelPending(TargetHidQueue *queue)
{
  for (unsigned index = 0; index < 2U; ++index) {
    TargetHidLane *lane = &queue->lane[index];
    while (lane->count != 0U) {
      const uint32_t ticket = lane->pending[lane->head].ticket;
      lane->head = (uint8_t)((lane->head + 1U) % TARGET_HID_QUEUE_DEPTH);
      --lane->count;
      Notify(queue, ticket, TARGET_HID_CANCELLED);
    }
  }
}

static void ScheduleRelease(TargetHidQueue *queue)
{
  const TargetHidState *last = queue->lane[1].active_transfer != 0U
                              ? &queue->lane[1].active.state : &queue->committed;
  queue->projected = (TargetHidState){.x = last->x, .y = last->y};
  queue->release_mask = NEUTRAL_REPORTS;
  CancelPending(queue);
}

void TargetHidQueue_Init(TargetHidQueue *queue, TargetHidDone done, void *context)
{
  *queue = (TargetHidQueue){.done = done, .context = context, .release_mask = NEUTRAL_REPORTS};
}

void TargetHidQueue_SetConnected(TargetHidQueue *queue, bool connected)
{
  if (queue->connected == connected) {
    return;
  }
  queue->connected = connected;
  if (connected) {
    return;
  }

  const uint32_t release = queue->release_ticket;
  for (unsigned index = 0; index < 2U; ++index) {
    TargetHidLane *lane = &queue->lane[index];
    const uint32_t active = lane->active_transfer != 0U ? lane->active.ticket : 0U;
    lane->active_transfer = 0U;
    Notify(queue, active, TARGET_HID_CANCELLED);
  }
  memset(queue->release_transfer, 0, sizeof(queue->release_transfer));
  queue->release_ticket = 0U;
  Notify(queue, release, TARGET_HID_CANCELLED);
  ScheduleRelease(queue);
}

void TargetHidQueue_EndSession(TargetHidQueue *queue)
{
  const uint32_t old_release = queue->release_ticket;
  queue->release_ticket = 0U;
  uint32_t active[2];
  for (unsigned index = 0; index < 2U; ++index) {
    TargetHidLane *lane = &queue->lane[index];
    active[index] = lane->active_transfer != 0U ? lane->active.ticket : 0U;
    lane->active.ticket = 0U;
  }
  ScheduleRelease(queue);
  Notify(queue, old_release, TARGET_HID_CANCELLED);
  for (unsigned index = 0; index < 2U; ++index) {
    Notify(queue, active[index], TARGET_HID_CANCELLED);
  }
}

TargetHidStatus TargetHidQueue_Submit(TargetHidQueue *queue, uint16_t opcode,
                                    const uint8_t *payload, size_t length,
                                    uint32_t *ticket)
{
  if (opcode == TARGET_HID_RELEASE_ALL) {
    if (length != 0U) {
      return TARGET_HID_BAD_PAYLOAD;
    }
    if (queue->release_ticket != 0U || queue->release_transfer[0] != 0U ||
        queue->release_transfer[1] != 0U ||
        queue->last_ticket == UINT32_MAX) {
      return TARGET_HID_BUSY;
    }
    queue->release_ticket = ++queue->last_ticket;
    *ticket = queue->release_ticket;
    ScheduleRelease(queue);
    return TARGET_HID_OK;
  }

  if (!queue->connected) {
    return TARGET_HID_NOT_READY;
  }
  TargetHidRequest request = {.state = queue->projected};
  const TargetHidStatus status = TargetHid_Apply(&request.state, opcode, payload, length,
                                                 &request.reports);
  if (status != TARGET_HID_OK) {
    return status;
  }
  TargetHidLane *lane = &queue->lane[request.reports.mask == TARGET_HID_REPORT_KEYBOARD ? 0 : 1];
  if (lane->count == TARGET_HID_QUEUE_DEPTH || queue->last_ticket == UINT32_MAX) {
    return TARGET_HID_BUSY;
  }
  request.ticket = ++queue->last_ticket;
  const size_t index = (lane->head + lane->count) % TARGET_HID_QUEUE_DEPTH;
  lane->pending[index] = request;
  ++lane->count;
  queue->projected = request.state;
  *ticket = request.ticket;
  return TARGET_HID_OK;
}

static void Commit(TargetHidQueue *queue, const TargetHidState *state, uint8_t mask)
{
  if ((mask & TARGET_HID_REPORT_KEYBOARD) != 0U) {
    queue->committed.modifiers = state->modifiers;
    memcpy(queue->committed.keys, state->keys, sizeof(state->keys));
  }
  if ((mask & TARGET_HID_REPORT_MOUSE) != 0U) {
    queue->committed.buttons = state->buttons;
    queue->committed.x = state->x;
    queue->committed.y = state->y;
  }
}

uint64_t TargetHidQueue_TakeAvailable(TargetHidQueue *queue, uint8_t available,
                                    TargetHidReports *reports)
{
  if (!queue->connected || queue->transfer_counter == UINT64_MAX) {
    return 0U;
  }
  for (unsigned index = 0; index < 2U; ++index) {
    const uint8_t mask = (uint8_t)(1U << index);
    if (queue->lane[index].active_transfer != 0U || queue->release_transfer[index] != 0U) {
      available &= (uint8_t)~mask;
    }
    if ((available & queue->release_mask & mask) != 0U) {
      TargetHidState *state = &queue->release_state[index];
      *state = queue->committed;
      (void)TargetHid_Apply(state, TARGET_HID_RELEASE_ALL, NULL, 0, reports);
      reports->mask = mask;
      queue->release_transfer[index] = ++queue->transfer_counter;
      return queue->release_transfer[index];
    }
  }
  for (unsigned index = 0; index < 2U; ++index) {
    const uint8_t mask = (uint8_t)(1U << index);
    TargetHidLane *lane = &queue->lane[index];
    if (lane->count == 0U || (available & mask) == 0U ||
        (queue->release_mask & mask) != 0U) {
      continue;
    }
    lane->active = lane->pending[lane->head];
    lane->head = (uint8_t)((lane->head + 1U) % TARGET_HID_QUEUE_DEPTH);
    --lane->count;
    lane->active_transfer = ++queue->transfer_counter;
    *reports = lane->active.reports;
    return lane->active_transfer;
  }
  return 0U;
}

uint64_t TargetHidQueue_Take(TargetHidQueue *queue, TargetHidReports *reports)
{
  if (queue->lane[0].active_transfer != 0U || queue->lane[1].active_transfer != 0U ||
      queue->release_transfer[0] != 0U || queue->release_transfer[1] != 0U) {
    return 0U;
  }
  return TargetHidQueue_TakeAvailable(queue, NEUTRAL_REPORTS, reports);
}

void TargetHidQueue_Complete(TargetHidQueue *queue, uint64_t transfer, bool delivered)
{
  if (transfer == 0U) {
    return;
  }
  for (unsigned index = 0; index < 2U; ++index) {
    if (queue->release_transfer[index] != transfer) {
      continue;
    }
    queue->release_transfer[index] = 0U;
    const uint32_t ticket = queue->release_ticket;
    if (!delivered) {
      queue->release_ticket = 0U;
      ScheduleRelease(queue);
      Notify(queue, ticket, TARGET_HID_IO_ERROR);
      return;
    }
    const uint8_t mask = (uint8_t)(1U << index);
    Commit(queue, &queue->release_state[index], mask);
    queue->release_mask &= (uint8_t)~mask;
    if (queue->release_mask == 0U) {
      queue->release_ticket = 0U;
      Notify(queue, ticket, TARGET_HID_OK);
    }
    return;
  }
  for (unsigned index = 0; index < 2U; ++index) {
    TargetHidLane *lane = &queue->lane[index];
    if (lane->active_transfer != transfer) {
      continue;
    }
    const uint32_t ticket = lane->active.ticket;
    lane->active_transfer = 0U;
    if (!delivered) {
      ScheduleRelease(queue);
      Notify(queue, ticket, TARGET_HID_IO_ERROR);
      return;
    }
    Commit(queue, &lane->active.state, lane->active.reports.mask);
    Notify(queue, ticket, TARGET_HID_OK);
    return;
  }
}

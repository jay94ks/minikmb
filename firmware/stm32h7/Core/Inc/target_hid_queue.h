#ifndef MINIKMB_TARGET_HID_QUEUE_H
#define MINIKMB_TARGET_HID_QUEUE_H

#include <stdbool.h>
#include "target_hid_report.h"

/* 16 requests per interface, preserving the prior total of 32 pending slots. */
enum { TARGET_HID_QUEUE_DEPTH = 16 };

typedef void (*TargetHidDone)(void *context, uint32_t ticket, TargetHidStatus status);

typedef struct {
  TargetHidState state;
  TargetHidReports reports;
  uint32_t ticket;
} TargetHidRequest;

typedef struct {
  TargetHidRequest pending[TARGET_HID_QUEUE_DEPTH];
  TargetHidRequest active;
  uint64_t active_transfer;
  uint8_t head;
  uint8_t count;
} TargetHidLane;

typedef struct {
  TargetHidLane lane[2];
  TargetHidState committed;
  TargetHidState projected;
  TargetHidDone done;
  void *context;
  uint64_t transfer_counter;
  uint64_t release_transfer[2];
  TargetHidState release_state[2];
  uint32_t last_ticket;
  uint32_t release_ticket;
  uint8_t release_mask;
  bool connected;
} TargetHidQueue;

/*
 * Serialize all calls in the adapter. Completion callbacks must not reenter.
 * A nonzero returned ticket is for the management reply; a transfer ID protects
 * the pending USB operation from stale callbacks after a disconnect.
 */
void TargetHidQueue_Init(TargetHidQueue *queue, TargetHidDone done, void *context);
void TargetHidQueue_SetConnected(TargetHidQueue *queue, bool connected);
void TargetHidQueue_EndSession(TargetHidQueue *queue);

TargetHidStatus TargetHidQueue_Submit(TargetHidQueue *queue, uint16_t opcode,
                                    const uint8_t *payload, size_t length,
                                    uint32_t *ticket);

/* Serialized compatibility API. The caller owns the copied bytes. */
uint64_t TargetHidQueue_Take(TargetHidQueue *queue, TargetHidReports *reports);
/* Each interface retains FIFO order and has its own bounded storage/in-flight ID.
 * A host that does not poll one interface must not block the other. RELEASE_ALL
 * completes only after both neutral reports, but each lane can resume after its
 * own neutral completes. Completion tickets may therefore arrive out of order. */
uint64_t TargetHidQueue_TakeAvailable(TargetHidQueue *queue, uint8_t available,
                                    TargetHidReports *reports);
void TargetHidQueue_Complete(TargetHidQueue *queue, uint64_t transfer, bool delivered);

#endif

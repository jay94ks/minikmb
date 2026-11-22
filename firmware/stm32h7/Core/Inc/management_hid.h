#ifndef MINIKMB_MANAGEMENT_HID_H
#define MINIKMB_MANAGEMENT_HID_H

#include "management_protocol.h"
#include "target_hid_queue.h"

enum { MANAGEMENT_HID_EVENT_DEPTH = 64 };

typedef struct {
  TargetHidQueue queue;
  ManagementHidEvent events[MANAGEMENT_HID_EVENT_DEPTH];
  uint64_t session;
  uint32_t dropped;
  uint8_t head;
  uint8_t count;
} ManagementHid;

/*
 * The adapter serializes access to the bridge, including the backend callbacks
 * invoked by Management and the transfer operations invoked by the target task.
 */
void ManagementHid_Init(ManagementHid *hid);
ManagementHidBackend ManagementHid_Backend(ManagementHid *hid);
void ManagementHid_SetConnected(ManagementHid *hid, bool connected);
uint64_t ManagementHid_TakeReport(ManagementHid *hid, TargetHidReports *reports);
uint64_t ManagementHid_TakeAvailableReport(ManagementHid *hid, uint8_t available,
                                         TargetHidReports *reports);
void ManagementHid_Complete(ManagementHid *hid, uint64_t transfer, bool delivered);

#endif

#include "management_hid.h"

#include <string.h>

static ManagementHid target;
static ManagementHidBackend backend;
static uint64_t next_session;
static uint8_t storage;

static uint64_t NewSession(void)
{
  return ++next_session;
}

static uint8_t Storage(void)
{
  return storage;
}

static uint8_t Target(void)
{
  return target.queue.connected ? 2U : 0U;
}

void Test_Init(bool enable_hid)
{
  Management_Reset();
  ManagementHid_Init(&target);
  backend = ManagementHid_Backend(&target);
  storage = 1U;
  const ManagementPlatform platform = {
    .new_session = NewSession,
    .storage_state = Storage,
    .target_state = Target,
    .uid = {0xCD, 0x12},
    .hid = enable_hid ? &backend : NULL,
  };
  Management_Init(&platform, 0U);
}

void Test_Storage(uint8_t state)
{
  storage = state;
}

void Test_SetConnected(bool connected)
{
  ManagementHid_SetConnected(&target, connected);
}

uint64_t Test_TakeReport(uint8_t *bytes)
{
  TargetHidReports reports;
  const uint64_t transfer = ManagementHid_TakeReport(&target, &reports);
  if (transfer != 0U) {
    bytes[0] = reports.mask;
    memcpy(bytes + 1, reports.keyboard, sizeof(reports.keyboard));
    memcpy(bytes + 1 + sizeof(reports.keyboard), reports.mouse, sizeof(reports.mouse));
  }
  return transfer;
}

void Test_Complete(uint64_t transfer, bool delivered)
{
  ManagementHid_Complete(&target, transfer, delivered);
}

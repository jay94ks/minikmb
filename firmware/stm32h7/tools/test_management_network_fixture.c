#include "management_protocol.h"
#include <string.h>

static uint8_t rx[MANAGEMENT_ETH_MAX], last[MANAGEMENT_ETH_MAX];
static uint16_t rx_length, last_length;
static bool configured, enabled, space;
static uint32_t commits;
static uint64_t next_session;
static uint8_t storage;

static uint64_t NewSession(void) { return ++next_session; }
static uint8_t Storage(void) { return storage; }
static uint8_t Target(void) { return configured ? 1 : 0; }
static bool Configured(void) { return configured; }
static bool Link(void) { return configured && enabled; }
static bool SetEnabled(bool on)
{
  if (on && !configured) return false;
  enabled = on;
  if (!on) rx_length = 0;
  return true;
}
static bool Space(void) { return space; }
static bool Submit(const uint8_t *data, uint16_t length)
{
  if (!space || !Link()) return false;
  ++commits;
  last_length = length;
  memcpy(last, data, length);
  return true;
}
static uint16_t TakeRx(uint8_t *data)
{
  const uint16_t length = rx_length;
  memcpy(data, rx, length);
  rx_length = 0;
  return length;
}
static uint32_t Drops(void) { return 0; }

void TestInit(void)
{
  static const ManagementNetworkBackend network = {
    .set_enabled = SetEnabled, .configured = Configured, .link_up = Link, .tx_available = Space,
    .submit = Submit, .take_rx = TakeRx, .rx_dropped = Drops, .tx_dropped = Drops,
  };
  const ManagementPlatform platform = {
    .new_session = NewSession, .target_state = Target, .storage_state = Storage,
    .uid = {0xCD, 0x12}, .network = &network,
  };
  storage = 1;
  configured = false;
  space = true;
  Management_Init(&platform, 0);
}

void TestTarget(bool online) { configured = online; }
void TestUpload(bool active) { storage = active ? 2 : 1; }
void TestSpace(bool available) { space = available; }
void TestRx(const uint8_t *data, uint16_t length)
{
  memcpy(rx, data, length);
  rx_length = length;
}
uint32_t TestCommits(void) { return commits; }
uint16_t TestLast(uint8_t *data)
{
  memcpy(data, last, last_length);
  return last_length;
}

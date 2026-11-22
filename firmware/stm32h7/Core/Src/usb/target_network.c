#include "target_features.h"
#if MINIKMB_TARGET_NCM

#include "target_tusb_namespace.h"
#include "target_network.h"
#include "tusb.h"
#include "FreeRTOS.h"
#include "task.h"

#include <string.h>

enum { NETWORK_QUEUE = 16 };

typedef struct {
  uint8_t data[MANAGEMENT_ETH_MAX];
  uint16_t length;
} EthernetFrame;

static EthernetFrame tx_frames[NETWORK_QUEUE] __attribute__((section(".d2_network"), aligned(32)));
static EthernetFrame rx_frames[NETWORK_QUEUE] __attribute__((section(".d2_network"), aligned(32)));
static EthernetFrame transfer __attribute__((section(".d2_network"), aligned(32)));
static struct { uint8_t head, length; } tx, rx;
static bool configured, wanted, data_active;
static uint32_t tx_dropped, rx_dropped, generation;

static void ClearQueues(void)
{
  tx_dropped += tx.length;
  rx_dropped += rx.length;
  tx.length = rx.length = 0;
  tx.head = rx.head = 0;
  ++generation;
}

static bool SetEnabled(bool enable)
{
  taskENTER_CRITICAL();
  const bool allowed = !enable || configured;
  if (allowed && enable != wanted) {
    wanted = enable;
    data_active = false;
    ClearQueues();
  }
  taskEXIT_CRITICAL();
  return allowed;
}

static bool Configured(void)
{
  taskENTER_CRITICAL();
  const bool value = configured;
  taskEXIT_CRITICAL();
  return value;
}

static bool LinkUp(void)
{
  taskENTER_CRITICAL();
  const bool value = data_active;
  taskEXIT_CRITICAL();
  return value;
}

static bool TxAvailable(void)
{
  taskENTER_CRITICAL();
  const bool available = data_active && tx.length < NETWORK_QUEUE;
  taskEXIT_CRITICAL();
  return available;
}

static bool Submit(const uint8_t *data, uint16_t length)
{
  taskENTER_CRITICAL();
  const bool available = data_active && tx.length < NETWORK_QUEUE;
  if (available) {
    EthernetFrame *slot = &tx_frames[(tx.head + tx.length) % NETWORK_QUEUE];
    memcpy(slot->data, data, length);
    slot->length = length;
    ++tx.length;
  } else {
    ++tx_dropped;
  }
  taskEXIT_CRITICAL();
  return available;
}

static uint16_t TakeRx(uint8_t data[MANAGEMENT_ETH_MAX])
{
  uint16_t length = 0;
  taskENTER_CRITICAL();
  if (data_active && rx.length != 0) {
    const EthernetFrame *slot = &rx_frames[rx.head];
    length = slot->length;
    memcpy(data, slot->data, length);
    rx.head = (uint8_t)((rx.head + 1U) % NETWORK_QUEUE);
    --rx.length;
  }
  taskEXIT_CRITICAL();
  return length;
}

static uint32_t RxDropped(void)
{
  taskENTER_CRITICAL();
  const uint32_t value = rx_dropped;
  taskEXIT_CRITICAL();
  return value;
}

static uint32_t TxDropped(void)
{
  taskENTER_CRITICAL();
  const uint32_t value = tx_dropped;
  taskEXIT_CRITICAL();
  return value;
}

bool tud_network_recv_cb(const uint8_t *data, uint16_t length)
{
  taskENTER_CRITICAL();
  if (data_active && length >= MANAGEMENT_ETH_MIN && length <= MANAGEMENT_ETH_MAX && rx.length < NETWORK_QUEUE) {
    EthernetFrame *slot = &rx_frames[(rx.head + rx.length) % NETWORK_QUEUE];
    memcpy(slot->data, data, length);
    slot->length = length;
    ++rx.length;
  } else {
    ++rx_dropped;
  }
  taskEXIT_CRITICAL();
  tud_network_recv_renew();
  return true;
}

uint16_t tud_network_xmit_cb(uint8_t *dst, void *ref, uint16_t bytes)
{
  memcpy(dst, ref, bytes);
  return bytes;
}

void TargetNetwork_Poll(bool mounted, bool reset)
{
  taskENTER_CRITICAL();
  configured = mounted;
  if (reset || !mounted) {
    if (wanted) ClearQueues();
    data_active = wanted = false;
  }
  const bool enable = wanted;
  taskEXIT_CRITICAL();
  if (!mounted) return;

  tud_network_link_state(1, enable);
  const bool usable = enable && tud_network_data_active();
  taskENTER_CRITICAL();
  if (data_active && !usable) ClearQueues();
  data_active = wanted && usable;
  const uint32_t epoch = generation;
  const uint16_t bytes = data_active && tx.length != 0 ? tx_frames[tx.head].length : 0;
  taskEXIT_CRITICAL();
  if (!bytes || !tud_network_can_xmit(bytes)) return;

  taskENTER_CRITICAL();
  const bool valid = generation == epoch && data_active && tx.length != 0;
  if (valid) {
    memcpy(transfer.data, tx_frames[tx.head].data, bytes);
    tx.head = (uint8_t)((tx.head + 1U) % NETWORK_QUEUE);
    --tx.length;
  }
  taskEXIT_CRITICAL();
  if (valid) tud_network_xmit(transfer.data, bytes);
}

const ManagementNetworkBackend *TargetNetwork_Backend(void)
{
  static const ManagementNetworkBackend backend = {
    .set_enabled = SetEnabled, .configured = Configured, .link_up = LinkUp, .tx_available = TxAvailable,
    .submit = Submit, .take_rx = TakeRx, .rx_dropped = RxDropped, .tx_dropped = TxDropped,
  };
  return &backend;
}

#endif

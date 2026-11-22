#include <assert.h>
#include <stdbool.h>
#include <stdio.h>

#include "device/usbd.c"

static unsigned clear_calls;
static bool hardware_armed, abort_requested, cancelled;

void dcd_edpt_clear_stall(uint8_t rhport, uint8_t ep_addr) {
  assert(rhport == 1);
  ++clear_calls;
  cancelled = false;
#ifdef MINIKMB_TUSB_TARGET_INSTANCE
  const bool bulk_in = ep_addr == 0x82 || ep_addr == 0x85 || ep_addr == 0x87;
  if (bulk_in && hardware_armed && abort_requested) {
    hardware_armed = false;
    cancelled = true;
  }
#else
  (void)ep_addr;
#endif
}

#ifdef MINIKMB_TUSB_TARGET_INSTANCE
bool target_dcd_edpt_clear_stall_cancelled(uint8_t rhport, uint8_t ep_addr) {
  dcd_edpt_clear_stall(rhport, ep_addr);
  return cancelled;
}
#endif

int main(void) {
  _usbd_rhport = 1;
  const uint8_t addresses[] = {0x83, 0x84, 0x02, 0x82, 0x85, 0x87};
  const uint8_t states[] = {
    0, TU_EDPT_STATE_STALLED | TU_EDPT_STATE_BUSY,
    TU_EDPT_STATE_CLAIMED | TU_EDPT_STATE_BUSY,
    TU_EDPT_STATE_STALLED | TU_EDPT_STATE_BUSY | TU_EDPT_STATE_CLAIMED
  };
  unsigned failures = 0, cases = 0, aborted = 0;
  for (unsigned i = 0; i < sizeof(addresses); ++i) {
    const bool bulk_in = i >= 3;
    for (unsigned j = 0; j < sizeof(states); ++j) {
      const bool claimed = (states[j] & TU_EDPT_STATE_CLAIMED) != 0;
      /* An unarmed but still claimed transfer can have a completion queued. */
      for (unsigned armed = 0; armed <= (unsigned)claimed; ++armed) {
        for (unsigned abort = 0; abort <= (unsigned)bulk_in; ++abort) {
          const uint8_t number = tu_edpt_number(addresses[i]);
          const uint8_t direction = tu_edpt_dir(addresses[i]);
          _usbd_dev.ep_status[number][direction] = states[j];
          hardware_armed = armed != 0;
          abort_requested = abort != 0;
          usbd_edpt_clear_stall(1, addresses[i]);
          const uint8_t after = _usbd_dev.ep_status[number][direction];
          ++cases;
          aborted += (unsigned)cancelled;
          if (((after & TU_EDPT_STATE_CLAIMED) != 0) != (claimed && !cancelled) ||
              (after & (TU_EDPT_STATE_STALLED | TU_EDPT_STATE_BUSY)) != 0) {
            printf("FAIL: ep=%02x initial=%02x after=%02x armed=%u abort=%u cancelled=%u\n",
                   addresses[i], states[j], after, armed, abort, (unsigned)cancelled);
            ++failures;
          }
        }
      }
    }
  }
  assert(cases == 54 && clear_calls == cases);
#ifdef MINIKMB_TUSB_TARGET_INSTANCE
  assert(aborted == 6);
#else
  assert(aborted == 0);
#endif
  if (failures != 0) return 2;
  printf("PASS: %u ownership cases, %u actual cancellations\n", cases, aborted);
  return 0;
}

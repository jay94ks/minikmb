#include "target_hid_report.h"

#include <assert.h>
#include <stdio.h>
#include <string.h>

static void Keyboard(void)
{
  TargetHidState state = {0};
  TargetHidReports reports = {0};
  const uint8_t shift[] = {0xE1, 0};
  const uint8_t unsupported[] = {0x03, 0};
  const uint8_t a[] = {0x04, 0};
  const uint8_t seventh[] = {0x0A, 0};

  for (uint8_t key = 0x04; key <= 0x09; ++key) {
    assert(TargetHid_Apply(&state, 0x0101, (const uint8_t[]){key, 0}, 2, &reports) == TARGET_HID_OK);
  }
  assert(TargetHid_Apply(&state, 0x0101, shift, 2, &reports) == TARGET_HID_OK);
  assert(TargetHid_Apply(&state, 0x0101, a, 2, &reports) == TARGET_HID_OK);
  assert(reports.mask == TARGET_HID_REPORT_KEYBOARD);
  assert(memcmp(reports.keyboard, (uint8_t[]){2, 0, 4, 5, 6, 7, 8, 9}, 8) == 0);
  assert(TargetHid_Apply(&state, 0x0101, seventh, 2, &reports) == TARGET_HID_BUSY);
  assert(TargetHid_Apply(&state, 0x0101, unsupported, 2, &reports) == TARGET_HID_UNSUPPORTED);
  assert(TargetHid_Apply(&state, 0x0101, a, 1, &reports) == TARGET_HID_BAD_PAYLOAD);
  assert(TargetHid_Apply(&state, 0x0101, NULL, 2, &reports) == TARGET_HID_BAD_PAYLOAD);
  assert(memcmp(reports.keyboard, (uint8_t[]){2, 0, 4, 5, 6, 7, 8, 9}, 8) == 0);
  assert(state.modifiers == 2 && memcmp(state.keys, (uint8_t[]){4, 5, 6, 7, 8, 9}, 6) == 0);

  assert(TargetHid_Apply(&state, 0x0102, a, 2, &reports) == TARGET_HID_OK);
  assert(TargetHid_Apply(&state, 0x0102, a, 2, &reports) == TARGET_HID_OK);
  assert(TargetHid_Apply(&state, 0x0101, seventh, 2, &reports) == TARGET_HID_OK);
  assert(TargetHid_Apply(&state, 0x0102, shift, 2, &reports) == TARGET_HID_OK);
  assert(memcmp(reports.keyboard, (uint8_t[]){0, 0, 10, 5, 6, 7, 8, 9}, 8) == 0);
}

static void MouseAndRelease(void)
{
  TargetHidState state = {0};
  TargetHidReports reports = {0};
  const uint8_t coordinates[] = {0xFF, 0x7F, 0x34, 0x12};
  const uint8_t excess_coordinates[] = {0, 0, 0, 0x80};
  const uint8_t forward[] = {5};
  const uint8_t invalid_button[] = {6};
  const uint8_t wheel[] = {0x81, 0xFF, 0x7F, 0x00};
  const uint8_t excess_wheel[] = {0x80, 0xFF, 0, 0};
  const uint8_t excess_pan[] = {0, 0, 0x80, 0};

  assert(TargetHid_Apply(&state, 0x0103, coordinates, 4, &reports) == TARGET_HID_OK);
  assert(TargetHid_Apply(&state, 0x0103, excess_coordinates, 4, &reports) == TARGET_HID_RANGE);
  assert(TargetHid_Apply(&state, 0x0104, invalid_button, 1, &reports) == TARGET_HID_RANGE);
  assert(state.x == 32767 && state.y == 0x1234 && state.buttons == 0);
  assert(TargetHid_Apply(&state, 0x0104, forward, 1, &reports) == TARGET_HID_OK);
  assert(TargetHid_Apply(&state, 0x0104, forward, 1, &reports) == TARGET_HID_OK);
  assert(TargetHid_Apply(&state, 0x0106, wheel, 4, &reports) == TARGET_HID_OK);
  assert(reports.mask == TARGET_HID_REPORT_MOUSE);
  assert(memcmp(reports.mouse, (uint8_t[]){16, 0xFF, 0x7F, 0x34, 0x12, 0x81, 0x7F}, 7) == 0);
  assert(TargetHid_Apply(&state, 0x0106, excess_wheel, 4, &reports) == TARGET_HID_RANGE);
  assert(TargetHid_Apply(&state, 0x0106, excess_pan, 4, &reports) == TARGET_HID_RANGE);
  assert(TargetHid_Apply(&state, 0x0105, forward, 1, &reports) == TARGET_HID_OK);
  assert(memcmp(reports.mouse, (uint8_t[]){0, 0xFF, 0x7F, 0x34, 0x12, 0, 0}, 7) == 0);
  assert(TargetHid_Apply(&state, 0x0180, NULL, 0, &reports) == TARGET_HID_UNSUPPORTED);
  assert(TargetHid_Apply(&state, 0x0107, forward, 1, &reports) == TARGET_HID_BAD_PAYLOAD);

  for (unsigned i = 0; i < 1000; ++i) {
    const uint8_t key[] = {(uint8_t)(4U + i % 161U), 0};
    const uint8_t button[] = {(uint8_t)(1U + i % 5U)};
    assert(TargetHid_Apply(&state, 0x0101, key, 2, &reports) == TARGET_HID_OK);
    assert(TargetHid_Apply(&state, 0x0104, button, 1, &reports) == TARGET_HID_OK);
    assert(TargetHid_Apply(&state, 0x0107, NULL, 0, &reports) == TARGET_HID_OK);
    assert(reports.mask == (TARGET_HID_REPORT_KEYBOARD | TARGET_HID_REPORT_MOUSE));
    assert(memcmp(reports.keyboard, (uint8_t[8]){0}, 8) == 0);
    assert(memcmp(reports.mouse, (uint8_t[]){0, 0xFF, 0x7F, 0x34, 0x12, 0, 0}, 7) == 0);
  }
}

int main(void)
{
  Keyboard();
  MouseAndRelease();
  puts("HID report validation passed, including 1000 simulated release sequences.");
  return 0;
}

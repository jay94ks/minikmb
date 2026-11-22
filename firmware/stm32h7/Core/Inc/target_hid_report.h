#ifndef MINIKMB_TARGET_HID_REPORT_H
#define MINIKMB_TARGET_HID_REPORT_H

#include <stddef.h>
#include <stdint.h>

enum {
  TARGET_HID_KEY_COUNT = 6,
  TARGET_HID_KEYBOARD_BYTES = 8,
  TARGET_HID_MOUSE_BYTES = 7,
  TARGET_HID_REPORT_KEYBOARD = 1,
  TARGET_HID_REPORT_MOUSE = 2,
  TARGET_HID_RELEASE_ALL = 0x0107,
};

typedef enum {
  TARGET_HID_OK = 0,
  TARGET_HID_UNSUPPORTED = 1,
  TARGET_HID_BAD_PAYLOAD = 2,
  TARGET_HID_BUSY = 4,
  TARGET_HID_NOT_READY = 9,
  TARGET_HID_RANGE = 10,
  TARGET_HID_IO_ERROR = 11,
  TARGET_HID_CANCELLED = 13,
} TargetHidStatus;

typedef struct {
  uint16_t x;
  uint16_t y;
  uint8_t modifiers;
  uint8_t keys[TARGET_HID_KEY_COUNT];
  uint8_t buttons;
} TargetHidState;

typedef struct {
  uint8_t mask;
  uint8_t keyboard[TARGET_HID_KEYBOARD_BYTES];
  uint8_t mouse[TARGET_HID_MOUSE_BYTES];
} TargetHidReports;

/*
 * A zero-initialized state has no keys or buttons pressed. Apply to a copy when
 * validating queue admission. Sequence replay and USB delivery belong to the caller.
 * Errors leave the state and reports unchanged. Wheel and pan are one-shot fields.
 */
TargetHidStatus TargetHid_Apply(TargetHidState *state, uint16_t opcode,
                               const uint8_t *payload, size_t length,
                               TargetHidReports *reports);

#endif

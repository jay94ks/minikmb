#include "target_hid_report.h"

#include <stdbool.h>
#include <string.h>

enum {
  KEY_DOWN = 0x0101,
  KEY_UP = 0x0102,
  MOUSE_MOVE = 0x0103,
  BUTTON_DOWN = 0x0104,
  BUTTON_UP = 0x0105,
  WHEEL = 0x0106,
};

static uint16_t ReadLe16(const uint8_t *bytes)
{
  return (uint16_t)((uint16_t)bytes[0] | ((uint16_t)bytes[1] << 8));
}

static int32_t ReadSignedLe16(const uint8_t *bytes)
{
  const uint16_t value = ReadLe16(bytes);
  return value <= INT16_MAX ? (int32_t)value : (int32_t)value - 65536;
}

static uint8_t SetBit(uint8_t value, uint8_t mask, bool set)
{
  return set ? (uint8_t)(value | mask) : (uint8_t)(value & (uint8_t)~mask);
}

static TargetHidStatus SetKey(TargetHidState *state, uint16_t usage, bool pressed)
{
  if (usage >= 0xE0U && usage <= 0xE7U) {
    const uint8_t mask = (uint8_t)(1U << (usage - 0xE0U));
    state->modifiers = SetBit(state->modifiers, mask, pressed);
    return TARGET_HID_OK;
  }
  if (usage < 0x04U || usage > 0xA4U) {
    return TARGET_HID_UNSUPPORTED;
  }

  size_t empty = TARGET_HID_KEY_COUNT;
  for (size_t i = 0; i < TARGET_HID_KEY_COUNT; ++i) {
    if (state->keys[i] == usage) {
      if (!pressed) {
        state->keys[i] = 0U;
      }
      return TARGET_HID_OK;
    }
    if (state->keys[i] == 0U && empty == TARGET_HID_KEY_COUNT) {
      empty = i;
    }
  }
  if (pressed) {
    if (empty == TARGET_HID_KEY_COUNT) {
      return TARGET_HID_BUSY;
    }
    state->keys[empty] = (uint8_t)usage;
  }
  return TARGET_HID_OK;
}

static void EncodeReports(const TargetHidState *state, uint8_t mask,
                          int32_t wheel, int32_t pan, TargetHidReports *reports)
{
  *reports = (TargetHidReports){.mask = mask};
  if ((mask & TARGET_HID_REPORT_KEYBOARD) != 0U) {
    reports->keyboard[0] = state->modifiers;
    memcpy(&reports->keyboard[2], state->keys, sizeof(state->keys));
  }
  if ((mask & TARGET_HID_REPORT_MOUSE) != 0U) {
    reports->mouse[0] = state->buttons;
    reports->mouse[1] = (uint8_t)state->x;
    reports->mouse[2] = (uint8_t)(state->x >> 8);
    reports->mouse[3] = (uint8_t)state->y;
    reports->mouse[4] = (uint8_t)(state->y >> 8);
    reports->mouse[5] = (uint8_t)wheel;
    reports->mouse[6] = (uint8_t)pan;
  }
}

TargetHidStatus TargetHid_Apply(TargetHidState *state, uint16_t opcode,
                               const uint8_t *payload, size_t length,
                               TargetHidReports *reports)
{
  static const uint8_t lengths[] = {2U, 2U, 4U, 1U, 1U, 4U, 0U};
  if (opcode < KEY_DOWN || opcode > TARGET_HID_RELEASE_ALL) {
    return TARGET_HID_UNSUPPORTED;
  }
  if (length != lengths[opcode - KEY_DOWN] || (length != 0U && payload == NULL)) {
    return TARGET_HID_BAD_PAYLOAD;
  }

  uint8_t mask = TARGET_HID_REPORT_MOUSE;
  int32_t wheel = 0;
  int32_t pan = 0;
  switch (opcode) {
    case KEY_DOWN:
    case KEY_UP: {
      const TargetHidStatus status = SetKey(state, ReadLe16(payload), opcode == KEY_DOWN);
      if (status != TARGET_HID_OK) {
        return status;
      }
      mask = TARGET_HID_REPORT_KEYBOARD;
      break;
    }
    case MOUSE_MOVE: {
      const uint16_t x = ReadLe16(payload);
      const uint16_t y = ReadLe16(payload + 2);
      if (x > 32767U || y > 32767U) {
        return TARGET_HID_RANGE;
      }
      state->x = x;
      state->y = y;
      break;
    }
    case BUTTON_DOWN:
    case BUTTON_UP:
      if (payload[0] < 1U || payload[0] > 5U) {
        return TARGET_HID_RANGE;
      }
      state->buttons = SetBit(state->buttons, (uint8_t)(1U << (payload[0] - 1U)),
                              opcode == BUTTON_DOWN);
      break;
    case WHEEL:
      wheel = ReadSignedLe16(payload);
      pan = ReadSignedLe16(payload + 2);
      if (wheel < -127 || wheel > 127 || pan < -127 || pan > 127) {
        return TARGET_HID_RANGE;
      }
      break;
    case TARGET_HID_RELEASE_ALL:
      /* Releasing buttons must not reset the last absolute coordinates. */
      *state = (TargetHidState){.x = state->x, .y = state->y};
      mask = TARGET_HID_REPORT_KEYBOARD | TARGET_HID_REPORT_MOUSE;
      break;
  }
  EncodeReports(state, mask, wheel, pan, reports);
  return TARGET_HID_OK;
}

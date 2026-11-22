#ifndef KBM_SLAVE_COMMAND_H
#define KBM_SLAVE_COMMAND_H

#include "protocol.h"
#include "serial_protocol.h"
#include "ethernet_protocol.h"
#include "link_protocol.h"
#include "reboot_protocol.h"
#include "disk_protocol.h"
#include "wake_protocol.h"
#include "hid_controls.h"
#include "boot_protocol.h"
#include "hid_diagnostics.h"

#include <stdbool.h>
#include <stdint.h>

typedef enum {
    KBM_OP_PING = 0x00,
    KBM_OP_KEY_DOWN = 0x10,
    KBM_OP_KEY_UP = 0x11,
    KBM_OP_MOUSE_MOVE = 0x20,
    KBM_OP_MOUSE_DOWN = 0x21,
    KBM_OP_MOUSE_UP = 0x22,
    KBM_OP_RELEASE_ALL = 0x7f,
} kbm_opcode_t;

typedef enum {
    KBM_STATUS_OK = 0x00,
    KBM_STATUS_BAD_OPCODE = 0x01,
    KBM_STATUS_BAD_LENGTH = 0x02,
    KBM_STATUS_BAD_VALUE = 0x03,
    KBM_STATUS_BAD_ESCAPE = 0x04,
    KBM_STATUS_BAD_ETX = 0x05,
    KBM_STATUS_BAD_CHECKSUM = 0x06,
    KBM_STATUS_SERIAL_NOT_READY = 0x07,
    KBM_STATUS_QUEUE_FULL = 0x08,
} kbm_status_t;

kbm_status_t kbm_command_validate(const kbm_packet_t *packet);

#endif


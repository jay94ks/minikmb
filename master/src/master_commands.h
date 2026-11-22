#ifndef KBM_MASTER_COMMANDS_H
#define KBM_MASTER_COMMANDS_H

#include "protocol.h"
#include "serial_protocol.h"
#include "ethernet_protocol.h"
#include "boot_protocol.h"

#include <stdbool.h>
#include <stdint.h>

#define KBM_PROTOCOL_VERSION 3u
#define KBM_ONLINE_MASK_ALL 0x03u
#define KBM_ROUTE_HEADER_SIZE 4u
#define KBM_ROUTE_RESPONSE_HEADER_SIZE 5u

typedef enum {
    KBM_OP_PING = 0x00,
    KBM_OP_STATUS = 0x71,
    KBM_OP_ROUTE = 0x73,
    KBM_OP_NATIVE_MODE = 0x77,
    KBM_EVENT_ONLINE_CHANGED = 0xf2,
} kbm_master_opcode_t;

typedef enum {
    KBM_STATUS_OK = 0x00,
    KBM_STATUS_UNKNOWN_OPCODE = 0x01,
    KBM_STATUS_BAD_LENGTH = 0x02,
    KBM_STATUS_BAD_VALUE = 0x03,
    KBM_STATUS_BAD_ESCAPE = 0x04,
    KBM_STATUS_BAD_ETX = 0x05,
    KBM_STATUS_BAD_CHECKSUM = 0x06,
    KBM_STATUS_TARGET_REQUIRED = 0x07,
    KBM_STATUS_QUEUE_FULL = 0x08,
    KBM_STATUS_UART_TIMEOUT = 0x09,
    KBM_STATUS_MODE_CONFLICT = 0x0a,
    KBM_STATUS_SLAVE_RESTARTED = KBM_BRIDGE_SLAVE_RESTARTED,
} kbm_master_status_t;

typedef struct {
    uint8_t port;
    uint16_t sequence;
    kbm_packet_t packet;
} kbm_bridge_request_t;

#ifdef __cplusplus
extern "C" {
#endif
bool kbm_master_is_local(uint8_t op);
kbm_master_status_t kbm_master_validate_local(const kbm_packet_t *packet);
kbm_master_status_t kbm_master_decode_route(const kbm_packet_t *packet,
                                           kbm_bridge_request_t *request);
void kbm_master_wrap_response(uint8_t port, uint16_t sequence,
                              kbm_master_status_t status,
                              const kbm_packet_t *slave_response,
                              kbm_packet_t *response);
#ifdef __cplusplus
}
#endif

#endif


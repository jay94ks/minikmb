#include "master_commands.h"
#include "ethernet_protocol.h"

#include <string.h>

bool kbm_master_is_local(uint8_t op) {
    return op == KBM_OP_STATUS || op == KBM_OP_NATIVE_MODE;
}

kbm_master_status_t kbm_master_validate_local(const kbm_packet_t *packet) {
    if (packet->op == KBM_OP_NATIVE_MODE) {
        if (packet->len == 0) return KBM_STATUS_OK;
        if (packet->len != 2) return KBM_STATUS_BAD_LENGTH;
        return packet->payload[0] <= 1 && packet->payload[1] <= 1
            ? KBM_STATUS_OK : KBM_STATUS_BAD_VALUE;
    }
    if (packet->op == KBM_OP_STATUS) {
        return packet->len == 0 ? KBM_STATUS_OK : KBM_STATUS_BAD_LENGTH;
    }
    return KBM_STATUS_UNKNOWN_OPCODE;
}

kbm_master_status_t kbm_master_decode_route(const kbm_packet_t *packet,
                                           kbm_bridge_request_t *request) {
    if (packet->op != KBM_OP_ROUTE) return KBM_STATUS_UNKNOWN_OPCODE;
    if (packet->len < KBM_ROUTE_HEADER_SIZE || packet->len>KBM_MAX_PAYLOAD) return KBM_STATUS_BAD_LENGTH;
    if (packet->payload[0] > 1) return KBM_STATUS_BAD_VALUE;
    const uint8_t op = packet->payload[3];
    // Never route master commands, responses, or the removed SELECT opcode.
    if ((op >= 0x70u && op != 0x7fu) || op == KBM_SERIAL_OP_RX_ACK || op == KBM_NET_CREDIT || op==KBM_BOOT_ACK) {
        return KBM_STATUS_BAD_VALUE;
    }
    request->port = packet->payload[0];
    request->sequence = (uint16_t)packet->payload[1] |
                        ((uint16_t)packet->payload[2] << 8u);
    request->packet.op = op;
    request->packet.len = (uint8_t)(packet->len - KBM_ROUTE_HEADER_SIZE);
    memcpy(request->packet.payload, &packet->payload[KBM_ROUTE_HEADER_SIZE],
           request->packet.len);
    return KBM_STATUS_OK;
}

void kbm_master_wrap_response(uint8_t port, uint16_t sequence,
                              kbm_master_status_t status,
                              const kbm_packet_t *slave_response,
                              kbm_packet_t *response) {
    response->op = (uint8_t)(KBM_OP_ROUTE | 0x80u);
    response->len = KBM_ROUTE_RESPONSE_HEADER_SIZE;
    response->payload[0] = port;
    response->payload[1] = (uint8_t)sequence;
    response->payload[2] = (uint8_t)(sequence >> 8u);
    response->payload[3] = (uint8_t)status;
    response->payload[4] = slave_response ? slave_response->op : 0;
    if (status == KBM_STATUS_OK && slave_response) {
        if (slave_response->len > KBM_MAX_PAYLOAD - KBM_ROUTE_RESPONSE_HEADER_SIZE) {
            response->payload[3] = KBM_STATUS_BAD_LENGTH;
            return;
        }
        memcpy(&response->payload[KBM_ROUTE_RESPONSE_HEADER_SIZE],
               slave_response->payload, slave_response->len);
        response->len = (uint8_t)(response->len + slave_response->len);
    }
}


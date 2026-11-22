#include "command.h"

kbm_status_t kbm_command_validate(const kbm_packet_t *packet) {
    switch (packet->op) {
        case KBM_BOOT_ACK:
            return kbm_boot_ack_valid(packet->payload,packet->len)?KBM_STATUS_OK:KBM_STATUS_BAD_VALUE;
        case KBM_DISK_CONFIG:
            return kbm_disk_config_valid(packet->payload,packet->len) ? KBM_STATUS_OK : KBM_STATUS_BAD_VALUE;
        case KBM_DISK_DATA:
            return kbm_disk_data_valid(packet->payload,packet->len) ? KBM_STATUS_OK : KBM_STATUS_BAD_VALUE;
        case KBM_DISK_FAIL:
            return packet->len==12 ? KBM_STATUS_OK : KBM_STATUS_BAD_LENGTH;
        case KBM_DISK_STATUS:
            return packet->len==0 ? KBM_STATUS_OK : KBM_STATUS_BAD_LENGTH;
        case KBM_REBOOT_OP:
            if (packet->len != KBM_REBOOT_MAGIC_SIZE) return KBM_STATUS_BAD_LENGTH;
            return kbm_reboot_valid(packet->payload, packet->len) ? KBM_STATUS_OK : KBM_STATUS_BAD_VALUE;
        case KBM_LINK_BAUD:
            return packet->len == 6 && kbm_link_lease_valid(packet->payload)
                ? KBM_STATUS_OK : KBM_STATUS_BAD_VALUE;
        case KBM_LINK_ECHO:
            return packet->len >= 1 && packet->len <= 248 ? KBM_STATUS_OK : KBM_STATUS_BAD_LENGTH;
        case KBM_OP_PING:
        case KBM_HID_DIAGNOSTICS_STATUS:
        case KBM_WAKE_OP:
        case KBM_OP_RELEASE_ALL:
        case KBM_SERIAL_OP_STATUS:
        case KBM_NET_STATUS:
            return packet->len == 0 ? KBM_STATUS_OK : KBM_STATUS_BAD_LENGTH;

        case KBM_SERIAL_OP_WRITE:
            return packet->len >= 1 && packet->len <= 251
                       ? KBM_STATUS_OK : KBM_STATUS_BAD_LENGTH;
        case KBM_SERIAL_OP_RX_ACK:
            return packet->len == 2 ? KBM_STATUS_OK : KBM_STATUS_BAD_LENGTH;
        case KBM_NET_CREDIT:
            return packet->len == 4 ? KBM_STATUS_OK : KBM_STATUS_BAD_LENGTH;
        case KBM_NET_ENABLE:
            return packet->len == 1 && packet->payload[0] <= 1 ? KBM_STATUS_OK : KBM_STATUS_BAD_VALUE;
        case KBM_NET_WRITE:
            return packet->len > KBM_NET_HEADER && packet->len <= KBM_NET_HEADER + KBM_NET_CHUNK
                ? KBM_STATUS_OK : KBM_STATUS_BAD_LENGTH;

        case KBM_SYSTEM_DOWN:
        case KBM_SYSTEM_UP:
            if(packet->len!=1) return KBM_STATUS_BAD_LENGTH;
            return kbm_hid_system_index(packet->payload[0])>=0?KBM_STATUS_OK:KBM_STATUS_BAD_VALUE;
        case KBM_OP_KEY_DOWN:
        case KBM_OP_KEY_UP:
            if (packet->len != 1) {
                return KBM_STATUS_BAD_LENGTH;
            }
            return packet->payload[0] != 0
                       ? KBM_STATUS_OK
                       : KBM_STATUS_BAD_VALUE;

        case KBM_OP_MOUSE_MOVE:
            if (packet->len != 6) {
                return KBM_STATUS_BAD_LENGTH;
            }
            // X/Y use the absolute HID logical range 0..32767. Wheel and pan
            // remain signed relative values in the range -127..127.
            if ((packet->payload[1] & 0x80u) != 0 ||
                (packet->payload[3] & 0x80u) != 0 ||
                packet->payload[4] == 0x80 ||
                packet->payload[5] == 0x80) {
                return KBM_STATUS_BAD_VALUE;
            }
            return KBM_STATUS_OK;

        case KBM_OP_MOUSE_DOWN:
        case KBM_OP_MOUSE_UP:
            if (packet->len != 1) {
                return KBM_STATUS_BAD_LENGTH;
            }
            if (packet->payload[0] == 0 ||
                (packet->payload[0] & 0xe0u) != 0) {
                return KBM_STATUS_BAD_VALUE;
            }
            return KBM_STATUS_OK;

        default:
            return KBM_STATUS_BAD_OPCODE;
    }
}


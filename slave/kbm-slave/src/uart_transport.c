#include "uart_transport.h"
#include "app_state.h"
#include "command.h"
#include "protocol.h"
#include "hardware/gpio.h"
#include "hardware/uart.h"
#include "uart_dma_transport.h"
#include "hardware/watchdog.h"
#include "reboot_guard.h"
#include "wake_bridge.h"
#include "pico/stdlib.h"
#include "pico/rand.h"
#include <stddef.h>

#ifndef KBM_UART_ID
#define KBM_UART_ID 0
#endif
#ifndef KBM_UART_TX_PIN
#define KBM_UART_TX_PIN 0
#endif
#ifndef KBM_UART_RX_PIN
#define KBM_UART_RX_PIN 1
#endif
#ifndef KBM_UART_BAUD
#define KBM_UART_BAUD 921600
#endif
#if KBM_UART_ID == 0
#define KBM_UART uart0
#elif KBM_UART_ID == 1
#define KBM_UART uart1
#else
#error KBM_UART_ID must be 0 or 1
#endif

static uint8_t tx_bytes[KBM_MAX_FRAME_SIZE];
static size_t tx_length;
static size_t tx_offset;
static kbm_packet_t ack;
static bool ack_pending;
static kbm_net_assembly_t net_assembly;
static uint32_t baud_pending, baud_switch_ms, baud_restore_ms;
static uint16_t baud_lease;
static kbm_reboot_guard_t reboot_guard;
static uint8_t large_rx[KBM_DISK_HEADER+KBM_DISK_CHUNK];
static kbm_uart_dma_t *dma;
static bool tx_dma_active;
static bool wake_pending;
static uint32_t wake_started;
static uint8_t boot_report[KBM_BOOT_BYTES];
static bool boot_confirmed;
static uint32_t boot_report_at;
static kbm_packet_t latest_hid_diagnostics;
static bool hid_diagnostics_valid,hid_diagnostics_pending;
extern uint8_t tud_network_mac_address[6];

static void encode_byte(uint8_t byte, void *user) {
    (void)user;
    tx_bytes[tx_length++] = byte;
}

static void send_status(uint8_t request_op, kbm_status_t status) {
    if(status==KBM_STATUS_OK && kbm_hid_op_can_wake(request_op))
        __atomic_fetch_add(&g_hid_activity_generation,1u,__ATOMIC_RELEASE);
    ack = (kbm_packet_t){.op = (uint8_t)(request_op | 0x80u), .len = 2};
    ack.payload[0] = (uint8_t)status;
    if(request_op==KBM_HID_DIAGNOSTICS_STATUS && status==KBM_STATUS_OK) {
        if(hid_diagnostics_valid) {
            ack.len=KBM_HID_DIAGNOSTICS_BYTES+1;
            memcpy(ack.payload+1,latest_hid_diagnostics.payload,KBM_HID_DIAGNOSTICS_BYTES);
        } else ack.payload[0]=KBM_STATUS_SERIAL_NOT_READY;
    } else if (request_op == KBM_DISK_STATUS) {
        ack.payload[1]=(uint8_t)__atomic_load_n(&g_disk_flags,__ATOMIC_ACQUIRE);
    } else if (request_op == KBM_SERIAL_OP_STATUS) {
        ack.payload[1] = (uint8_t)__atomic_load_n(&g_serial_flags, __ATOMIC_ACQUIRE);
    } else if (request_op == KBM_NET_STATUS) {
        ack.len = 12;
        ack.payload[1] = (uint8_t)(__atomic_load_n(&g_net_ready, __ATOMIC_ACQUIRE) |
            (__atomic_load_n(&g_net_enabled, __ATOMIC_ACQUIRE) << 1));
        memcpy(ack.payload + 2, tud_network_mac_address, 6);
        kbm_net_put32(ack.payload + 8, 115200u);
    } else {
        ack.payload[1] = (uint8_t)queue_get_level(
            request_op == KBM_SERIAL_OP_WRITE ? &g_serial_tx_queue :
            request_op == KBM_DISK_CONFIG || request_op == KBM_DISK_DATA || request_op == KBM_DISK_FAIL ? &g_disk_commands : &g_command_queue);
    }
    ack_pending = true;
}

static void service_uart_tx(void) {
    if(tx_dma_active) {
        if(kbm_uart_dma_tx_busy(dma)) return;
        tx_dma_active=false;tx_offset=tx_length;
    }
    if (tx_offset == tx_length) {
        kbm_packet_t event;
        const kbm_packet_t *next = NULL;
        if (ack_pending) {
            next = &ack;
        } else if(!boot_confirmed && (int32_t)(to_ms_since_boot(get_absolute_time())-boot_report_at)>=0) {
            event=(kbm_packet_t){.op=KBM_BOOT_EVENT,.len=KBM_BOOT_BYTES};
            boot_report[13]=(uint8_t)__atomic_load_n(&g_usb_state,__ATOMIC_ACQUIRE);
            boot_report[14]=(uint8_t)queue_get_level(&g_command_queue);
            memcpy(event.payload,boot_report,sizeof(boot_report));next=&event;
            boot_report_at=to_ms_since_boot(get_absolute_time())+250u;
        } else if(!g_reboot_requested && hid_diagnostics_pending) {
            next=&latest_hid_diagnostics;hid_diagnostics_pending=false;
        } else if (!g_reboot_requested && queue_try_remove(&g_uart_event_queue, &event)) {
            next = &event;
        } else if (!g_reboot_requested && queue_is_empty(&g_command_queue) && queue_try_remove(&g_disk_events, &event)) {
            next = &event;
        } else if (!g_reboot_requested && queue_is_empty(&g_command_queue) && queue_try_remove(&g_net_event_queue, &event)) {
            next = &event;
        }
        if (!next) return;
        tx_length = 0;
        tx_offset = 0;
        kbm_packet_write(next, encode_byte, NULL);
        if (next == &ack) ack_pending = false;
    }
    // Buffer remains immutable until DMA has consumed the whole variable frame.
    kbm_uart_dma_tx_start(dma,tx_bytes,tx_length);tx_dma_active=true;
}

void uart_transport_run(void) {
    uart_init(KBM_UART, KBM_UART_BAUD);
    gpio_set_function(KBM_UART_TX_PIN, GPIO_FUNC_UART);
    gpio_set_function(KBM_UART_RX_PIN, GPIO_FUNC_UART);
    uart_set_format(KBM_UART, 8, 1, UART_PARITY_NONE);
    uart_set_hw_flow(KBM_UART, false, false);
    uart_set_fifo_enabled(KBM_UART, true);
    dma=kbm_uart_dma_init(KBM_UART);
    kbm_decoder_t decoder;
    kbm_decoder_init_large(&decoder,large_rx,sizeof(large_rx));
    boot_report[0]=KBM_BOOT_SCHEMA;boot_report[1]=3;
    boot_report[2]=1;boot_report[3]=8; // Firmware8.1, USB HID layout still0800.
    const uint64_t nonce=get_rand_64()|1ull;
    for(unsigned i=0;i<8;i++) boot_report[4+i]=(uint8_t)(nonce>>(8u*i));
    boot_report[12]=watchdog_caused_reboot()?1u:0u;

    while (true) {
        if(queue_try_remove(&g_hid_diagnostics,&latest_hid_diagnostics))
            hid_diagnostics_valid=hid_diagnostics_pending=true;
        service_uart_tx();
        const uint32_t now = to_ms_since_boot(get_absolute_time());
        if(wake_pending) {
            uint8_t result;
            const bool ready=kbm_wake_poll(&result);
            if(ready || (uint32_t)(now-wake_started)>=100u) {
                if(!ready) { kbm_wake_cancel();result=KBM_WAKE_TIMED_OUT; }
                send_status(KBM_WAKE_OP,KBM_STATUS_OK);ack.payload[1]=result;
                wake_pending=false;
            }
        }
        if (__atomic_load_n(&g_reboot_requested, __ATOMIC_ACQUIRE)) {
            if (kbm_reboot_ready(&reboot_guard, now, ack_pending, tx_offset == tx_length,
                                !(uart_get_hw(KBM_UART)->fr & UART_UARTFR_BUSY_BITS))) {
                watchdog_reboot(0, 0, 10); // RP2040 normal flash boot, NOT BOOTSEL or host-PC reboot.
                while (true) tight_loop_contents();
            }
            tight_loop_contents();
            continue; // No further commands/events may overtake the reboot ACK.
        }
        if (baud_pending && !ack_pending && tx_offset == tx_length &&
            !(uart_get_hw(KBM_UART)->fr & UART_UARTFR_BUSY_BITS)) {
            if (!baud_switch_ms) baud_switch_ms = now + 10;
            if ((int32_t)(now - baud_switch_ms) >= 0) {
                uart_set_baudrate(KBM_UART, baud_pending);
                baud_restore_ms = now + baud_lease;
                baud_pending = baud_switch_ms = 0;
                kbm_decoder_init_large(&decoder,large_rx,sizeof(large_rx));
                kbm_uart_dma_discard(dma);
            }
        }
        if (baud_restore_ms && (int32_t)(now - baud_restore_ms) >= 0) {
            uart_set_baudrate(KBM_UART, KBM_UART_BAUD);
            baud_restore_ms = 0;
            kbm_decoder_init_large(&decoder,large_rx,sizeof(large_rx));
            kbm_uart_dma_discard(dma);
        }
        uint8_t byte;
        for (unsigned budget = 0; budget < 64 && !ack_pending && !wake_pending; ++budget) {
            const int received=kbm_uart_dma_read(dma,&byte);
            if(!received) break;
            if(received<0) {
                __atomic_fetch_add(&g_slave_uart_errors,1u,__ATOMIC_RELEASE);
                kbm_decoder_init_large(&decoder,large_rx,sizeof(large_rx));continue;
            }
            const kbm_decode_result_t result =
                kbm_decoder_feed(&decoder, byte);
            if (result == KBM_DECODE_MORE) continue;
            if(result==KBM_DECODE_LARGE_FRAME) {
                static kbm_disk_transfer_t transfer;
                kbm_status_t status=KBM_STATUS_BAD_VALUE;
                if(boot_confirmed && !baud_pending&&!baud_restore_ms && decoder.packet.op==KBM_DISK_DATA &&
                   kbm_disk_data_valid(large_rx,decoder.packet.len)) {
                    transfer.len=decoder.packet.len;memcpy(transfer.payload,large_rx,transfer.len);
                    status=queue_try_add(&g_disk_transfers,&transfer)?KBM_STATUS_OK:KBM_STATUS_QUEUE_FULL;
                }
                send_status(decoder.packet.op,status);continue;
            }
            kbm_status_t status = KBM_STATUS_OK;
            if(result!=KBM_DECODE_FRAME) __atomic_fetch_add(&g_slave_uart_errors,1u,__ATOMIC_RELEASE);
            if (result == KBM_DECODE_BAD_LENGTH) status=KBM_STATUS_BAD_LENGTH;
            else if (result == KBM_DECODE_BAD_ESCAPE) status = KBM_STATUS_BAD_ESCAPE;
            else if (result == KBM_DECODE_BAD_ETX) status = KBM_STATUS_BAD_ETX;
            else if (result == KBM_DECODE_BAD_CHECKSUM) status = KBM_STATUS_BAD_CHECKSUM;
            else status = kbm_command_validate(&decoder.packet);
            const uint8_t op = decoder.packet.op;
            if(status==KBM_STATUS_OK && op==KBM_BOOT_ACK) {
                if(memcmp(decoder.packet.payload+1,boot_report+KBM_BOOT_TOKEN_OFFSET,KBM_BOOT_TOKEN_BYTES)==0) boot_confirmed=true;
                continue; // Internal session barrier: no response tracker entry.
            }
            // Old master bytes may still be in flight when this MCU boots.
            // Never queue control/storage/network work until the matching ACK.
            if(status==KBM_STATUS_OK && !boot_confirmed && op!=KBM_OP_PING &&
               op!=KBM_LINK_ECHO && op!=KBM_SERIAL_OP_STATUS && op!=KBM_NET_STATUS && op!=KBM_DISK_STATUS)
                status=KBM_STATUS_SERIAL_NOT_READY;
            if(op==KBM_HID_DIAGNOSTICS_STATUS && result==KBM_DECODE_FRAME)
                status=kbm_command_validate(&decoder.packet); // Read-only even before session ACK.
            // Corruption during an intentional marginal-baud test must never
            // turn random resynchronised bytes into a real HID transition.
            if ((baud_pending || baud_restore_ms) && op != KBM_LINK_ECHO && op != KBM_OP_PING) {
                status = KBM_STATUS_BAD_OPCODE;
            }
            if (status == KBM_STATUS_OK && op == KBM_LINK_ECHO) {
                ack = (kbm_packet_t){.op = KBM_LINK_ECHO | 0x80, .len = decoder.packet.len + 1};
                memcpy(ack.payload + 1, decoder.packet.payload, decoder.packet.len);
                ack_pending = true;
                continue;
            }
            if (status==KBM_STATUS_OK && (op==KBM_DISK_CONFIG || op==KBM_DISK_DATA || op==KBM_DISK_FAIL || op==KBM_DISK_STATUS)) {
                if(op!=KBM_DISK_STATUS && !queue_try_add(&g_disk_commands,&decoder.packet)) status=KBM_STATUS_QUEUE_FULL;
                send_status(op,status);
                continue;
            }
            if (status == KBM_STATUS_OK && op == KBM_REBOOT_OP) {
                send_status(op, KBM_STATUS_OK);
                ack.payload[1] = 0; // Accepted reset, not a HID queue operation.
                __atomic_store_n(&g_reboot_requested, 1u, __ATOMIC_RELEASE);
                break;
            }
            if(status==KBM_STATUS_OK && op==KBM_WAKE_OP) {
                kbm_wake_request();wake_started=now;wake_pending=true;break;
            }
            if (status == KBM_STATUS_OK && op == KBM_LINK_BAUD) {
                baud_pending = kbm_net_u32(decoder.packet.payload);
                baud_lease = kbm_net_u16(decoder.packet.payload + 4);
                baud_switch_ms = 0;
                send_status(op, status);
                continue;
            }
            if (status == KBM_STATUS_OK && op == KBM_NET_CREDIT) {
                const uint32_t credit = (uint32_t)kbm_net_u16(decoder.packet.payload) << 16 |
                    kbm_net_u16(decoder.packet.payload + 2);
                __atomic_store_n(&g_net_credit, credit, __ATOMIC_RELEASE);
                continue;
            }
            if (status == KBM_STATUS_OK && op == KBM_NET_ENABLE) {
                __atomic_store_n(&g_net_enabled, decoder.packet.payload[0], __ATOMIC_RELEASE);
                net_assembly.offset = 0;
            } else if (status == KBM_STATUS_OK && op == KBM_NET_WRITE) {
                if (!__atomic_load_n(&g_net_enabled, __ATOMIC_ACQUIRE) ||
                    !__atomic_load_n(&g_net_ready, __ATOMIC_ACQUIRE)) status = KBM_STATUS_SERIAL_NOT_READY;
                else {
                    int accepted = kbm_net_accept(&net_assembly, decoder.packet.payload, decoder.packet.len);
                    if (accepted < 0) status = KBM_STATUS_BAD_VALUE;
                    else if (accepted == 1 && !queue_try_add(&g_net_tx_queue, &net_assembly.frame)) status = KBM_STATUS_QUEUE_FULL;
                }
                send_status(op, status);
                continue;
            }
            if (status == KBM_STATUS_OK && op == KBM_SERIAL_OP_RX_ACK) {
                const uint16_t sequence = (uint16_t)decoder.packet.payload[0] |
                    ((uint16_t)decoder.packet.payload[1] << 8u);
                __atomic_store_n(&g_serial_rx_acked, sequence, __ATOMIC_RELEASE);
                continue; // Transport credit: deliberately no response.
            }
            if (status == KBM_STATUS_OK && op == KBM_SERIAL_OP_WRITE) {
                const uint8_t flags = (uint8_t)__atomic_load_n(&g_serial_flags, __ATOMIC_ACQUIRE);
                if (!kbm_serial_tx_ready(flags)) status = KBM_STATUS_SERIAL_NOT_READY;
                else if (!queue_try_add(&g_serial_tx_queue, &decoder.packet)) status = KBM_STATUS_QUEUE_FULL;
            } else if (status == KBM_STATUS_OK && op != KBM_OP_PING &&
                       op != KBM_SERIAL_OP_STATUS && op != KBM_NET_STATUS && op != KBM_NET_ENABLE && op!=KBM_HID_DIAGNOSTICS_STATUS) {
                kbm_hid_command_t command={.packet=decoder.packet,
                    .usb_epoch=__atomic_load_n(&g_hid_usb_epoch,__ATOMIC_ACQUIRE)};
                if(op==KBM_SYSTEM_DOWN && !(__atomic_load_n(&g_serial_flags,__ATOMIC_ACQUIRE)&1u)) {
                    send_status(op,KBM_STATUS_SERIAL_NOT_READY);continue;
                }
                if (!queue_try_add(&g_command_queue, &command)) {
                    // Explicit rejection, NOT an accepted/lost transition.
                    // Keep decoding so PING/status/recovery can always proceed.
                    status=KBM_STATUS_QUEUE_FULL;
                    __atomic_fetch_add(&g_hid_queue_rejected,1u,__ATOMIC_RELEASE);
                }
            }
            send_status(op, status);
        }
        tight_loop_contents();
    }
}

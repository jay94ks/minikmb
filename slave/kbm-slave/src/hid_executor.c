#include "hid_executor.h"

#include "app_state.h"
#include "command.h"
#include "protocol.h"
#include "serial_bridge.h"
#include "network_bridge.h"
#include "disk_bridge.h"
#include "tusb_config.h"
#include "usb_descriptors.h"
#include "wake_bridge.h"
#include "usb_session.h"

#include "bsp/board_api.h"
#include "hardware/gpio.h"
#include "pico/stdlib.h"
#include "tusb.h"
#include "device/dcd.h"
#include "hardware/watchdog.h"

#include <stdbool.h>
#include <stdint.h>
#include <string.h>

typedef struct {
    bool waiting;
    bool submitted;
    bool can_wake;
    uint8_t report_id;
    uint8_t length;
    uint8_t data[8];
    uint32_t usb_epoch;
} hid_transfer_t;

typedef struct {
    bool active;
    bool release_mouse_pending;
    bool release_system_pending;
    uint8_t system_keys[KBM_SYSTEM_REPORT_BYTES];
    uint32_t system_epoch;
    uint32_t usb_epoch;
    kbm_packet_t packet;
    uint8_t keys_down[32];
    uint8_t mouse_buttons;
    uint16_t mouse_x;
    uint16_t mouse_y;
} executor_t;

static hid_transfer_t transfer;
static bool usb_suspended;
static uint32_t suspend_activity_generation;
static uint32_t usb_reset_generation;
static kbm_usb_session_t usb_session;
static uint32_t usb_bus_resets,usb_setups;
static uint32_t hid_submitted,hid_completed,hid_wait_started;

typedef struct {
    bool usb_ready;
    bool output_on;
    uint64_t last_toggle_us;
} status_led_t;

static void status_led_init(status_led_t *led) {
    memset(led, 0, sizeof(*led));
    led->last_toggle_us = time_us_64();
    gpio_init(PICO_DEFAULT_LED_PIN);
    gpio_set_dir(PICO_DEFAULT_LED_PIN, GPIO_OUT);
    gpio_put(PICO_DEFAULT_LED_PIN, 0);
}

static void status_led_task(status_led_t *led) {
    const bool ready = network_bridge_usb_available() && tud_mounted() && tud_ready();
    const uint64_t now = time_us_64();

    if (ready) {
        if (!led->output_on) {
            led->output_on = true;
            gpio_put(PICO_DEFAULT_LED_PIN, 1);
        }
        led->usb_ready = true;
        led->last_toggle_us = now;
        return;
    }

    if (led->usb_ready) {
        // Make a disconnect visible immediately, then begin blinking.
        led->usb_ready = false;
        led->output_on = false;
        led->last_toggle_us = now;
        gpio_put(PICO_DEFAULT_LED_PIN, 0);
        return;
    }

    if (now - led->last_toggle_us >= 500000u) {
        led->last_toggle_us = now;
        led->output_on = !led->output_on;
        gpio_put(PICO_DEFAULT_LED_PIN, led->output_on ? 1 : 0);
    }
}

static void set_mounted(bool mounted) {
    __atomic_store_n(&g_usb_mounted, mounted, __ATOMIC_RELEASE);
}

void tud_event_hook_cb(uint8_t rhport,uint32_t eventid,bool in_isr) {
    (void)rhport;(void)in_isr;
    // This hook may be in an IRQ. Record only; core1 performs state changes.
    if(eventid==DCD_EVENT_BUS_RESET || eventid==DCD_EVENT_UNPLUGGED)
        __atomic_fetch_add(&usb_reset_generation,1u,__ATOMIC_RELEASE);
    if(eventid==DCD_EVENT_BUS_RESET) __atomic_fetch_add(&usb_bus_resets,1u,__ATOMIC_RELEASE);
    if(eventid==DCD_EVENT_SETUP_RECEIVED) __atomic_fetch_add(&usb_setups,1u,__ATOMIC_RELEASE);
}

void tud_suspend_cb(bool remote_wakeup_en) {
    suspend_activity_generation=__atomic_load_n(&g_hid_activity_generation,__ATOMIC_ACQUIRE);
    kbm_wake_suspend(remote_wakeup_en);
    usb_suspended = true;
}

void tud_resume_cb(void) {
    kbm_wake_resume();
    usb_suspended = false;
}

static void poll_usb_session(void) {
    const bool mounted=tud_mounted(),suspended=tud_suspended();
    const uint8_t changes=kbm_usb_session_poll(&usb_session,mounted,suspended,
        __atomic_load_n(&usb_reset_generation,__ATOMIC_ACQUIRE));
    set_mounted(mounted); // Published mirror is sampled, not callback-latched.
    __atomic_store_n(&g_usb_state,(mounted?1u:0u)|(tud_ready()?2u:0u)|(suspended?4u:0u),__ATOMIC_RELEASE);
    if(changes) __atomic_fetch_add(&g_hid_usb_epoch,1u,__ATOMIC_RELEASE);
    if(changes&(KBM_USB_SESSION_RESET|KBM_USB_SESSION_MOUNT)) {
        // Host reset cancels endpoint ownership even if umount was never called.
        transfer.submitted=false;
        transfer.can_wake=false;
    }
    if(usb_suspended!=suspended || (changes&KBM_USB_SESSION_RESET)) {
        if(suspended) {
            suspend_activity_generation=__atomic_load_n(&g_hid_activity_generation,__ATOMIC_ACQUIRE);
            kbm_wake_suspend(false); // Missing callback: never invent host permission.
        } else kbm_wake_resume();
    }
    usb_suspended=suspended;
}

void tud_hid_report_complete_cb(uint8_t instance,
                                uint8_t const *report,
                                uint16_t len) {
    (void)instance;
    (void)report;
    (void)len;
    ++hid_completed; // USB transfer completion, not host OS key interpretation.
    // A canceled cached power DOWN was converted to neutral before reconnect.
    // Its old completion must not consume the newer neutral report.
    if(transfer.waiting && transfer.report_id==KBM_REPORT_ID_SYSTEM && len>=2 &&
       report[0]==KBM_REPORT_ID_SYSTEM && (len!=transfer.length+1u ||
       memcmp(report+1,transfer.data,transfer.length)!=0)) {
        transfer.submitted=false;return;
    }
    transfer.waiting = false;
    transfer.submitted = false;
}

uint16_t tud_hid_get_report_cb(uint8_t instance,
                               uint8_t report_id,
                               hid_report_type_t report_type,
                               uint8_t *buffer,
                               uint16_t requested_length) {
    (void)instance;
    (void)report_id;
    (void)report_type;
    (void)buffer;
    (void)requested_length;
    return 0;
}

void tud_hid_set_report_cb(uint8_t instance,
                           uint8_t report_id,
                           hid_report_type_t report_type,
                           uint8_t const *buffer,
                           uint16_t buffer_size) {
    (void)instance;
    (void)report_id;
    (void)report_type;
    (void)buffer;
    (void)buffer_size;
}

static void queue_report(uint8_t report_id, const void *data, uint8_t length) {
    transfer.can_wake = false;
    transfer.report_id = report_id;
    transfer.length = length;
    memcpy(transfer.data, data, length);
    transfer.waiting = true;
    transfer.submitted = false;
    hid_wait_started=to_ms_since_boot(get_absolute_time());
}

static void service_transfer(void) {
    if (!transfer.waiting || transfer.submitted || !tud_mounted() || !network_bridge_usb_available()) {
        return;
    }
    if(transfer.report_id==KBM_REPORT_ID_SYSTEM &&
       !kbm_hid_power_current(transfer.usb_epoch,
           __atomic_load_n(&g_hid_usb_epoch,__ATOMIC_ACQUIRE),tud_ready())) {
        memset(transfer.data,0,transfer.length); // Never replay stale system input.
    }
    if (usb_suspended) {
        // A power-button/UP/release report must not undo the resulting sleep.
        // Fresh activity queued behind a neutral release may still wake the PC.
        if(kbm_hid_resume_needed(transfer.can_wake,
            __atomic_load_n(&g_hid_activity_generation,__ATOMIC_ACQUIRE),suspend_activity_generation))
            kbm_wake_automatic();
        return;
    }
    if (tud_hid_ready() &&
        tud_hid_report(transfer.report_id, transfer.data, transfer.length)) {
        transfer.submitted = true;
        ++hid_submitted;
    }
}

static void publish_hid_diagnostics(void) {
    static uint32_t next_sample,last_publish;
    static uint8_t last_flags=0xff;
    const uint32_t now=to_ms_since_boot(get_absolute_time());
    if((int32_t)(now-next_sample)<0) return;
    next_sample=now+20u; // Bounded sampling/queue-lock cost, also under congestion.
    const uint8_t flags=(tud_mounted()?KBM_DIAG_MOUNTED:0u)|(tud_ready()?KBM_DIAG_READY:0u)|
        (tud_suspended()?KBM_DIAG_SUSPENDED:0u)|(tud_hid_ready()?KBM_DIAG_HID_READY:0u)|
        (network_bridge_usb_available()?KBM_DIAG_STABLE:0u)|(transfer.waiting?KBM_DIAG_WAITING:0u)|
        (transfer.submitted?KBM_DIAG_SUBMITTED:0u);
    // Report active transfer flags/counts at most once a second. USB lifecycle
    // transitions can publish immediately without flooding on every HID report.
    if((flags&0x17u)==(last_flags&0x17u) && (uint32_t)(now-last_publish)<1000u) return;
    kbm_packet_t event={.op=KBM_HID_DIAGNOSTICS_EVENT,.len=KBM_HID_DIAGNOSTICS_BYTES};
    uint8_t *p=event.payload;
    p[0]=KBM_HID_DIAGNOSTICS_SCHEMA;p[1]=flags;
    p[2]=(uint8_t)queue_get_level(&g_command_queue);p[3]=transfer.waiting?transfer.report_id:0;
    p[4]=watchdog_caused_reboot()?1u:0u;
    const uint32_t values[]={now,__atomic_load_n(&usb_bus_resets,__ATOMIC_ACQUIRE),
        __atomic_load_n(&g_hid_usb_epoch,__ATOMIC_ACQUIRE),hid_submitted,hid_completed,
        transfer.waiting?(uint32_t)(now-hid_wait_started):0u,
        __atomic_load_n(&usb_setups,__ATOMIC_ACQUIRE),
        __atomic_load_n(&g_hid_queue_rejected,__ATOMIC_ACQUIRE),
        __atomic_load_n(&g_slave_uart_errors,__ATOMIC_ACQUIRE),
        __atomic_load_n(&g_hid_activity_generation,__ATOMIC_ACQUIRE),0u};
    for(unsigned i=0;i<sizeof(values)/sizeof(values[0]);i++) kbm_net_put32(p+8+4*i,values[i]);
    if(queue_try_add(&g_hid_diagnostics,&event)) { last_publish=now;last_flags=flags; }
}

static void queue_keyboard_state(const executor_t *executor) {
    uint8_t report[8] = {0};
    kbm_hid_keyboard_report(executor->keys_down,report);
    queue_report(KBM_REPORT_ID_KEYBOARD, report, sizeof(report));
}
static void queue_system_state(const executor_t *executor) {
    queue_report(KBM_REPORT_ID_SYSTEM,executor->system_keys,KBM_SYSTEM_REPORT_BYTES);
    transfer.usb_epoch=executor->usb_epoch;
}

static void queue_mouse_state(const executor_t *executor,
                              int8_t wheel,
                              int8_t pan) {
    const hid_abs_mouse_report_t report = {
        .buttons = executor->mouse_buttons,
        .x = (int16_t)executor->mouse_x,
        .y = (int16_t)executor->mouse_y,
        .wheel = wheel,
        .pan = pan,
    };
    queue_report(KBM_REPORT_ID_MOUSE, &report, sizeof(report));
}

static void execute_packet(executor_t *executor) {
    const uint8_t *payload = executor->packet.payload;

    switch (executor->packet.op) {
        case KBM_OP_PING:
            executor->active = false;
            break;

        case KBM_OP_KEY_DOWN:
            kbm_hid_key_set(executor->keys_down, payload[0], true);
            queue_keyboard_state(executor);
            transfer.can_wake=true;
            executor->active = false;
            break;

        case KBM_OP_KEY_UP:
            kbm_hid_key_set(executor->keys_down, payload[0], false);
            queue_keyboard_state(executor);
            executor->active = false;
            break;

        case KBM_OP_MOUSE_MOVE:
            executor->mouse_x =
                (uint16_t)payload[0] | ((uint16_t)payload[1] << 8u);
            executor->mouse_y =
                (uint16_t)payload[2] | ((uint16_t)payload[3] << 8u);
            queue_mouse_state(executor, (int8_t)payload[4],
                              (int8_t)payload[5]);
            transfer.can_wake=true;
            executor->active = false;
            break;

        case KBM_OP_MOUSE_DOWN:
            executor->mouse_buttons |= payload[0];
            queue_mouse_state(executor, 0, 0);
            transfer.can_wake=true;
            executor->active = false;
            break;

        case KBM_OP_MOUSE_UP:
            executor->mouse_buttons &= (uint8_t)~payload[0];
            queue_mouse_state(executor, 0, 0);
            executor->active = false;
            break;

        case KBM_SYSTEM_DOWN:
        case KBM_SYSTEM_UP:
            if(executor->system_epoch!=executor->usb_epoch) {
                memset(executor->system_keys,0,sizeof(executor->system_keys));
                executor->system_epoch=executor->usb_epoch;
            }
            if(executor->packet.op==KBM_SYSTEM_DOWN &&
               !kbm_hid_power_current(executor->usb_epoch,
                   __atomic_load_n(&g_hid_usb_epoch,__ATOMIC_ACQUIRE),tud_ready())) {
                executor->active=false;break;
            }
            (void)kbm_hid_system_set(executor->system_keys,payload[0],executor->packet.op==KBM_SYSTEM_DOWN);
            queue_system_state(executor);
            executor->active=false;
            break;
        case KBM_OP_RELEASE_ALL:
            memset(executor->keys_down, 0, sizeof(executor->keys_down));
            executor->mouse_buttons = 0;
            queue_keyboard_state(executor);
            executor->release_mouse_pending = true;
            memset(executor->system_keys,0,sizeof(executor->system_keys));
            executor->release_system_pending=true;
            break;

        default:
            // Core 0 validates every packet before enqueueing it.
            executor->active = false;
            break;
    }
}

void hid_executor_core1(void) {
    executor_t executor;
    status_led_t status_led;
    memset(&executor, 0, sizeof(executor));
    memset(&transfer, 0, sizeof(transfer));
    usb_suspended = false;
    set_mounted(false);

    // Core 1 is the sole owner of TinyUSB and every HID callback/report.
    tud_init(BOARD_TUD_RHPORT);
    if (board_init_after_tusb) {
        board_init_after_tusb();
    }
    status_led_init(&status_led);

    while (true) {
        tud_task();
        poll_usb_session();
        if (__atomic_load_n(&g_reboot_requested, __ATOMIC_ACQUIRE)) {
            tud_disconnect(); // Only core 1 may touch TinyUSB, even at reset.
            while (true) tight_loop_contents(); // Core 0 drains ACK and resets both cores.
        }
        status_led_task(&status_led);
        kbm_wake_task();
        service_transfer();
        publish_hid_diagnostics();
        // Start HID work first. Auxiliary traffic still progresses while a
        // report is in flight, but can never delay starting an accepted report.
        if (!transfer.waiting && tud_mounted() && network_bridge_usb_available()) {
            if (executor.release_mouse_pending) {
                executor.release_mouse_pending = false;
                queue_mouse_state(&executor, 0, 0);
            } else if(executor.release_system_pending) {
                executor.release_system_pending=false;
                executor.active=false;
                queue_system_state(&executor);
            } else if (!executor.active) {
                kbm_hid_command_t command;
                if(queue_try_remove(&g_command_queue,&command)) {
                    executor.packet=command.packet;executor.usb_epoch=command.usb_epoch;
                    executor.active = true;
                    execute_packet(&executor);
                }
            }
        }
        serial_bridge_task();
        disk_bridge_task();
        network_bridge_task();
        tight_loop_contents();
    }
}


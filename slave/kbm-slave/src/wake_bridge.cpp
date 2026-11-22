#include "wake_bridge.h"
#include "network_bridge.h"
#include "firmware/usb_wake.hpp"
#include "pico/stdlib.h"
#include "tusb.h"
namespace {
struct Usb {
    bool mounted() const { return tud_mounted(); }
    bool available() const { return network_bridge_usb_available(); }
    bool wake() const { return tud_remote_wakeup(); }
} usb;
struct Clock { uint64_t microseconds() const { return time_us_64(); } } clock_source;
kbm::UsbWake<Usb,Clock> wake(usb,clock_source);
// Upper24 bits are an epoch: a late USB result cannot complete a newer request.
uint32_t mailbox,sequence;
}
extern "C" void kbm_wake_request(void) {
    sequence+=0x100u;
    __atomic_store_n(&mailbox,sequence|KBM_WAKE_PENDING,__ATOMIC_RELEASE);
}
extern "C" bool kbm_wake_poll(uint8_t *result) {
    const auto value=__atomic_load_n(&mailbox,__ATOMIC_ACQUIRE);
    if((value&255u)==KBM_WAKE_PENDING) return false;
    *result=static_cast<uint8_t>(value);return true;
}
extern "C" void kbm_wake_cancel(void) {
    __atomic_store_n(&mailbox,sequence|KBM_WAKE_TIMED_OUT,__ATOMIC_RELEASE);
}
extern "C" void kbm_wake_task(void) {
    auto expected=__atomic_load_n(&mailbox,__ATOMIC_ACQUIRE);
    if((expected&255u)!=KBM_WAKE_PENDING) return;
    const auto result=wake.attempt();
    if(result!=KBM_WAKE_PENDING)
        __atomic_compare_exchange_n(&mailbox,&expected,(expected&~255u)|result,false,__ATOMIC_RELEASE,__ATOMIC_RELAXED);
}
extern "C" void kbm_wake_suspend(bool allowed) { wake.suspend(allowed); }
extern "C" void kbm_wake_resume(void) { wake.resume(); }
extern "C" void kbm_wake_automatic(void) { wake.automatic(); }

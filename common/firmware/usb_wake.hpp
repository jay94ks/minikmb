#pragma once
#include "wake_protocol.h"
#include <cstdint>
namespace kbm {
// USB backend and clock are compile-time injected; no HID report, heap or retry.
template<class Usb, class Clock> class UsbWake {
public:
    UsbWake(Usb& usb, Clock& clock):usb_(usb),clock_(clock) {}
    void suspend(bool allowed) { suspended_=true;allowed_=allowed;auto_done_=false;at_=clock_.microseconds(); }
    void resume() { suspended_=false;allowed_=false;auto_done_=false; }
    std::uint8_t attempt() {
        if(!usb_.mounted()) return KBM_WAKE_IGNORED_DISCONNECTED;
        if(!usb_.available()) return KBM_WAKE_IGNORED_TRANSITION;
        if(!suspended_) return KBM_WAKE_IGNORED_ACTIVE;
        if(!allowed_) return KBM_WAKE_IGNORED_HOST_DISABLED;
        if(clock_.microseconds()-at_<5000u) return KBM_WAKE_PENDING;
        auto_done_=true; // Explicit wake must not also trigger an automatic HID wake.
        return usb_.wake()?KBM_WAKE_SENT:KBM_WAKE_IGNORED_UNSUPPORTED;
    }
    void automatic() {
        if(!auto_done_ && attempt()!=KBM_WAKE_PENDING) auto_done_=true;
    }
private:
    Usb& usb_;Clock& clock_;
    bool suspended_{},allowed_{},auto_done_{};
    std::uint64_t at_{};
};
}

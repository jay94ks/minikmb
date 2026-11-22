#include "firmware/usb_wake.hpp"
#include <cassert>
#include <cstdio>
struct Usb { bool connected=true,ready=true,supported=true;unsigned calls=0;
    bool mounted() const { return connected; } bool available() const { return ready; }
    bool wake() { ++calls;return supported; } };
struct Clock { uint64_t now=0;uint64_t microseconds() const { return now; } };
int main() {
    Usb usb;Clock clock;kbm::UsbWake<Usb,Clock> wake(usb,clock);
    assert(wake.attempt()==KBM_WAKE_IGNORED_ACTIVE&&usb.calls==0);
    wake.suspend(false);assert(wake.attempt()==KBM_WAKE_IGNORED_HOST_DISABLED&&usb.calls==0);
    usb.connected=false;assert(wake.attempt()==KBM_WAKE_IGNORED_DISCONNECTED);
    usb.connected=true;usb.ready=false;assert(wake.attempt()==KBM_WAKE_IGNORED_TRANSITION);
    usb.ready=true;wake.suspend(true);assert(wake.attempt()==KBM_WAKE_PENDING&&usb.calls==0);
    clock.now=4999;assert(wake.attempt()==KBM_WAKE_PENDING);
    clock.now=5000;assert(wake.attempt()==KBM_WAKE_SENT&&usb.calls==1);
    wake.automatic();assert(usb.calls==1);
    usb.supported=false;assert(wake.attempt()==KBM_WAKE_IGNORED_UNSUPPORTED&&usb.calls==2);
    wake.suspend(true);clock.now+=5000;wake.automatic();wake.automatic();
    assert(usb.calls==3); // Unsupported automatic wake is never retried in this suspend.
    wake.resume();assert(wake.attempt()==KBM_WAKE_IGNORED_ACTIVE);
    wake.suspend(false);wake.automatic();wake.automatic();assert(usb.calls==3);
    std::puts("PASS USB wake: explicit result codes, host permission, minimum suspend time, no unsupported retry or HID");
}

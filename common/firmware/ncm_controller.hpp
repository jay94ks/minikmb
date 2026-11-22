#pragma once
#include <cstdint>

namespace kbm {
inline bool reached(std::uint32_t now, std::uint32_t deadline) noexcept {
    return static_cast<std::int32_t>(now - deadline) >= 0;
}

// USB and Clock are injected implementations, not virtual interfaces. All calls
// may be inlined on RP2040; Windows tests supply deterministic fakes.
// Visible may also be a bitmask: one detach/reconnect coordinator must own all
// optional functions on a composite device (NCM and MSC on the slave).
template<class Usb, class Clock, class Visible = bool>
class NcmVisibility {
public:
    NcmVisibility(Usb& usb, Clock& clock) : usb_(usb), clock_(clock) {}
    void request(Visible visible) {
        if (visible == usb_.visible()) return;
        usb_.disconnect();
        usb_.setVisible(visible); // Descriptors only change while disconnected.
        reconnecting_ = true;
        reconnect_at_ = clock_.now() + 300u;
    }
    void tick() {
        if (reconnecting_ && reached(clock_.now(), reconnect_at_)) {
            reconnecting_ = false;
            usb_.connect();
        }
    }
    bool changing() const noexcept { return reconnecting_; }
    bool ready() const { return !reconnecting_ && usb_.visible() && usb_.ready(); }
private:
    Usb& usb_;
    Clock& clock_;
    bool reconnecting_{};
    std::uint32_t reconnect_at_{};
};

// Ports implements begin(mode, dhcp) and transitionMask(). It owns cross-core
// publication; the state machine has no Pico SDK/TinyUSB/OS dependencies.
template<class Usb, class Clock, class Ports>
class NativeModeController {
public:
    NativeModeController(Usb& usb, Clock& clock, Ports& ports)
        : usb_(usb), clock_(clock), ports_(ports), visibility_(usb, clock) {}
    bool request(bool enabled, bool dhcp) {
        if (changing()) return false;
        requested_ = enabled; dhcp_ = dhcp;
        pending_ = ack_wait_ = true;
        return true;
    }
    void acknowledgeSent() {
        if (!pending_ || !ack_wait_) return;
        ack_wait_ = false; apply_at_ = clock_.now() + 200u;
    }
    void tick() {
        if (pending_ && !ack_wait_ && reached(clock_.now(), apply_at_)) {
            pending_ = false; applied_ = requested_;
            ports_.begin(requested_, requested_ && dhcp_);
            visibility_.request(requested_);
        }
        visibility_.tick();
    }
    bool owned() const { return requested_ || applied_ || changing(); }
    bool changing() const { return pending_ || visibility_.changing() || ports_.transitionMask(); }
    bool enabled() const noexcept { return requested_; }
    bool dhcp() const noexcept { return dhcp_; }
    bool visible() const { return usb_.visible(); }
    bool ready() const { return applied_ && visibility_.ready(); }
    std::uint8_t transitionMask() const {
        return static_cast<std::uint8_t>(ports_.transitionMask() | (pending_ ? 3u : 0u));
    }
private:
    Usb& usb_;
    Clock& clock_;
    Ports& ports_;
    NcmVisibility<Usb, Clock> visibility_;
    bool requested_{}, dhcp_{}, applied_{}, pending_{}, ack_wait_{};
    std::uint32_t apply_at_{};
};
}

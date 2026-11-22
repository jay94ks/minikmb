#pragma once
#include "ncm_controller.hpp"
#include "fragment_sender.hpp"
#include <cstdint>
namespace kbm {
class NativeUartSession {
public:
    void begin(bool mode, std::uint32_t now) {
        mode_ = mode; cycle_ = Cycle::Disable; at_ = now;
        ready_ = pending_ = false; transitioning_ = true; sender_.reset();
    }
    void rebooted(std::uint32_t now) {
        ready_ = false; sender_.reset(); reset_ = true;
        if (mode_) { cycle_ = Cycle::Probe; at_ = now + 1500u; }
    }
    void timedOut() {
        pending_ = ready_ = transitioning_ = false; sender_.reset();
        cycle_ = mode_ ? Cycle::Disable : Cycle::Idle;
    }
    template<class Source, class Packet>
    bool next(std::uint32_t now, bool online, Source& source, Packet& packet) {
        if (pending_) return false;
        if (!online) {
            ready_ = transitioning_ = false;
            if (!mode_) cycle_ = Cycle::Idle;
            return false;
        }
        packet = {};
        if (cycle_ != Cycle::Idle) {
            if (!reached(now, at_)) return false;
            if (cycle_ == Cycle::Wait) cycle_ = Cycle::Enable;
            if (cycle_ == Cycle::Disable || cycle_ == Cycle::Enable) {
                packet.op = KBM_NET_ENABLE; packet.len = 1;
                packet.payload[0] = cycle_ == Cycle::Enable;
            } else packet.op = KBM_NET_STATUS;
        } else if (mode_) {
            if (static_cast<std::int32_t>(now - at_) >= 1000) packet.op = KBM_NET_STATUS;
            else if (!sender_.next(source, packet)) return false;
        } else return false;
        pending_ = true; op_ = packet.op;
        return true;
    }
    template<class Packet>
    bool acknowledge(const Packet& packet, std::uint32_t now) {
        if (!pending_ || packet.op != (op_ | 0x80u)) return false;
        pending_ = false;
        const bool ok = packet.len && packet.payload[0] == 0;
        if (op_ == KBM_NET_ENABLE) {
            if (ok) {
                bool enabled = cycle_ != Cycle::Disable;
                cycle_ = enabled ? (mode_ ? Cycle::Probe : Cycle::Idle) : Cycle::Wait;
                if (enabled) transitioning_ = false;
                at_ = now + 600u;
            } else at_ = now + 500u;
        } else if (op_ == KBM_NET_STATUS) {
            ready_ = ok && packet.len >= 2 && (packet.payload[1] & 3u) == 3u;
            if (ready_) { cycle_ = Cycle::Idle; transitioning_ = false; at_ = now; }
            else {
                bool disabled = ok && packet.len >= 2 && !(packet.payload[1] & 2u);
                cycle_ = disabled ? Cycle::Enable : Cycle::Probe;
                reset_ |= disabled; at_ = now + 500u;
            }
        } else if (op_ == KBM_NET_WRITE) sender_.acknowledge(ok);
        return true;
    }
    bool pending() const noexcept { return pending_; }
    bool ready() const noexcept { return ready_; }
    bool transitioning() const noexcept { return transitioning_; }
    bool takeReset() noexcept { bool value = reset_; reset_ = false; return value; }
private:
    enum class Cycle { Idle, Disable, Wait, Enable, Probe };
    Cycle cycle_{Cycle::Idle};
    FragmentSender sender_;
    std::uint32_t at_{};
    std::uint8_t op_{};
    bool mode_{}, pending_{}, ready_{}, transitioning_{}, reset_{};
};
}

#pragma once
#include "ethernet_protocol.h"
#include <cstdint>
#include <cstring>
namespace kbm {
// Transport-independent stop-and-wait fragment encoder. Source queue and Packet
// are injected, so the exact production implementation can run in Windows tests.
class FragmentSender {
public:
    template<class Source, class Packet>
    bool next(Source& source, Packet& packet) {
        if (!active_) {
            if (!source.pop(frame_)) return false;
            active_ = true; ++id_; offset_ = 0;
            crc_ = kbm_net_crc(frame_.data, frame_.length);
        }
        const auto remaining = static_cast<std::uint16_t>(frame_.length - offset_);
        const auto count = static_cast<std::uint16_t>(remaining > KBM_NET_CHUNK ? KBM_NET_CHUNK : remaining);
        packet.op = KBM_NET_WRITE;
        packet.len = static_cast<std::uint8_t>(KBM_NET_HEADER + count);
        kbm_net_put16(packet.payload, id_);
        kbm_net_put16(packet.payload + 2, frame_.length);
        kbm_net_put16(packet.payload + 4, offset_);
        kbm_net_put32(packet.payload + 6, crc_);
        std::memcpy(packet.payload + KBM_NET_HEADER, frame_.data + offset_, count);
        return true;
    }
    void acknowledge(bool success) {
        if (!active_) return;
        if (!success) { active_ = false; return; }
        const auto remaining = static_cast<std::uint16_t>(frame_.length - offset_);
        offset_ = static_cast<std::uint16_t>(offset_ + (remaining > KBM_NET_CHUNK ? KBM_NET_CHUNK : remaining));
        if (offset_ == frame_.length) active_ = false;
    }
    void reset() noexcept { active_ = false; }
    bool active() const noexcept { return active_; }
    std::uint32_t key() const noexcept { return (static_cast<std::uint32_t>(id_) << 16) | offset_; }
private:
    kbm_net_frame_t frame_{};
    bool active_{};
    std::uint16_t id_{}, offset_{};
    std::uint32_t crc_{};
};
}

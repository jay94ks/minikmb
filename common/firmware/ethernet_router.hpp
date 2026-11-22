#pragma once
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
namespace kbm {
// Only USB -> slave destinations are selected here. Slave -> slave forwarding
// is deliberately absent. One stable host MAC per isolated slave USB adapter.
template<std::size_t Ports>
class EthernetRouter {
    static_assert(Ports > 0 && Ports <= 8);
public:
    void learn(std::size_t port, const std::uint8_t* source) {
        if (port >= Ports || (source[0]&1)) return;
        std::memcpy(macs_[port].data(), source, 6); valid_[port] = true;
    }
    void forget(std::size_t port) { if(port < Ports) valid_[port] = false; }
    void clear() { valid_.fill(false); }
    std::uint8_t targets(const std::uint8_t* destination, std::uint8_t available) const {
        auto result = static_cast<std::uint8_t>((1u << Ports)-1u);
        if (!(destination[0]&1)) {
            for(std::size_t i=0; i<Ports; ++i) if(valid_[i] && !std::memcmp(destination, macs_[i].data(), 6))
                result = static_cast<std::uint8_t>(1u << i);
        }
        return result & available;
    }
private:
    std::array<std::array<std::uint8_t,6>,Ports> macs_{};
    std::array<bool,Ports> valid_{};
};
}

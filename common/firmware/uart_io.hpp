#pragma once
#include <cstddef>
namespace kbm {
// No allocation or virtual calls. Driver may access RP2040 registers directly,
// while a Windows mock controls FIFO capacity and incoming byte boundaries.
template<class Driver>
class UartIo {
public:
    explicit UartIo(Driver& driver) : driver_(driver) {}
    template<std::size_t Budget, class Consumer>
    void receive(Consumer&& consume) {
        for (std::size_t n=0; n<Budget && driver_.readable(); ++n) consume(driver_.read());
    }
    template<std::size_t Budget>
    void send(const unsigned char* bytes, std::size_t length, std::size_t& offset) {
        for (std::size_t n=0; n<Budget && offset<length && driver_.writable(); ++n)
            driver_.write(bytes[offset++]);
    }
private:
    Driver& driver_;
};
}

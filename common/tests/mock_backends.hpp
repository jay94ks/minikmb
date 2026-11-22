#pragma once
#include <array>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <deque>
#include <vector>
namespace mock {
struct Clock { std::uint32_t value{}; std::uint32_t now() const { return value; } void advance(std::uint32_t n) { value += n; } };
struct Usb {
    bool shown{}, configured{};
    unsigned disconnects{}, connects{}, descriptor_changes{};
    bool detached{};
    bool visible() const { return shown; }
    bool ready() const { return configured && !detached; }
    void disconnect() { detached=true; configured=false; ++disconnects; }
    void setVisible(bool value) { assert(detached); shown=value; ++descriptor_changes; }
    void connect() { detached=false; ++connects; }
};
struct Ports {
    unsigned begins{};
    std::uint8_t mask{};
    bool mode{}, dhcp{};
    void begin(bool value, bool enable_dhcp) { mode=value; dhcp=enable_dhcp; mask=3; ++begins; }
    std::uint8_t transitionMask() const { return mask; }
};
template<class T, std::size_t N> struct Storage {
    std::array<T,N> values{};
    std::size_t head{}, size{};
    struct Backend {
        Storage* storage;
        bool push(const void* source, std::size_t bytes, std::size_t capacity) {
            assert(bytes==sizeof(T) && capacity==N);
            if(storage->size==N) return false;
            std::memcpy(&storage->values[(storage->head+storage->size)%N],source,bytes);
            ++storage->size; return true;
        }
        bool pop(void* target, std::size_t bytes) {
            assert(bytes==sizeof(T));
            if(!storage->size) return false;
            std::memcpy(target,&storage->values[storage->head],bytes);
            storage->head=(storage->head+1)%N; --storage->size; return true;
        }
        std::size_t size() const { return storage->size; }
    };
};
struct Uart {
    std::deque<std::uint8_t> rx;
    std::vector<std::uint8_t> tx;
    std::size_t slots{};
    bool readable() const { return !rx.empty(); }
    bool writable() const { return slots>0; }
    std::uint8_t read() { auto value=rx.front(); rx.pop_front(); return value; }
    void write(std::uint8_t value) { assert(slots); --slots; tx.push_back(value); }
};
}

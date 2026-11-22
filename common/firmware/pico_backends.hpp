#pragma once
#include "pico/stdlib.h"
#include "pico/util/queue.h"
#include "hardware/uart.h"
#include "tusb.h"
#include <cassert>
#include <cstddef>
#include <cstdint>

extern "C" bool kbm_ncm_link_ready(void);
namespace kbm::pico {
struct Clock {
    std::uint32_t now() const { return to_ms_since_boot(get_absolute_time()); }
};
struct QueueBackend {
    queue_t* queue;
    bool push(const void* value, std::size_t bytes, std::size_t capacity) {
        assert(bytes == queue->element_size && capacity == queue->element_count);
        (void)bytes; (void)capacity;
        return queue_try_add(queue, value);
    }
    bool pop(void* value, std::size_t bytes) {
        assert(bytes == queue->element_size);
        (void)bytes;
        return queue_try_remove(queue, value);
    }
    std::size_t size() const { return queue_get_level(queue); }
};
// Descriptors supplied by the owning board. The template avoids function-pointer
// dispatch in USB/clock hot paths and makes reset/reenumeration easy to simulate.
template<class Descriptors>
struct UsbNcm {
    bool visible() const { return Descriptors::visible(); }
    void setVisible(bool value) { Descriptors::setVisible(value); }
    void disconnect() { tud_disconnect(); }
    void connect() { tud_connect(); }
    bool ready() const { return tud_ready() && kbm_ncm_link_ready(); }
};
class Uart {
public:
    uart_inst_t* instance{};
    bool readable() const { return uart_is_readable(instance); }
    bool writable() const { return uart_is_writable(instance); }
    std::uint8_t read() { return static_cast<std::uint8_t>(uart_getc(instance)); }
    void write(std::uint8_t value) { uart_get_hw(instance)->dr = value; }
    void setBaud(std::uint32_t baud) { uart_set_baudrate(instance, baud); }
};
}

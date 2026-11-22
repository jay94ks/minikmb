#pragma once
#include <cstddef>
#include <type_traits>

namespace kbm {
// Backend owns synchronization/storage. Pico implementation wraps its multicore
// queue; Windows mock uses fixed storage. There is no hidden allocation here.
template<class T, std::size_t Capacity, class Backend>
class Queue {
    static_assert(Capacity > 0);
    static_assert(std::is_trivially_copyable_v<T>);
public:
    explicit Queue(Backend backend) : backend_(backend) {}
    bool push(const T& value) { return backend_.push(&value, sizeof(T), Capacity); }
    bool pop(T& value) { return backend_.pop(&value, sizeof(T)); }
    bool full() const { return backend_.size() >= Capacity; }
    std::size_t size() const { return backend_.size(); }
    static constexpr std::size_t capacity = Capacity;
private:
    Backend backend_;
};
}

#pragma once

// Clang 22 recognizes these ACLE builtins when declared. Including all of
// arm_acle.h would collide with the SDK's barrier helpers.
#ifdef __cplusplus
extern "C" {
#endif
void __wfe(void);
void __sev(void);
void __wfi(void);
#ifdef __cplusplus
}
#endif

#include "hardware/sync.h"

// Preserve the SDK's barrier and cast while keeping the cast as the final
// statement expression value for Clang 22.
#undef remove_volatile_cast_no_barrier
#define remove_volatile_cast_no_barrier(type, value) ((type)(value))
#undef remove_volatile_cast
#define remove_volatile_cast(type, value) ({ __compiler_memory_barrier(); (type)(value); })

#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
build="$(mktemp -d)"
trap 'rm -rf -- "$build"' EXIT
fatfs=Middlewares/Third_Party/FatFs/src

# FatFs's embedded "long" ABI is 32-bit; adapt it to a 64-bit Linux host.
mkdir "$build/option"
cp "$fatfs/ff.c" "$fatfs/ff.h" "$fatfs/diskio.h" "$build/"
cp "$fatfs/option/cc949.c" "$build/option/"
sed 's/typedef long/typedef int/; s/typedef unsigned long[[:blank:]]*DWORD/typedef unsigned int DWORD/' \
    "$fatfs/integer.h" > "$build/integer.h"
# The test has a single storage caller.
sed -e '/^#include /d' -e 's/\(#define _FS_REENTRANT *\)1/\10/' \
    -e 's/ff_malloc  pvPortMalloc/ff_malloc  malloc/' -e 's/ff_free  vPortFree/ff_free  free/' \
    FATFS/Target/ffconf.h > "$build/ffconf.h"
cat > "$build/fatfs.h" <<'HEADER'
#include "ff.h"
#include "diskio.h"
enum { SD_PRESENT = 1 };
int BSP_SD_IsDetected(void);
extern char SDPath[4];
extern FATFS SDFatFS;
extern FIL SDFile;
HEADER

gcc -std=c11 -O2 -fPIC -include stdlib.h -I "$build" -c "$build/ff.c" -o "$build/ff.o"
gcc -std=c11 -O2 -fPIC -I "$build" -c "$build/option/cc949.c" -o "$build/cc949.o"
if [[ ${1:-} == --large ]]; then
    gcc -std=c11 -O2 -Wall -Wextra -Wpedantic -Wconversion -Werror -I "$build" -I Core/Inc \
        Core/Src/storage_disk.c Core/Src/optical_extents.c tools/test_storage_large.c \
        "$build/ff.o" "$build/cc949.o" -lz -o "$build/test_storage_large"
    "$build/test_storage_large"
    exit
fi

gcc -std=c11 -O2 -Wall -Wextra -Wpedantic -Wconversion -Werror -I "$build" -I Core/Inc \
    Core/Src/writable_disk.c Core/Src/storage_disk.c Core/Src/storage_card_profile.c Core/Src/optical_extents.c tools/test_storage_disk.c \
    "$build/ff.o" "$build/cc949.o" -lz -o "$build/test_storage_disk"
"$build/test_storage_disk"
printf 'FAT32 and exFAT storage, writable images and card profiles passed\n'

cat > "$build/main.h" <<'HEADER'
#include <stdlib.h>
#define Error_Handler() abort()
HEADER
cat > "$build/FreeRTOS.h" <<'HEADER'
#include <assert.h>
#include <stdint.h>
typedef uint32_t StackType_t;
typedef unsigned UBaseType_t;
typedef struct { unsigned unused; } StaticTask_t;
typedef StaticTask_t *TaskHandle_t;
#define configASSERT(expression) assert(expression)
#define pdTRUE 1
#define pdMS_TO_TICKS(milliseconds) (milliseconds)
HEADER
cat > "$build/task.h" <<'HEADER'
#define taskENTER_CRITICAL() ((void)0)
#define taskEXIT_CRITICAL() ((void)0)
#define xTaskNotifyGive(handle) ((void)(handle))
#define ulTaskNotifyTake(clear, timeout) ((void)(clear), (void)(timeout), 0U)
static inline TaskHandle_t xTaskCreateStatic(void (*fn)(void *), const char *name,
    uint32_t depth, void *argument, UBaseType_t priority, StackType_t *stack, StaticTask_t *control)
{
    (void)fn; (void)name; (void)depth; (void)argument; (void)priority; (void)stack;
    return control;
}
HEADER
cat > "$build/cmsis_os2.h" <<'HEADER'
enum { osPriorityNormal = 24 };
HEADER
cat > "$build/stm32h7xx.h" <<'HEADER'
#include <stdint.h>
extern const uint8_t test_uid[12];
#define UID_BASE test_uid
HEADER
cat > "$build/test_tusb_config.h" <<'HEADER'
#define CFG_TUSB_MCU OPT_MCU_NONE
#define CFG_TUSB_OS OPT_OS_NONE
#define CFG_TUD_ENABLED 1
#define TUP_DCD_ENDPOINT_MAX 9
#define CFG_TUD_CDC 1
#define CFG_TUD_MSC 1
#define CFG_TUD_NCM 1
#define CFG_TUD_MSC_EP_BUFSIZE 8192
HEADER
tinyusb=Middlewares/Third_Party/TinyUSB/src
gcc -std=c11 -O2 -Wall -Wextra -Wpedantic -Wconversion -Wno-sign-conversion -Werror -fPIC -shared -I "$build" -I Core/Inc \
    -DMINIKMB_TARGET_HID=0 -isystem "$tinyusb" '-DCFG_TUSB_CONFIG_FILE="test_tusb_config.h"' \
    Core/Src/storage_disk.c Core/Src/storage_card_profile.c Core/Src/optical_extents.c Core/Src/usb/management_protocol.c \
    Core/Src/writable_disk.c Core/Src/usb/target_descriptors.c Core/Src/usb/target_msc.c Core/Src/usb/management_msc.c \
    tools/test_storage_wire_fixture.c "$build/ff.o" "$build/cc949.o" -lz -o "$build/test_storage_wire.so"
python3 -B tools/test_storage_wire.py "$build/test_storage_wire.so"

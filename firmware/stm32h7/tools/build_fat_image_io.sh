#!/usr/bin/env bash
# Build the host file-image adapter with the same FatFs revision as firmware.
set -euo pipefail
out="${1:?usage: build_fat_image_io.sh OUTPUT}"
out="$(realpath -m -- "$out")"
cd "$(dirname "$0")/.."
build="$(mktemp -d)"
trap 'rm -rf -- "$build"' EXIT
fatfs=Middlewares/Third_Party/FatFs/src
mkdir "$build/option"
cp "$fatfs/ff.c" "$fatfs/ff.h" "$fatfs/diskio.h" "$build/"
cp "$fatfs/option/cc949.c" "$build/option/"
sed 's/typedef long/typedef int/; s/typedef unsigned long[[:blank:]]*DWORD/typedef unsigned int DWORD/' \
    "$fatfs/integer.h" > "$build/integer.h"
sed -e '/^#include /d' -e 's/\(#define _FS_REENTRANT *\)1/\10/' \
    FATFS/Target/ffconf.h > "$build/ffconf.h"
gcc -std=c11 -O2 -I "$build" -c "$build/ff.c" -o "$build/ff.o"
gcc -std=c11 -O2 -I "$build" -c "$build/option/cc949.c" -o "$build/cc949.o"
gcc -std=c11 -O2 -Wall -Wextra -Wpedantic -Wconversion -Werror -I "$build" \
    tools/fat_image_io.c "$build/ff.o" "$build/cc949.o" -o "$out"

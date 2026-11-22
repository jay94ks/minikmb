#ifndef MINIKMB_STORAGE_CRC32_H
#define MINIKMB_STORAGE_CRC32_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* The storage task is the only owner of the CRC peripheral. */
bool StorageCrc32_Start(void);
void StorageCrc32_Append(const uint32_t *words, size_t count);
uint32_t StorageCrc32_End(void);

#endif

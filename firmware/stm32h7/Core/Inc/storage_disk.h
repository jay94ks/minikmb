#ifndef MINIKMB_STORAGE_DISK_H
#define MINIKMB_STORAGE_DISK_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
  STORAGE_IMAGE_HEADER_BYTES = 44,
  STORAGE_IMAGE_NAME_BYTES = 120,
};

typedef struct {
  uint64_t size;
  uint32_t crc32;
  uint16_t name_length;
  uint8_t name[STORAGE_IMAGE_NAME_BYTES];
} StorageDiskImage;

/* Values shared with MiniKMB management protocol v1. */
typedef enum {
  STORAGE_DISK_OK = 0,
  STORAGE_DISK_UNSUPPORTED = 1,
  STORAGE_DISK_BAD_PAYLOAD = 2,
  STORAGE_DISK_BAD_STATE = 3,
  STORAGE_DISK_NOT_READY = 9,
  STORAGE_DISK_RANGE = 10,
  STORAGE_DISK_IO_ERROR = 11,
  STORAGE_DISK_CHECKSUM_MISMATCH = 12,
  STORAGE_DISK_CANCELLED = 13,
  STORAGE_DISK_NO_SPACE = 14,
  STORAGE_DISK_FRAGMENT_LIMIT = 15,
  STORAGE_DISK_READ_ONLY = 16,
  STORAGE_DISK_NOT_FOUND = 17,
} StorageDiskResult;

StorageDiskResult StorageDisk_DecodeImage(const uint8_t *payload, size_t length, StorageDiskImage *image);

typedef bool (*StorageDiskProgress)(void *context, uint64_t done, uint64_t total);

/* Exactly one storage task owns all disk and FatFs calls. */
bool StorageDisk_Present(void);
StorageDiskResult StorageDisk_Format(bool exfat);
StorageDiskResult StorageDisk_Begin(const StorageDiskImage *image);
StorageDiskResult StorageDisk_Write(const void *data, size_t length);
StorageDiskResult StorageDisk_Finish(StorageDiskProgress progress, void *context);
StorageDiskResult StorageDisk_Load(StorageDiskImage *image, StorageDiskProgress progress, void *context);
void StorageDisk_Invalidate(void);
StorageDiskResult StorageDisk_Read(uint64_t offset, void *buffer, uint32_t sectors);

#endif

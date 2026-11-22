#ifndef MINIKMB_WRITABLE_DISK_H
#define MINIKMB_WRITABLE_DISK_H

#include "storage_disk.h"

enum { WRITABLE_DISK_SECTOR = 512, WRITABLE_DISK_MAX_ID = 0x0FFFFFFF };

typedef enum {
  WRITABLE_FAT32 = 1, WRITABLE_EXFAT, WRITABLE_EXT4,
} WritableDiskFormat;

typedef struct {
  uint32_t id;
  uint32_t sectors;
  WritableDiskFormat format;
} WritableDiskInfo;

/* FatFs calls are owned by the storage task. Only one image file is open at a time. */
StorageDiskResult WritableDisk_Create(const WritableDiskInfo *info, StorageDiskProgress progress, void *context);
StorageDiskResult WritableDisk_Commit(uint32_t crc32, StorageDiskProgress progress, void *context);
StorageDiskResult WritableDisk_Open(uint32_t id, bool writable, WritableDiskInfo *info);
StorageDiskResult WritableDisk_Read(uint32_t lba, uint32_t count, void *data);
StorageDiskResult WritableDisk_Write(uint32_t lba, uint32_t count, const void *data);
StorageDiskResult WritableDisk_Sync(void);
StorageDiskResult WritableDisk_Close(void);
StorageDiskResult WritableDisk_List(uint32_t cursor, WritableDiskInfo items[4], uint8_t *count, uint32_t *next);
StorageDiskResult WritableDisk_Delete(uint32_t id);

#endif

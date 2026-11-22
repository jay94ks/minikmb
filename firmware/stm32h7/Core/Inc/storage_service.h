#ifndef MINIKMB_STORAGE_SERVICE_H
#define MINIKMB_STORAGE_SERVICE_H

#include "management_protocol.h"
#include "storage_disk.h"
#include "storage_card_profile.h"

enum { STORAGE_OPTICAL_READ_BYTES = 8192 };

typedef struct {
  uint64_t bytes;
  uint32_t generation;
} StorageOpticalMedia;

void StorageService_OpticalStatus(StorageOpticalMedia *media);
bool StorageService_OpticalRead(uint32_t generation, uint64_t offset, size_t bytes);
bool StorageService_OpticalTake(void *data, size_t capacity, size_t *bytes, StorageDiskResult *status);
bool StorageService_OpticalEject(void);

typedef enum {
  STORAGE_RAW_DIRECT, STORAGE_RAW_TARGET, STORAGE_RAW_MANAGEMENT
} StorageRawPath;

enum { STORAGE_RAW_IO_BYTES = 8192, STORAGE_RAW_BLOCK_BYTES = 512 };

typedef struct {
  uint32_t sectors, generation;
  bool writable;
} StorageRawMedia;

StorageRawMedia StorageService_RawStatus(StorageRawPath path);
bool StorageService_RawSubmit(StorageRawPath path, uint32_t generation, uint32_t lba,
                              size_t bytes, const void *write_data);
bool StorageService_RawTake(StorageRawPath path, uint32_t generation, void *read_data,
                            size_t *bytes, StorageDiskResult *status, bool *write);
bool StorageService_RawEject(StorageRawPath path, uint32_t generation);

bool StorageService_OpticalEnabled(void);
void StorageService_OpticalApplied(bool enabled);
void StorageService_Start(void);
void StorageService_SetUid(const uint8_t uid[STORAGE_CONTROLLER_UID_BYTES]);
const ManagementStorageBackend *StorageService_Backend(void);

#endif

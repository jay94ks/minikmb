#ifndef MINIKMB_STORAGE_CARD_PROFILE_H
#define MINIKMB_STORAGE_CARD_PROFILE_H

#include "storage_disk.h"

enum { STORAGE_BOARD_ID_BYTES = 16, STORAGE_CONTROLLER_UID_BYTES = 12, STORAGE_USER_ID_BYTES = 4096, STORAGE_CHANNEL_NAME_BYTES = 64 };

typedef struct {
  uint8_t board_id[STORAGE_BOARD_ID_BYTES];
  uint8_t controller_uid[STORAGE_CONTROLLER_UID_BYTES];
} StorageCardProfile;

/* All calls belong to the SD worker while no FatFs file is open elsewhere. */
StorageDiskResult StorageCard_LoadProfile(StorageCardProfile *profile);
StorageDiskResult StorageCard_EnsureProfile(const uint8_t uid[STORAGE_CONTROLLER_UID_BYTES], StorageCardProfile *profile);
StorageDiskResult StorageCard_InitProfile(const StorageCardProfile *profile);
bool StorageCard_ValidName(const uint8_t *name, size_t length);
StorageDiskResult StorageCard_LoadName(uint8_t *name, size_t capacity, size_t *length);
StorageDiskResult StorageCard_StoreName(const uint8_t uid[STORAGE_CONTROLLER_UID_BYTES], const uint8_t *name, size_t length);
StorageDiskResult StorageCard_LoadUserId(void *data, size_t capacity, size_t *length);
StorageDiskResult StorageCard_StoreUserId(const void *data, size_t length);

#endif

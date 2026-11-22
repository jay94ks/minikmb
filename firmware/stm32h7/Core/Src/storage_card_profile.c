#include "storage_card_profile.h"
#include "fatfs.h"

#include <string.h>

enum { BOARD_BYTES = 40, NAMED_BOARD_BYTES = 116, USER_HEADER = 16 };

static const TCHAR folder[] = "0:/MINIKBM";
static const TCHAR board_path[] = "0:/MINIKBM/BOARD.ID";
static const TCHAR board_temp[] = "0:/MINIKBM/BOARD.TMP";
static const TCHAR board_backup[] = "0:/MINIKBM/BOARD.BAK";
static const TCHAR *const user_paths[2] = {"0:/MINIKBM/USER0.BIN", "0:/MINIKBM/USER1.BIN"};
static const uint8_t board_magic[8] = {'M', 'I', 'N', 'I', 'K', 'M', 'B', 1};
static const uint8_t user_magic[4] = {'M', 'K', 'U', 1};

static uint8_t user_record[USER_HEADER + STORAGE_USER_ID_BYTES]
    __attribute__((section(".axi_files"), aligned(32)));

_Static_assert(sizeof(StorageCardProfile) + sizeof(board_magic) + 4U == BOARD_BYTES, "card profile layout");

static uint32_t ReadLe32(const uint8_t *p)
{
  return p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static void PutLe32(uint8_t *p, uint32_t value)
{
  for (unsigned i = 0; i < 4; ++i) {
    p[i] = (uint8_t)value;
    value >>= 8;
  }
}

static uint32_t Crc32(uint32_t crc, const uint8_t *data, size_t size)
{
  for (size_t n = 0; n < size; ++n) {
    crc ^= data[n];
    for (unsigned bit = 0; bit < 8; ++bit) {
      crc = (crc >> 1) ^ (0xEDB88320U & (0U - (crc & 1U)));
    }
  }
  return crc;
}

static StorageDiskResult DiskResult(FRESULT result)
{
  if (result == FR_OK) return STORAGE_DISK_OK;
  if (result == FR_NOT_READY || !StorageDisk_Present()) return STORAGE_DISK_NOT_READY;
  if (result == FR_NO_FILE || result == FR_NO_PATH) return STORAGE_DISK_NOT_FOUND;
  return STORAGE_DISK_IO_ERROR;
}

static StorageDiskResult Mount(void)
{
  if (!StorageDisk_Present()) return STORAGE_DISK_NOT_READY;
  const FRESULT result = f_mount(&SDFatFS, SDPath, 1);
  if (result != FR_OK) (void)f_mount(NULL, SDPath, 0);
  return DiskResult(result);
}

static StorageDiskResult EnsureFolder(void)
{
  const FRESULT result = f_mkdir(folder);
  return result == FR_EXIST ? STORAGE_DISK_OK : DiskResult(result);
}

static StorageDiskResult ReadFile(const TCHAR *path, uint8_t *data, size_t capacity, size_t *size)
{
  FRESULT result = f_open(&SDFile, path, FA_READ);
  if (result != FR_OK) return DiskResult(result);
  const FSIZE_t length = f_size(&SDFile);
  UINT transferred = 0;
  if (length <= capacity) result = f_read(&SDFile, data, (UINT)length, &transferred);
  const FRESULT close = f_close(&SDFile);
  if (result != FR_OK) return DiskResult(result);
  if (close != FR_OK) return DiskResult(close);
  if (length > capacity || transferred != length) return STORAGE_DISK_CHECKSUM_MISMATCH;
  *size = transferred;
  return STORAGE_DISK_OK;
}

static StorageDiskResult WriteFile(const TCHAR *path, const uint8_t *data, size_t size)
{
  FRESULT result = f_open(&SDFile, path, FA_CREATE_ALWAYS | FA_WRITE);
  if (result != FR_OK) return DiskResult(result);
  UINT transferred;
  result = f_write(&SDFile, data, (UINT)size, &transferred);
  const FRESULT close = f_close(&SDFile);
  if (result != FR_OK) return DiskResult(result);
  if (close != FR_OK) return DiskResult(close);
  return transferred == size ? STORAGE_DISK_OK : STORAGE_DISK_NO_SPACE;
}

bool StorageCard_ValidName(const uint8_t *name, size_t length)
{
  if (length > STORAGE_CHANNEL_NAME_BYTES) return false;
  for (size_t i = 0; i < length;) {
    const uint8_t c = name[i++];
    if (c < 0x20U || c == 0x7FU) return false;
    if (c < 0x80U) continue;
    unsigned count; uint32_t value, minimum;
    if (c >= 0xC2U && c <= 0xDFU) { count = 1; value = c & 0x1FU; minimum = 0x80; }
    else if (c >= 0xE0U && c <= 0xEFU) { count = 2; value = c & 0x0FU; minimum = 0x800; }
    else if (c >= 0xF0U && c <= 0xF4U) { count = 3; value = c & 7U; minimum = 0x10000; }
    else return false;
    if (count > length - i) return false;
    while (count-- != 0U) {
      const uint8_t next = name[i++];
      if ((next & 0xC0U) != 0x80U) return false;
      value = (value << 6) | (next & 0x3FU);
    }
    if (value < minimum || value > 0x10FFFFU || (value >= 0xD800U && value <= 0xDFFFU) ||
        (value >= 0x80U && value <= 0x9FU) || value == 0x2028U || value == 0x2029U) return false;
  }
  return true;
}

static StorageDiskResult BoardRecordAt(const TCHAR *path, uint8_t bytes[NAMED_BOARD_BYTES], size_t *size)
{
  StorageDiskResult result = ReadFile(path, bytes, NAMED_BOARD_BYTES, size);
  if (result != STORAGE_DISK_OK) return result;
  if ((*size != BOARD_BYTES && *size != NAMED_BOARD_BYTES) ||
      memcmp(bytes, board_magic, sizeof(board_magic)) != 0 ||
      ReadLe32(bytes + BOARD_BYTES - 4U) != ~Crc32(UINT32_MAX, bytes, BOARD_BYTES - 4U)) return STORAGE_DISK_CHECKSUM_MISMATCH;
  if (*size == NAMED_BOARD_BYTES) {
    const size_t length = (size_t)bytes[44] | ((size_t)bytes[45] << 8);
    if (memcmp(bytes + 40, "MKN1", 4) != 0 || bytes[46] != 0U || bytes[47] != 0U ||
        ReadLe32(bytes + 112) != ~Crc32(UINT32_MAX, bytes, 112) ||
        !StorageCard_ValidName(bytes + 48, length)) return STORAGE_DISK_CHECKSUM_MISMATCH;
  }
  return STORAGE_DISK_OK;
}

static StorageDiskResult BoardRecord(uint8_t bytes[NAMED_BOARD_BYTES], size_t *size)
{
  const TCHAR *const paths[] = {board_path, board_backup, board_temp};
  StorageDiskResult status = STORAGE_DISK_NOT_FOUND;
  for (size_t i = 0; i < sizeof(paths) / sizeof(paths[0]); ++i) {
    const StorageDiskResult result = BoardRecordAt(paths[i], bytes, size);
    if (result == STORAGE_DISK_OK) return result;
    if (result != STORAGE_DISK_NOT_FOUND && result != STORAGE_DISK_CHECKSUM_MISMATCH) return result;
    if (result == STORAGE_DISK_CHECKSUM_MISMATCH) status = result;
  }
  return status;
}

static StorageDiskResult BoardProfile(StorageCardProfile *profile)
{
  uint8_t bytes[NAMED_BOARD_BYTES]; size_t size;
  const StorageDiskResult result = BoardRecord(bytes, &size);
  if (result == STORAGE_DISK_OK) memcpy(profile, bytes + sizeof(board_magic), sizeof(*profile));
  return result;
}

StorageDiskResult StorageCard_LoadName(uint8_t *name, size_t capacity, size_t *length)
{
  *length = 0;
  StorageDiskResult result = Mount();
  if (result != STORAGE_DISK_OK) return result;
  uint8_t bytes[NAMED_BOARD_BYTES]; size_t size;
  result = BoardRecord(bytes, &size);
  if (result == STORAGE_DISK_OK && size == NAMED_BOARD_BYTES) {
    *length = (size_t)bytes[44] | ((size_t)bytes[45] << 8);
    if (*length > capacity) result = STORAGE_DISK_RANGE;
    else if (*length != 0U) memcpy(name, bytes + 48, *length);
  }
  (void)f_mount(NULL, SDPath, 0);
  return result;
}

StorageDiskResult StorageCard_StoreName(const uint8_t uid[STORAGE_CONTROLLER_UID_BYTES], const uint8_t *name, size_t length)
{
  if (!StorageCard_ValidName(name, length)) return STORAGE_DISK_BAD_PAYLOAD;
  StorageDiskResult result = Mount();
  if (result != STORAGE_DISK_OK) return result;
  uint8_t bytes[NAMED_BOARD_BYTES] = {0}; size_t size;
  result = BoardRecord(bytes, &size);
  if (result == STORAGE_DISK_OK && memcmp(bytes + 24, uid, STORAGE_CONTROLLER_UID_BYTES) != 0) result = STORAGE_DISK_BAD_STATE;
  if (result == STORAGE_DISK_OK) {
    memset(bytes + BOARD_BYTES, 0, NAMED_BOARD_BYTES - BOARD_BYTES);
    memcpy(bytes + 40, "MKN1", 4); bytes[44] = (uint8_t)length;
    if (length != 0U) memcpy(bytes + 48, name, length);
    PutLe32(bytes + 112, ~Crc32(UINT32_MAX, bytes, 112));
    result = WriteFile(board_temp, bytes, sizeof(bytes));
    if (result == STORAGE_DISK_OK) {
      FILINFO info;
      FRESULT present = f_stat(board_path, &info);
      if (present == FR_OK) {
        FRESULT prior = f_unlink(board_backup);
        if (prior == FR_OK || prior == FR_NO_FILE) prior = f_rename(board_path, board_backup);
        result = DiskResult(prior);
      } else if (present != FR_NO_FILE) result = DiskResult(present);
      if (result == STORAGE_DISK_OK) result = DiskResult(f_rename(board_temp, board_path));
    }
  }
  (void)f_mount(NULL, SDPath, 0);
  return result;
}

StorageDiskResult StorageCard_LoadProfile(StorageCardProfile *profile)
{
  StorageDiskResult result = Mount();
  if (result != STORAGE_DISK_OK) return result;
  result = BoardProfile(profile);
  (void)f_mount(NULL, SDPath, 0);
  return result;
}

StorageDiskResult StorageCard_InitProfile(const StorageCardProfile *profile)
{
  static const uint8_t empty_id[STORAGE_BOARD_ID_BYTES] = {0};
  if (memcmp(empty_id, profile->board_id, sizeof(empty_id)) == 0) return STORAGE_DISK_BAD_PAYLOAD;
  StorageDiskResult result = Mount();
  if (result != STORAGE_DISK_OK) return result;
  StorageCardProfile existing;
  result = BoardProfile(&existing);
  if (result == STORAGE_DISK_OK && memcmp(&existing, profile, sizeof(existing)) != 0) {
    result = STORAGE_DISK_BAD_STATE;
  } else if (result == STORAGE_DISK_NOT_FOUND) {
    result = EnsureFolder();
    if (result == STORAGE_DISK_OK) {
      uint8_t bytes[BOARD_BYTES];
      memcpy(bytes, board_magic, sizeof(board_magic));
      memcpy(bytes + sizeof(board_magic), profile, sizeof(*profile));
      PutLe32(bytes + BOARD_BYTES - 4U, ~Crc32(UINT32_MAX, bytes, BOARD_BYTES - 4U));
      result = WriteFile(board_temp, bytes, sizeof(bytes));
      if (result == STORAGE_DISK_OK) result = DiskResult(f_rename(board_temp, board_path));
    }
  }
  (void)f_mount(NULL, SDPath, 0);
  return result;
}

StorageDiskResult StorageCard_EnsureProfile(const uint8_t uid[STORAGE_CONTROLLER_UID_BYTES], StorageCardProfile *profile)
{
  StorageDiskResult result = StorageCard_LoadProfile(profile);
  if (result != STORAGE_DISK_NOT_FOUND) return result;
  /* Stable board identity independent of management USB/RNG availability.
     Preserve any existing board ID, including legacy randomly assigned IDs. */
  memcpy(profile->controller_uid, uid, STORAGE_CONTROLLER_UID_BYTES);
  memcpy(profile->board_id, uid, STORAGE_CONTROLLER_UID_BYTES);
  memcpy(profile->board_id + STORAGE_CONTROLLER_UID_BYTES, "MKB1", 4);
  result = StorageCard_InitProfile(profile);
  return result;
}

static uint32_t UserChecksum(size_t length)
{
  const uint32_t header_crc = Crc32(UINT32_MAX, user_record, 12);
  return ~Crc32(header_crc, user_record + USER_HEADER, length);
}

static StorageDiskResult UserSlot(unsigned slot, uint32_t *generation, size_t *length)
{
  size_t size;
  const StorageDiskResult result = ReadFile(user_paths[slot], user_record, sizeof(user_record), &size);
  if (result != STORAGE_DISK_OK) return result;
  if (size < USER_HEADER || memcmp(user_record, user_magic, sizeof(user_magic)) != 0) {
    return STORAGE_DISK_CHECKSUM_MISMATCH;
  }
  const uint32_t data_size = ReadLe32(user_record + 8);
  const uint32_t seq = ReadLe32(user_record + 4);
  if (seq == 0U || data_size != size - USER_HEADER || UserChecksum(data_size) != ReadLe32(user_record + 12)) {
    return STORAGE_DISK_CHECKSUM_MISMATCH;
  }
  *generation = seq;
  *length = data_size;
  return STORAGE_DISK_OK;
}

static StorageDiskResult NewestUserSlot(unsigned *slot, uint32_t *generation)
{
  uint32_t sequence[2] = {0, 0};
  bool corrupt = false;
  for (unsigned i = 0; i < 2; ++i) {
    size_t length;
    const StorageDiskResult result = UserSlot(i, &sequence[i], &length);
    if (result == STORAGE_DISK_CHECKSUM_MISMATCH) {
      corrupt = true;
    } else if (result != STORAGE_DISK_OK && result != STORAGE_DISK_NOT_FOUND) {
      return result;
    }
  }
  *slot = sequence[1] > sequence[0] ? 1U : 0U;
  *generation = sequence[*slot];
  if (*generation != 0U) return STORAGE_DISK_OK;
  return corrupt ? STORAGE_DISK_CHECKSUM_MISMATCH : STORAGE_DISK_NOT_FOUND;
}

StorageDiskResult StorageCard_LoadUserId(void *data, size_t capacity, size_t *length)
{
  *length = 0;
  StorageDiskResult result = Mount();
  if (result != STORAGE_DISK_OK) return result;
  unsigned slot;
  uint32_t generation;
  size_t length_on_card;
  result = NewestUserSlot(&slot, &generation);
  if (result == STORAGE_DISK_OK) result = UserSlot(slot, &generation, &length_on_card);
  if (result == STORAGE_DISK_OK) {
    *length = length_on_card;
    if (capacity < length_on_card) result = STORAGE_DISK_RANGE;
    else if (length_on_card != 0U) memcpy(data, user_record + USER_HEADER, length_on_card);
  }
  (void)f_mount(NULL, SDPath, 0);
  return result;
}

StorageDiskResult StorageCard_StoreUserId(const void *data, size_t length)
{
  if (length > STORAGE_USER_ID_BYTES) return STORAGE_DISK_RANGE;
  StorageDiskResult result = Mount();
  if (result != STORAGE_DISK_OK) return result;
  unsigned current_slot;
  uint32_t generation = 0;
  result = NewestUserSlot(&current_slot, &generation);
  if (result == STORAGE_DISK_OK || result == STORAGE_DISK_NOT_FOUND ||
      result == STORAGE_DISK_CHECKSUM_MISMATCH) {
    result = EnsureFolder();
    if (result == STORAGE_DISK_OK && generation == UINT32_MAX) {
      result = STORAGE_DISK_BAD_STATE;
    } else if (result == STORAGE_DISK_OK) {
      const unsigned slot = generation == 0U ? 0U : current_slot ^ 1U;
      memcpy(user_record, user_magic, sizeof(user_magic));
      PutLe32(user_record + 4, generation + 1U);
      PutLe32(user_record + 8, (uint32_t)length);
      if (length != 0U) memcpy(user_record + USER_HEADER, data, length);
      PutLe32(user_record + 12, UserChecksum(length));
      result = WriteFile(user_paths[slot], user_record, USER_HEADER + length);
    }
  }
  (void)f_mount(NULL, SDPath, 0);
  return result;
}

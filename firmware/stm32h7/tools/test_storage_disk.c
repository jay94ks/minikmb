#include "fatfs.h"
#include "storage_crc32.h"
#include "storage_card_profile.h"
#include "writable_disk.h"
#include "storage_disk.h"
#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <zlib.h>

enum { BLOCK = 512, SECTORS = 160000, IMAGE_BYTES = 40 * 1024 };
static uint8_t *card;
static uint8_t payload[IMAGE_BYTES];
static uint8_t readback[IMAGE_BYTES];
static bool inserted = true;
static unsigned nonadjacent_reads;
static DWORD next_read;
static bool trace_reads;
static uLong crc;
FATFS SDFatFS;
FIL SDFile;
char SDPath[4] = "0:";

int BSP_SD_IsDetected(void) { return inserted ? SD_PRESENT : 0; }
DSTATUS disk_initialize(BYTE drive) { return disk_status(drive); }
DSTATUS disk_status(BYTE drive) { return drive == 0 && inserted ? 0 : STA_NOINIT | STA_NODISK; }
DRESULT disk_read(BYTE drive, BYTE *buffer, DWORD sector, UINT count)
{
  if (disk_status(drive) != 0) return RES_NOTRDY;
  if (!count || sector >= SECTORS || count > SECTORS - sector) return RES_PARERR;
  if (trace_reads) {
    if (sector != next_read) ++nonadjacent_reads;
    next_read = sector + count;
  }
  memcpy(buffer, card + (size_t)sector * BLOCK, (size_t)count * BLOCK);
  return RES_OK;
}
DRESULT disk_write(BYTE drive, const BYTE *buffer, DWORD sector, UINT count)
{
  if (disk_status(drive) != 0) return RES_NOTRDY;
  if (!count || sector >= SECTORS || count > SECTORS - sector) return RES_PARERR;
  memcpy(card + (size_t)sector * BLOCK, buffer, (size_t)count * BLOCK);
  return RES_OK;
}
DRESULT disk_ioctl(BYTE drive, BYTE command, void *buffer)
{
  if (disk_status(drive) != 0) return RES_NOTRDY;
  switch (command) {
    case CTRL_SYNC: return RES_OK;
    case GET_SECTOR_COUNT: *(DWORD *)buffer = SECTORS; return RES_OK;
    case GET_SECTOR_SIZE: *(WORD *)buffer = BLOCK; return RES_OK;
    case GET_BLOCK_SIZE: *(DWORD *)buffer = 1; return RES_OK;
    default: return RES_PARERR;
  }
}
void *ff_memalloc(UINT bytes) { return malloc(bytes); }
void ff_memfree(void *buffer) { free(buffer); }
DWORD get_fattime(void) { return ((2026U - 1980U) << 25U) | (1U << 21U) | (1U << 16U); }
bool StorageCrc32_Start(void) { crc = crc32(0L, Z_NULL, 0); return true; }
void StorageCrc32_Append(const uint32_t *words, size_t count)
{
  crc = crc32(crc, (const Bytef *)words, (uInt)(count * sizeof(*words)));
}
uint32_t StorageCrc32_End(void) { return (uint32_t)crc; }

static bool CancelAfterFirstRead(void *context, uint64_t done, uint64_t total)
{
  (void)context;
  assert(total == IMAGE_BYTES);
  return done == 0U;
}

static void WriteImage(const StorageDiskImage *image, bool fragment)
{
  assert(StorageDisk_Begin(image) == STORAGE_DISK_OK);
  for (unsigned offset = 0; offset < IMAGE_BYTES; offset += 4096) {
    assert(StorageDisk_Write(payload + offset, 4096) == STORAGE_DISK_OK);
    if (fragment && offset == 0) {
      FIL gap;
      UINT count;
      assert(f_open(&gap, "0:/GAP.BIN", FA_CREATE_ALWAYS | FA_WRITE) == FR_OK);
      assert(f_write(&gap, payload, sizeof(payload), &count) == FR_OK && count == sizeof(payload));
      assert(f_close(&gap) == FR_OK);
    }
  }
}


static void ExerciseCardProfile(void)
{
  const StorageCardProfile identity = {
    .board_id = {0xF1, 0xEA, 0xDC, 0xCA},
    .controller_uid = {0x25, 0xAD, 0xCB},
  };
  StorageCardProfile loaded;
  assert(StorageCard_LoadProfile(&loaded) == STORAGE_DISK_NOT_FOUND);
  assert(StorageCard_InitProfile(&identity) == STORAGE_DISK_OK);
  assert(StorageCard_InitProfile(&identity) == STORAGE_DISK_OK);
  assert(StorageCard_LoadProfile(&loaded) == STORAGE_DISK_OK);
  assert(memcmp(&loaded, &identity, sizeof(loaded)) == 0);
  loaded.controller_uid[0] ^= 1;
  assert(StorageCard_InitProfile(&loaded) == STORAGE_DISK_BAD_STATE);
  assert(StorageCard_LoadProfile(&loaded) == STORAGE_DISK_OK);
  assert(memcmp(&loaded, &identity, sizeof(loaded)) == 0);

  size_t length;
  uint8_t id[STORAGE_USER_ID_BYTES];
  assert(StorageCard_LoadUserId(id, sizeof(id), &length) == STORAGE_DISK_NOT_FOUND);
  for (unsigned i = 0; i < sizeof(id); ++i) id[i] = (uint8_t)i;
  assert(StorageCard_StoreUserId(id, sizeof(id) + 1U) == STORAGE_DISK_RANGE);
  assert(StorageCard_StoreUserId(id, sizeof(id)) == STORAGE_DISK_OK);
  assert(StorageCard_LoadUserId(readback, 1, &length) == STORAGE_DISK_RANGE);
  assert(length == sizeof(id));
  assert(StorageCard_LoadUserId(readback, sizeof(readback), &length) == STORAGE_DISK_OK);
  assert(length == sizeof(id) && memcmp(readback, id, length) == 0);
  assert(StorageCard_StoreUserId("new", 3) == STORAGE_DISK_OK);
  assert(StorageCard_LoadUserId(readback, sizeof(readback), &length) == STORAGE_DISK_OK);
  assert(length == 3 && memcmp(readback, "new", 3) == 0);

  assert(f_mount(&SDFatFS, SDPath, 1) == FR_OK);
  assert(f_open(&SDFile, "0:/MINIKBM/USER1.BIN", FA_WRITE | FA_CREATE_ALWAYS) == FR_OK);
  UINT transferred;
  assert(f_write(&SDFile, "broken", 6, &transferred) == FR_OK && transferred == 6);
  assert(f_close(&SDFile) == FR_OK);
  assert(f_mount(NULL, SDPath, 0) == FR_OK);
  assert(StorageCard_LoadUserId(readback, sizeof(readback), &length) == STORAGE_DISK_OK);
  assert(length == sizeof(id) && memcmp(readback, id, length) == 0);
  assert(StorageCard_StoreUserId(NULL, 0) == STORAGE_DISK_OK);
  assert(StorageCard_LoadUserId(NULL, 0, &length) == STORAGE_DISK_OK && length == 0);
  assert(StorageCard_LoadProfile(&loaded) == STORAGE_DISK_OK);
  assert(memcmp(&loaded, &identity, sizeof(loaded)) == 0);
  uint8_t name[STORAGE_CHANNEL_NAME_BYTES]; size_t name_length;
  assert(StorageCard_LoadName(name, sizeof(name), &name_length) == STORAGE_DISK_OK && name_length == 0U);
  const uint8_t title[] = "서버 A";
  assert(StorageCard_StoreName(identity.controller_uid, title, sizeof(title) - 1U) == STORAGE_DISK_OK);
  assert(StorageCard_LoadName(name, sizeof(name), &name_length) == STORAGE_DISK_OK);
  assert(name_length == sizeof(title) - 1U && memcmp(name, title, name_length) == 0);
  assert(StorageCard_LoadProfile(&loaded) == STORAGE_DISK_OK && memcmp(&loaded, &identity, sizeof(loaded)) == 0);
  uint8_t wrong[STORAGE_CONTROLLER_UID_BYTES] = {0};
  assert(StorageCard_StoreName(wrong, title, sizeof(title) - 1U) == STORAGE_DISK_BAD_STATE);
  const uint8_t invalid[] = {0xC0, 0xAF};
  assert(StorageCard_StoreName(identity.controller_uid, invalid, sizeof(invalid)) == STORAGE_DISK_BAD_PAYLOAD);
  assert(StorageCard_StoreName(identity.controller_uid, (const uint8_t *)"bad\nname", 8) == STORAGE_DISK_BAD_PAYLOAD);
  assert(StorageCard_StoreName(identity.controller_uid, name, sizeof(name) + 1U) == STORAGE_DISK_BAD_PAYLOAD);
  assert(StorageCard_StoreName(identity.controller_uid, (const uint8_t *)"second", 6) == STORAGE_DISK_OK);
  /* Simulate interrupted replacement: valid BOARD.BAK, missing BOARD.ID. */
  assert(f_mount(&SDFatFS, SDPath, 1) == FR_OK);
  assert(f_unlink("0:/MINIKBM/BOARD.ID") == FR_OK);
  assert(f_mount(NULL, SDPath, 0) == FR_OK);
  assert(StorageCard_LoadName(name, sizeof(name), &name_length) == STORAGE_DISK_OK);
  assert(name_length == sizeof(title) - 1U && memcmp(name, title, name_length) == 0);
  assert(StorageCard_StoreName(identity.controller_uid, (const uint8_t *)"recovered", 9) == STORAGE_DISK_OK);
  assert(StorageCard_LoadName(name, sizeof(name), &name_length) == STORAGE_DISK_OK && name_length == 9);
  assert(StorageCard_LoadUserId(NULL, 0, &length) == STORAGE_DISK_OK && length == 0);
  assert(StorageCard_StoreName(identity.controller_uid, NULL, 0) == STORAGE_DISK_OK);
  assert(StorageCard_LoadName(name, sizeof(name), &name_length) == STORAGE_DISK_OK && name_length == 0);

}


static void ExerciseWritableImage(void)
{
  const WritableDiskInfo metadata = {.id = 0x23456, .sectors = 4, .format = WRITABLE_EXFAT};
  WritableDiskInfo image;
  uint8_t data[4 * BLOCK] = {0};
  memcpy(data + BLOCK, payload, BLOCK);
  assert(WritableDisk_Create(&metadata, NULL, NULL) == STORAGE_DISK_OK);
  assert(WritableDisk_Write(1, 1, payload) == STORAGE_DISK_OK);
  assert(WritableDisk_Commit((uint32_t)crc32(0, data, sizeof(data)), NULL, NULL) == STORAGE_DISK_OK);

  assert(WritableDisk_Open(metadata.id, false, &image) == STORAGE_DISK_OK);
  assert(image.id == metadata.id && image.sectors == metadata.sectors && image.format == metadata.format);
  assert(WritableDisk_Read(0, 4, readback) == STORAGE_DISK_OK);
  assert(memcmp(data, readback, sizeof(data)) == 0);
  assert(WritableDisk_Write(1, 1, data) == STORAGE_DISK_READ_ONLY);
  assert(WritableDisk_Close() == STORAGE_DISK_OK);

  assert(WritableDisk_Open(metadata.id, true, &image) == STORAGE_DISK_OK);
  assert(WritableDisk_Write(2, 3, payload) == STORAGE_DISK_RANGE);
  assert(WritableDisk_Write(1, 1, data) == STORAGE_DISK_OK);
  assert(WritableDisk_Sync() == STORAGE_DISK_OK);
  assert(WritableDisk_Close() == STORAGE_DISK_OK);
  assert(WritableDisk_Open(metadata.id, false, &image) == STORAGE_DISK_OK);
  assert(WritableDisk_Read(1, 1, readback) == STORAGE_DISK_OK);
  assert(memcmp(readback, data, BLOCK) == 0);
  assert(WritableDisk_Close() == STORAGE_DISK_OK);

  /* Enough images to extend the writable directory on a 512-byte-cluster card. */
  for (uint32_t id = 1; id <= 6; ++id) {
    const WritableDiskInfo other = {.id = id, .sectors = 1, .format = WRITABLE_FAT32};
    assert(WritableDisk_Create(&other, NULL, NULL) == STORAGE_DISK_OK);
    assert(WritableDisk_Commit((uint32_t)crc32(0, data, BLOCK), NULL, NULL) == STORAGE_DISK_OK);
    assert(WritableDisk_Open(other.id, false, &image) == STORAGE_DISK_OK);
    assert(WritableDisk_Close() == STORAGE_DISK_OK);
  }
  WritableDiskInfo entries[4]; uint8_t count; uint32_t cursor = 0, next, total = 0;
  do {
    assert(WritableDisk_List(cursor, entries, &count, &next) == STORAGE_DISK_OK);
    assert(count <= 4U && (next == 0U || next > cursor));
    total += count; cursor = next;
  } while (cursor != 0U);
  assert(total == 7U);
  assert(WritableDisk_Open(metadata.id, false, &image) == STORAGE_DISK_OK);
  assert(WritableDisk_List(0, entries, &count, &next) == STORAGE_DISK_OK && count == 4U);
  assert(WritableDisk_Delete(6U) == STORAGE_DISK_BAD_STATE);
  assert(WritableDisk_Close() == STORAGE_DISK_OK);
  assert(WritableDisk_Delete(6U) == STORAGE_DISK_OK);
  assert(WritableDisk_Open(6U, false, &image) == STORAGE_DISK_NOT_FOUND);
  assert(WritableDisk_Open(metadata.id, false, &image) == STORAGE_DISK_OK);
  assert(WritableDisk_Close() == STORAGE_DISK_OK);
}

static void ExerciseVolume(BYTE format)
{
  uint8_t mkfs_buffer[8192];
  assert(f_mkfs(SDPath, format | FM_SFD, BLOCK, mkfs_buffer, sizeof(mkfs_buffer)) == FR_OK);
  assert(f_mount(&SDFatFS, SDPath, 1) == FR_OK);
  assert(SDFatFS.fs_type == (format == FM_EXFAT ? FS_EXFAT : FS_FAT32));
  assert(f_mount(NULL, SDPath, 0) == FR_OK);

  ExerciseCardProfile();
  ExerciseWritableImage();

  StorageDiskImage loaded;
  const StorageDiskImage image = {
    .size = IMAGE_BYTES, .crc32 = (uint32_t)crc32(0L, payload, IMAGE_BYTES),
    .name_length = 10, .name = "sample.iso",
  };
  assert(StorageDisk_Load(&loaded, NULL, NULL) == STORAGE_DISK_NOT_FOUND);
  StorageDiskImage large = image;
  large.size = UINT64_C(4294967296);
  assert(StorageDisk_Begin(&large) == STORAGE_DISK_RANGE);

  WriteImage(&image, true);
  assert(StorageDisk_Finish(NULL, NULL) == STORAGE_DISK_OK);
  trace_reads = true;
  nonadjacent_reads = 0;
  next_read = 0;
  assert(StorageDisk_Read(0, readback, IMAGE_BYTES / BLOCK) == STORAGE_DISK_OK);
  trace_reads = false;
  assert(nonadjacent_reads >= 2U);
  assert(memcmp(readback, payload, sizeof(payload)) == 0);
  assert(StorageDisk_Read(IMAGE_BYTES, readback, 1) == STORAGE_DISK_RANGE);
  assert(StorageDisk_Read(1, readback, 1) == STORAGE_DISK_RANGE);

  inserted = false;
  assert(StorageDisk_Read(0, readback, 1) == STORAGE_DISK_NOT_READY);
  assert(StorageDisk_Load(&loaded, NULL, NULL) == STORAGE_DISK_NOT_READY);
  inserted = true;
  assert(StorageDisk_Load(&loaded, NULL, NULL) == STORAGE_DISK_OK);
  assert(loaded.size == image.size && loaded.crc32 == image.crc32 &&
         loaded.name_length == image.name_length && memcmp(loaded.name, image.name, image.name_length) == 0);

  assert(StorageDisk_Begin(&image) == STORAGE_DISK_OK);
  assert(StorageDisk_Write(payload, 1008) == STORAGE_DISK_OK);
  StorageDisk_Invalidate();
  assert(StorageDisk_Read(0, readback, 1) == STORAGE_DISK_BAD_STATE);
  assert(StorageDisk_Load(&loaded, NULL, NULL) == STORAGE_DISK_NOT_FOUND);

  WriteImage(&image, false);
  assert(StorageDisk_Finish(CancelAfterFirstRead, NULL) == STORAGE_DISK_CANCELLED);
  assert(StorageDisk_Load(&loaded, NULL, NULL) == STORAGE_DISK_NOT_FOUND);

  StorageDiskImage wrong_crc = image;
  wrong_crc.crc32 ^= 1U;
  WriteImage(&wrong_crc, false);
  assert(StorageDisk_Finish(NULL, NULL) == STORAGE_DISK_CHECKSUM_MISMATCH);
  assert(StorageDisk_Read(0, readback, 1) == STORAGE_DISK_BAD_STATE);
  assert(StorageDisk_Load(&loaded, NULL, NULL) == STORAGE_DISK_NOT_FOUND);
}

static void ExerciseBeginPayload(void)
{
  uint8_t bytes[49] = {
    [1] = 8, [8] = 0x12, [9] = 0x34, [10] = 0x56, [11] = 0x78, [40] = 1, [42] = 5,
    [44] = 0xEA, [45] = 0xB0, [46] = 0x80, [47] = '.', [48] = 'x',
  };
  StorageDiskImage image;
  assert(StorageDisk_DecodeImage(bytes, sizeof(bytes), &image) == STORAGE_DISK_OK);
  assert(image.size == 2048 && image.crc32 == 0x78563412U && image.name_length == 5);
  assert(StorageDisk_DecodeImage(bytes, sizeof(bytes) - 1, &image) == STORAGE_DISK_BAD_PAYLOAD);
  bytes[12] = 1;
  assert(StorageDisk_DecodeImage(bytes, sizeof(bytes), &image) == STORAGE_DISK_BAD_PAYLOAD);
  bytes[12] = 0;
  bytes[48] = '/';
  assert(StorageDisk_DecodeImage(bytes, sizeof(bytes), &image) == STORAGE_DISK_BAD_PAYLOAD);
  bytes[48] = 'x';
  bytes[45] = 0x41;
  assert(StorageDisk_DecodeImage(bytes, sizeof(bytes), &image) == STORAGE_DISK_BAD_PAYLOAD);
  bytes[45] = 0xB0;
  bytes[40] = 2;
  assert(StorageDisk_DecodeImage(bytes, sizeof(bytes), &image) == STORAGE_DISK_UNSUPPORTED);
}

int main(void)
{
  card = calloc(SECTORS, BLOCK);
  assert(card != NULL);
  for (size_t i = 0; i < sizeof(payload); ++i) payload[i] = (uint8_t)(i ^ (i >> 5U));
  ExerciseBeginPayload();
  ExerciseVolume(FM_FAT32);
  memset(card, 0, (size_t)SECTORS * BLOCK);
  ExerciseVolume(FM_EXFAT);
  free(card);
  return 0;
}

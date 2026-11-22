#define _GNU_SOURCE
#define _FILE_OFFSET_BITS 64
#include "fatfs.h"
#include "storage_crc32.h"
#include "storage_disk.h"

#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <zlib.h>

enum { BLOCK = 512, CARD_SECTORS = 12 * 1024 * 1024, CHUNK = 16 * 1024 };
static const uint64_t boundary = UINT64_C(1) << 32;
static const uint64_t size = (UINT64_C(1) << 32) + CHUNK;
static const uint8_t zeros[CHUNK];

static int card = -1;
static bool inserted = true;
static uLong crc;
FATFS SDFatFS;
FIL SDFile;
char SDPath[4] = "0:";

int BSP_SD_IsDetected(void) { return inserted ? SD_PRESENT : 0; }
DSTATUS disk_status(BYTE drive) { return drive == 0 && inserted ? 0 : STA_NOINIT | STA_NODISK; }
DSTATUS disk_initialize(BYTE drive) { return disk_status(drive); }

DRESULT disk_read(BYTE drive, BYTE *data, DWORD sector, UINT count)
{
  if (disk_status(drive) != 0) return RES_NOTRDY;
  if (!count || sector >= CARD_SECTORS || count > CARD_SECTORS - sector) return RES_PARERR;
  const size_t bytes = (size_t)count * BLOCK;
  return pread(card, data, bytes, (off_t)sector * BLOCK) == (ssize_t)bytes ? RES_OK : RES_ERROR;
}

DRESULT disk_write(BYTE drive, const BYTE *data, DWORD sector, UINT count)
{
  if (disk_status(drive) != 0) return RES_NOTRDY;
  if (!count || sector >= CARD_SECTORS || count > CARD_SECTORS - sector) return RES_PARERR;
  const off_t offset = (off_t)sector * BLOCK;
  const size_t bytes = (size_t)count * BLOCK;
  if (bytes <= sizeof(zeros) && memcmp(data, zeros, bytes) == 0 &&
      fallocate(card, FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE, offset, (off_t)bytes) == 0) {
    return RES_OK;
  }
  return pwrite(card, data, bytes, offset) == (ssize_t)bytes ? RES_OK : RES_ERROR;
}

DRESULT disk_ioctl(BYTE drive, BYTE command, void *buffer)
{
  if (disk_status(drive) != 0) return RES_NOTRDY;
  switch (command) {
    case CTRL_SYNC: return fsync(card) == 0 ? RES_OK : RES_ERROR;
    case GET_SECTOR_COUNT: *(DWORD *)buffer = CARD_SECTORS; return RES_OK;
    case GET_SECTOR_SIZE: *(WORD *)buffer = BLOCK; return RES_OK;
    case GET_BLOCK_SIZE: *(DWORD *)buffer = 1; return RES_OK;
    default: return RES_PARERR;
  }
}

void *ff_memalloc(UINT bytes) { return malloc(bytes); }
void ff_memfree(void *buffer) { free(buffer); }
DWORD get_fattime(void) { return ((2026U - 1980U) << 25U) | (1U << 21U) | (1U << 16U); }
bool StorageCrc32_Start(void) { crc = crc32(0, NULL, 0); return true; }
void StorageCrc32_Append(const uint32_t *words, size_t count)
{
  crc = crc32(crc, (const Bytef *)words, (uInt)(count * sizeof(*words)));
}
uint32_t StorageCrc32_End(void) { return (uint32_t)crc; }

static void Sample(uint64_t offset, uint8_t data[CHUNK])
{
  memset(data, 0, CHUNK);
  if (offset == 0 || offset == boundary - CHUNK || offset == boundary) {
    const uint8_t marker = offset == 0 ? 0x31 : (offset < boundary ? 0x64 : 0xF1);
    for (size_t i = 0; i < CHUNK; ++i) data[i] = (uint8_t)(marker + i % 113U);
  }
}

static void VerifySamples(void)
{
  static uint8_t expected[CHUNK], actual[CHUNK];
  const uint64_t points[] = {0, boundary - CHUNK, boundary};
  for (size_t index = 0; index < sizeof(points)/sizeof(points[0]); ++index) {
    Sample(points[index], expected);
    assert(StorageDisk_Read(points[index], actual, CHUNK / BLOCK) == STORAGE_DISK_OK);
    assert(memcmp(expected, actual, sizeof(actual)) == 0);
  }
  assert(StorageDisk_Read(size, actual, 1) == STORAGE_DISK_RANGE);
}

int main(void)
{
  char path[] = "/tmp/minikmb-large-card-XXXXXX";
  card = mkstemp(path);
  assert(card >= 0 && unlink(path) == 0);
  assert(ftruncate(card, (off_t)CARD_SECTORS * BLOCK) == 0);

  uint8_t format_buffer[32768];
  assert(f_mkfs(SDPath, FM_EXFAT | FM_SFD, sizeof(format_buffer), format_buffer, sizeof(format_buffer)) == FR_OK);

  uint8_t chunk[CHUNK];
  uLong expected_crc = crc32(0, NULL, 0);
  for (uint64_t offset = 0; offset < size; offset += CHUNK) {
    Sample(offset, chunk);
    expected_crc = crc32(expected_crc, chunk, sizeof(chunk));
  }
  StorageDiskImage image = {.size = size, .crc32 = (uint32_t)expected_crc,
                            .name_length = 9, .name = "large.iso"};
  assert(StorageDisk_Begin(&image) == STORAGE_DISK_OK);
  for (uint64_t offset = 0; offset < size; offset += CHUNK) {
    Sample(offset, chunk);
    assert(StorageDisk_Write(chunk, sizeof(chunk)) == STORAGE_DISK_OK);
  }
  assert(StorageDisk_Finish(NULL, NULL) == STORAGE_DISK_OK);
  VerifySamples();

  inserted = false;
  StorageDisk_Invalidate();
  assert(StorageDisk_Load(&image, NULL, NULL) == STORAGE_DISK_NOT_READY);
  inserted = true;
  assert(StorageDisk_Load(&image, NULL, NULL) == STORAGE_DISK_OK);
  assert(image.size == size && image.crc32 == (uint32_t)expected_crc);
  VerifySamples();
  printf("Sparse exFAT: %llu bytes, CRC32 %08x, samples beyond 4 GiB and remount passed\n",
         (unsigned long long)size, image.crc32);
  assert(close(card) == 0);
  return 0;
}

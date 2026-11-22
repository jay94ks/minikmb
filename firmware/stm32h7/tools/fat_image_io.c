#define _FILE_OFFSET_BITS 64
#define _POSIX_C_SOURCE 200809L

#include "ff.h"
#include "diskio.h"

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

enum { SECTOR_BYTES = 512, BUFFER_BYTES = 64 * 1024 };
static FILE *disk;
static DWORD disk_sectors;
static bool writable;
static uint8_t buffer[BUFFER_BYTES];

DSTATUS disk_status(BYTE drive)
{
  return drive != 0 || disk == NULL ? STA_NOINIT : (writable ? 0 : STA_PROTECT);
}

DSTATUS disk_initialize(BYTE drive)
{
  return disk_status(drive);
}

static bool SeekSectors(BYTE drive, DWORD start, UINT count)
{
  return drive == 0 && disk != NULL && count != 0 && start < disk_sectors &&
         count <= disk_sectors - start && fseeko(disk, (off_t)start * SECTOR_BYTES, SEEK_SET) == 0;
}

DRESULT disk_read(BYTE drive, BYTE *data, DWORD start, UINT count)
{
  if (!SeekSectors(drive, start, count)) return RES_PARERR;
  return fread(data, SECTOR_BYTES, count, disk) == count ? RES_OK : RES_ERROR;
}

DRESULT disk_write(BYTE drive, const BYTE *data, DWORD start, UINT count)
{
  if (!writable) return RES_WRPRT;
  if (!SeekSectors(drive, start, count)) return RES_PARERR;
  return fwrite(data, SECTOR_BYTES, count, disk) == count ? RES_OK : RES_ERROR;
}

DRESULT disk_ioctl(BYTE drive, BYTE request, void *data)
{
  if (drive != 0 || disk == NULL) return RES_NOTRDY;
  switch (request) {
    case CTRL_SYNC: return !writable || (fflush(disk) == 0 && fsync(fileno(disk)) == 0) ? RES_OK : RES_ERROR;
    case GET_SECTOR_COUNT: *(DWORD *)data = disk_sectors; return RES_OK;
    case GET_SECTOR_SIZE: *(WORD *)data = SECTOR_BYTES; return RES_OK;
    case GET_BLOCK_SIZE: *(DWORD *)data = 1; return RES_OK;
    default: return RES_PARERR;
  }
}

void *ff_memalloc(UINT count)
{
  return malloc(count);
}

void ff_memfree(void *memory)
{
  free(memory);
}

DWORD get_fattime(void)
{
  return (46U << 25U) | (1U << 21U) | (1U << 16U);
}

static bool RecordName(const char *name, char *out)
{
  const size_t length = strlen(name);
  if (length == 0U || length > 200U || name[length - 1] == '.' || name[length - 1] == ' ') return false;
  for (const unsigned char *byte = (const unsigned char *)name; *byte; ++byte) {
    if (*byte < 0x20 || *byte > 0x7E || strchr("/\\:*?\"<>|", *byte) != NULL) return false;
  }
  memcpy(out, "0:/", 3);
  memcpy(out + 3, name, length + 1);
  return true;
}

static int Put(FILE *source, FIL *target)
{
  size_t length;
  while ((length = fread(buffer, 1, sizeof(buffer), source)) != 0U) {
    UINT written;
    const FRESULT result = f_write(target, buffer, (UINT)length, &written);
    if (result != FR_OK || written != length) {
      fputs("Could not write file to image\n", stderr);
      return 1;
    }
  }
  return ferror(source) ? 1 : 0;
}

static int Get(FIL *source, FILE *target)
{
  for (;;) {
    UINT length;
    if (f_read(source, buffer, sizeof(buffer), &length) != FR_OK ||
        fwrite(buffer, 1, length, target) != length) return 1;
    if (length == 0U) return 0;
  }
}

int main(int argc, char **argv)
{
  char path[204];
  if (argc != 5 || (strcmp(argv[1], "put") != 0 && strcmp(argv[1], "get") != 0) ||
      !RecordName(argv[3], path)) {
    fputs("Usage: fat-image-io {put|get} IMAGE IMAGE_ROOT_NAME LOCAL_FILE\n", stderr);
    return 2;
  }
  writable = strcmp(argv[1], "put") == 0;
  disk = fopen(argv[2], writable ? "r+b" : "rb");
  if (disk == NULL) { perror("open image"); return 1; }
  struct stat image;
  int status = 1;
  if (fstat(fileno(disk), &image) != 0 || !S_ISREG(image.st_mode) || image.st_size <= 0 ||
      image.st_size % SECTOR_BYTES != 0 || (uint64_t)(image.st_size / SECTOR_BYTES) > UINT32_MAX) {
    fputs("Image must be a regular file with a supported 512-byte sector count\n", stderr);
  } else {
    disk_sectors = (DWORD)(image.st_size / SECTOR_BYTES);
    FATFS volume;
    const FRESULT mounted = f_mount(&volume, "0:", 1);
    if (mounted != FR_OK || (volume.fs_type != FS_FAT32 && volume.fs_type != FS_EXFAT)) {
      fputs("Image must contain FAT32 or exFAT\n", stderr);
    } else {
      FIL file;
      const FRESULT opened = f_open(&file, path, writable ? FA_WRITE | FA_CREATE_NEW : FA_READ);
      if (opened == FR_OK) {
        FILE *local = fopen(argv[4], writable ? "rb" : "wbx");
        if (local == NULL) {
          perror("open local file");
        } else {
          status = writable ? Put(local, &file) : Get(&file, local);
          if (fclose(local) != 0) status = 1;
          if (!writable && status != 0) (void)unlink(argv[4]);
        }
        if (f_close(&file) != FR_OK) status = 1;
        if (writable && status != 0) (void)f_unlink(path);
      } else {
        fprintf(stderr, "Could not open image member (%d)\n", opened);
      }
    }
    (void)f_mount(NULL, "0:", 0);
  }
  if (fclose(disk) != 0) status = 1;
  return status;
}

#include "writable_disk.h"

#include "fatfs.h"
#include "storage_crc32.h"

#include <string.h>

enum { DRIVE = 0, BUFFER_BYTES = 4096, MARKER_BYTES = 24 };
static const TCHAR folder[] = "0:/MKB-DISK";
static const TCHAR prefix[] = "0:/MKB-DISK/D";
static const uint8_t magic[] = {'M', 'K', 'B', 'R', 'W', 1, 0, 0};
static const char hex[] = "0123456789ABCDEF";

static uint32_t buffer[BUFFER_BYTES / sizeof(uint32_t)]
    __attribute__((section(".axi_files"), aligned(32)));
static FIL image_file __attribute__((section(".axi_files"), aligned(32)));
static WritableDiskInfo media;
static bool open_file, provision, write_enabled;
static TCHAR image_path[25], marker_path[25];
static StorageDiskResult ReadMarker(uint32_t id, WritableDiskInfo *info);

static StorageDiskResult DiskResult(FRESULT result)
{
  if (result == FR_OK) return STORAGE_DISK_OK;
  if (result == FR_NOT_READY || !StorageDisk_Present()) return STORAGE_DISK_NOT_READY;
  if (result == FR_NO_FILE || result == FR_NO_PATH) return STORAGE_DISK_NOT_FOUND;
  if (result == FR_EXIST) return STORAGE_DISK_BAD_STATE;
  if (result == FR_WRITE_PROTECTED) return STORAGE_DISK_READ_ONLY;
  return STORAGE_DISK_IO_ERROR;
}

static void SetPath(TCHAR *path, uint32_t id, const char *extension)
{
  const size_t start = sizeof(prefix) - 1U;
  memcpy(path, prefix, start);
  for (unsigned i = 0; i < 7; ++i) path[start + i] = hex[(id >> (24U - 4U * i)) & 15U];
  memcpy(path + start + 7U, extension, 5);
}

static uint32_t ReadLE(const uint8_t *p)
{
  return p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static void PutLE(uint8_t *p, uint32_t value)
{
  for (unsigned i = 0; i < 4; ++i) {
    p[i] = (uint8_t)value;
    value >>= 8;
  }
}

static bool Supported(WritableDiskFormat format)
{
  return format == WRITABLE_FAT32 || format == WRITABLE_EXFAT || format == WRITABLE_EXT4;
}

static StorageDiskResult Mount(uint32_t id)
{
  if (open_file) return STORAGE_DISK_BAD_STATE;
  if (id == 0U || id > WRITABLE_DISK_MAX_ID) return STORAGE_DISK_RANGE;
  if (!StorageDisk_Present()) return STORAGE_DISK_NOT_READY;
  SetPath(image_path, id, ".IMG");
  SetPath(marker_path, id, ".OK1");
  const FRESULT result = f_mount(&SDFatFS, SDPath, 1);
  if (result != FR_OK) (void)f_mount(NULL, SDPath, 0);
  return DiskResult(result);
}

StorageDiskResult WritableDisk_Close(void)
{
  const bool present = StorageDisk_Present();
  StorageDiskResult result = STORAGE_DISK_OK;
  if (open_file) result = present ? DiskResult(f_close(&image_file)) : STORAGE_DISK_NOT_READY;
  if (!present) (void)disk_status(DRIVE);
  open_file = provision = write_enabled = false;
  (void)f_mount(NULL, SDPath, 0);
  return result;
}

static StorageDiskResult OpenImage(BYTE mode)
{
  const FRESULT result = f_open(&image_file, image_path, mode);
  open_file = result == FR_OK;
  return DiskResult(result);
}

static StorageDiskResult WriteBytes(const void *data, UINT count)
{
  UINT done = 0;
  const FRESULT result = f_write(&image_file, data, count, &done);
  if (result != FR_OK) return DiskResult(result);
  return done == count ? STORAGE_DISK_OK : STORAGE_DISK_NO_SPACE;
}

static StorageDiskResult ReadBytes(void *data, UINT count)
{
  UINT done = 0;
  const FRESULT result = f_read(&image_file, data, count, &done);
  if (result != FR_OK) return DiskResult(result);
  return done == count ? STORAGE_DISK_OK : STORAGE_DISK_IO_ERROR;
}

static StorageDiskResult ZeroImage(uint64_t size, StorageDiskProgress progress, void *context)
{
  memset(buffer, 0, sizeof(buffer));
  for (uint64_t offset = 0; offset < size;) {
    if (progress != NULL && !progress(context, offset, size)) return STORAGE_DISK_CANCELLED;
    const uint64_t left = size - offset;
    const UINT bytes = left < sizeof(buffer) ? (UINT)left : (UINT)sizeof(buffer);
    const StorageDiskResult result = WriteBytes(buffer, bytes);
    if (result != STORAGE_DISK_OK) return result;
    offset += bytes;
  }
  if (progress != NULL && !progress(context, size, size)) return STORAGE_DISK_CANCELLED;
  return STORAGE_DISK_OK;
}

StorageDiskResult WritableDisk_Create(const WritableDiskInfo *info, StorageDiskProgress progress, void *context)
{
  if (!Supported(info->format)) return STORAGE_DISK_UNSUPPORTED;
  if (info->sectors == 0U) return STORAGE_DISK_RANGE;
  StorageDiskResult status = Mount(info->id);
  if (status != STORAGE_DISK_OK) return status;

  const uint64_t size = (uint64_t)info->sectors * WRITABLE_DISK_SECTOR;
  DWORD card_sectors;
  const DRESULT geometry = disk_ioctl(DRIVE, GET_SECTOR_COUNT, &card_sectors);
  if (geometry != RES_OK) {
    status = STORAGE_DISK_IO_ERROR;
  } else if (info->sectors > card_sectors || (SDFatFS.fs_type != FS_EXFAT && size > UINT32_MAX)) {
    status = STORAGE_DISK_RANGE;
  } else {
    const FRESULT made = f_mkdir(folder);
    status = made == FR_EXIST ? STORAGE_DISK_OK : DiskResult(made);
    if (status == STORAGE_DISK_OK) {
      WritableDiskInfo existing;
      const StorageDiskResult marker = ReadMarker(info->id, &existing);
      if (marker == STORAGE_DISK_CHECKSUM_MISMATCH) {
        FILINFO partial;
        const FRESULT stat = f_stat(marker_path, &partial);
        if (stat != FR_OK) status = DiskResult(stat);
        else if (partial.fsize < MARKER_BYTES) status = DiskResult(f_unlink(marker_path));
        else status = STORAGE_DISK_CHECKSUM_MISMATCH;
      } else if (marker != STORAGE_DISK_NOT_FOUND) {
        status = marker == STORAGE_DISK_OK ? STORAGE_DISK_BAD_STATE : marker;
      }
    }
    if (status == STORAGE_DISK_OK) status = OpenImage(FA_CREATE_ALWAYS | FA_READ | FA_WRITE);
  }
  if (status == STORAGE_DISK_OK) status = ZeroImage(size, progress, context);
  if (status != STORAGE_DISK_OK) {
    (void)WritableDisk_Close();
    return status;
  }
  media = *info;
  provision = write_enabled = true;
  return STORAGE_DISK_OK;
}

static StorageDiskResult Transfer(uint32_t lba, uint32_t count, void *read, const void *write)
{
  if (!open_file) return STORAGE_DISK_BAD_STATE;
  if (write != NULL && !write_enabled) return STORAGE_DISK_READ_ONLY;
  if (lba >= media.sectors || count == 0U || count > media.sectors - lba ||
      count > UINT32_MAX / WRITABLE_DISK_SECTOR) return STORAGE_DISK_RANGE;
  const uint64_t offset = (uint64_t)lba * WRITABLE_DISK_SECTOR;
  const FRESULT result = f_lseek(&image_file, (FSIZE_t)offset);
  if (result != FR_OK) return DiskResult(result);
  if (f_tell(&image_file) != offset) return STORAGE_DISK_RANGE;
  const UINT bytes = (UINT)(count * WRITABLE_DISK_SECTOR);
  return write != NULL ? WriteBytes(write, bytes) : ReadBytes(read, bytes);
}

StorageDiskResult WritableDisk_Read(uint32_t lba, uint32_t count, void *data)
{
  return Transfer(lba, count, data, NULL);
}

StorageDiskResult WritableDisk_Write(uint32_t lba, uint32_t count, const void *data)
{
  return Transfer(lba, count, NULL, data);
}

StorageDiskResult WritableDisk_Sync(void)
{
  if (!open_file) return STORAGE_DISK_BAD_STATE;
  return write_enabled ? DiskResult(f_sync(&image_file)) : STORAGE_DISK_OK;
}

static StorageDiskResult Verify(uint32_t expected, StorageDiskProgress progress, void *context)
{
  if (f_rewind(&image_file) != FR_OK || !StorageCrc32_Start()) return STORAGE_DISK_IO_ERROR;
  const uint64_t size = f_size(&image_file);
  StorageDiskResult result = STORAGE_DISK_OK;
  for (uint64_t offset = 0; offset < size;) {
    if (progress != NULL && !progress(context, offset, size)) {
      result = STORAGE_DISK_CANCELLED;
      break;
    }
    const uint64_t left = size - offset;
    const UINT bytes = left < sizeof(buffer) ? (UINT)left : (UINT)sizeof(buffer);
    result = ReadBytes(buffer, bytes);
    if (result != STORAGE_DISK_OK) break;
    StorageCrc32_Append(buffer, bytes / sizeof(buffer[0]));
    offset += bytes;
  }
  const uint32_t crc = StorageCrc32_End();
  if (result != STORAGE_DISK_OK) return result;
  if (crc != expected) return STORAGE_DISK_CHECKSUM_MISMATCH;
  return progress == NULL || progress(context, size, size) ? STORAGE_DISK_OK : STORAGE_DISK_CANCELLED;
}

static bool MarkerCrc(const uint32_t *words, uint32_t *crc)
{
  if (!StorageCrc32_Start()) return false;
  StorageCrc32_Append(words, (MARKER_BYTES - 4U) / sizeof(uint32_t));
  *crc = StorageCrc32_End();
  return true;
}

static StorageDiskResult SaveMarker(void)
{
  uint32_t words[MARKER_BYTES / sizeof(uint32_t)] = {0};
  uint8_t *record = (uint8_t *)words;
  memcpy(record, magic, sizeof(magic));
  PutLE(record + 8, media.id);
  PutLE(record + 12, media.sectors);
  PutLE(record + 16, media.format);
  uint32_t crc;
  if (!MarkerCrc(words, &crc)) return STORAGE_DISK_IO_ERROR;
  PutLE(record + 20, crc);
  FIL file;
  FRESULT result = f_open(&file, marker_path, FA_CREATE_NEW | FA_WRITE);
  if (result != FR_OK) return DiskResult(result);
  UINT written = 0;
  result = f_write(&file, record, MARKER_BYTES, &written);
  const FRESULT close = f_close(&file);
  if (result == FR_OK && close == FR_OK && written == MARKER_BYTES) return STORAGE_DISK_OK;
  (void)f_unlink(marker_path);
  if (result != FR_OK || close != FR_OK) return DiskResult(result != FR_OK ? result : close);
  return STORAGE_DISK_NO_SPACE;
}

StorageDiskResult WritableDisk_Commit(uint32_t crc32, StorageDiskProgress progress, void *context)
{
  if (!open_file || !provision) return STORAGE_DISK_BAD_STATE;
  StorageDiskResult status = DiskResult(f_close(&image_file));
  open_file = false;
  if (status == STORAGE_DISK_OK) status = OpenImage(FA_READ);
  if (status == STORAGE_DISK_OK) status = Verify(crc32, progress, context);
  if (open_file) {
    const FRESULT close = f_close(&image_file);
    if (status == STORAGE_DISK_OK) status = DiskResult(close);
  }
  open_file = false;
  if (status == STORAGE_DISK_OK) status = SaveMarker();
  (void)WritableDisk_Close();
  return status;
}

static StorageDiskResult ReadMarkerAt(const TCHAR *marker, uint32_t id, WritableDiskInfo *info)
{
  uint32_t words[MARKER_BYTES / sizeof(uint32_t)];
  const uint8_t *record = (const uint8_t *)words;
  FIL file;
  FRESULT result = f_open(&file, marker, FA_READ);
  if (result != FR_OK) return DiskResult(result);
  const FSIZE_t size = f_size(&file);
  UINT read = 0;
  if (size == MARKER_BYTES) result = f_read(&file, words, MARKER_BYTES, &read);
  const FRESULT close = f_close(&file);
  if (result != FR_OK || close != FR_OK) return DiskResult(result != FR_OK ? result : close);
  if (read != MARKER_BYTES || memcmp(record, magic, sizeof(magic)) != 0 || ReadLE(record + 8) != id) {
    return STORAGE_DISK_CHECKSUM_MISMATCH;
  }
  uint32_t crc;
  if (!MarkerCrc(words, &crc)) return STORAGE_DISK_IO_ERROR;
  if (ReadLE(record + 20) != crc) return STORAGE_DISK_CHECKSUM_MISMATCH;
  const WritableDiskInfo decoded = {.id = id, .sectors = ReadLE(record + 12), .format = ReadLE(record + 16)};
  if (!Supported(decoded.format) || decoded.sectors == 0U) return STORAGE_DISK_CHECKSUM_MISMATCH;
  *info = decoded;
  return STORAGE_DISK_OK;
}

StorageDiskResult WritableDisk_Open(uint32_t id, bool writable, WritableDiskInfo *info)
{
  StorageDiskResult status = Mount(id);
  if (status != STORAGE_DISK_OK) return status;
  status = ReadMarker(id, &media);
  if (status == STORAGE_DISK_OK) status = OpenImage(writable ? (FA_READ | FA_WRITE) : FA_READ);
  if (status == STORAGE_DISK_OK && f_size(&image_file) != (uint64_t)media.sectors * WRITABLE_DISK_SECTOR) {
    status = STORAGE_DISK_CHECKSUM_MISMATCH;
  }
  if (status != STORAGE_DISK_OK) {
    (void)WritableDisk_Close();
    return status;
  }
  write_enabled = writable;
  *info = media;
  return STORAGE_DISK_OK;
}

static StorageDiskResult ReadMarker(uint32_t id, WritableDiskInfo *info)
{
  return ReadMarkerAt(marker_path, id, info);
}

StorageDiskResult WritableDisk_List(uint32_t cursor, WritableDiskInfo items[4], uint8_t *count, uint32_t *next)
{
  *count = 0; *next = 0;
  const bool mounted = open_file;
  if (!mounted) { const StorageDiskResult status = Mount(1U); if (status != STORAGE_DISK_OK) return status; }
  DIR dir;
  FRESULT status = f_opendir(&dir, folder);
  if (status == FR_NO_PATH || status == FR_NO_FILE) { if (!mounted) (void)f_mount(NULL, SDPath, 0); return STORAGE_DISK_OK; }
  if (status != FR_OK) { if (!mounted) (void)f_mount(NULL, SDPath, 0); return DiskResult(status); }
  uint32_t position = 0;
  StorageDiskResult result = STORAGE_DISK_OK;
  while (*count < 4U) {
    FILINFO info = {0};
    status = f_readdir(&dir, &info);
    if (status != FR_OK || info.fname[0] == 0) break;
    if (position++ < cursor || (info.fattrib & AM_DIR) != 0U) continue;
    if (strlen(info.fname) != 12U || info.fname[0] != 'D' || strcmp(info.fname + 8, ".OK1") != 0) continue;
    uint32_t id = 0; bool valid = true;
    for (unsigned i = 1; i < 8; ++i) {
      const char *digit = strchr(hex, info.fname[i]);
      if (digit == NULL) { valid = false; break; }
      id = (id << 4) | (uint32_t)(digit - hex);
    }
    if (!valid || id == 0U) continue;
    TCHAR marker[25], image[25]; SetPath(marker, id, ".OK1"); SetPath(image, id, ".IMG");
    WritableDiskInfo item;
    result = ReadMarkerAt(marker, id, &item);
    if (result == STORAGE_DISK_CHECKSUM_MISMATCH || result == STORAGE_DISK_NOT_FOUND) { result = STORAGE_DISK_OK; continue; }
    if (result != STORAGE_DISK_OK) break;
    FILINFO file = {0}; status = f_stat(image, &file);
    if (status == FR_NO_FILE || status == FR_NO_PATH) continue;
    if (status != FR_OK) { result = DiskResult(status); break; }
    if ((file.fattrib & AM_DIR) != 0U || file.fsize != (uint64_t)item.sectors * WRITABLE_DISK_SECTOR) continue;
    items[(*count)++] = item;
  }
  if (result == STORAGE_DISK_OK && status != FR_OK) result = DiskResult(status);
  if (*count == 4U) *next = position;
  const FRESULT closed = f_closedir(&dir);
  if (result == STORAGE_DISK_OK && closed != FR_OK) result = DiskResult(closed);
  if (!mounted) (void)f_mount(NULL, SDPath, 0);
  return result;
}

StorageDiskResult WritableDisk_Delete(uint32_t id)
{
  StorageDiskResult result = Mount(id);
  if (result != STORAGE_DISK_OK) return result;
  /* Remove validity first: interrupted formatting must never expose a valid old marker. */
  FRESULT status = f_unlink(marker_path);
  if (status == FR_OK || status == FR_NO_FILE) status = f_unlink(image_path);
  result = DiskResult(status);
  (void)f_mount(NULL, SDPath, 0);
  return result;
}

#include "storage_disk.h"

#include "fatfs.h"
#include "optical_extents.h"
#include "storage_crc32.h"

#include <string.h>

enum {
  DRIVE = 0,
  MEDIA_OPTICAL = 1,
  FILE_BUFFER_BYTES = 16 * 1024,
  LINK_WORDS = 2 * OPTICAL_MAX_EXTENTS + 2,
};

static const TCHAR directory_path[] = "0:/MINIKBM";
static const TCHAR image_path[] = "0:/MINIKBM/IMAGE.BIN";
static const TCHAR marker_path[] = "0:/MINIKBM/IMAGE.OK";
static const uint8_t marker_magic[] = {'M', 'K', 'B', 'O', 'P', 'T', 1, 0};

static OpticalExtent extent[OPTICAL_MAX_EXTENTS]
    __attribute__((section(".axi_extents"), aligned(32)));
static DWORD link_map[LINK_WORDS]
    __attribute__((section(".axi_files"), aligned(32)));
static uint32_t file_buffer[FILE_BUFFER_BYTES / sizeof(uint32_t)]
    __attribute__((section(".axi_files"), aligned(32)));
static StorageDiskImage current_image;
static size_t extent_count;
static bool file_open, writing;

_Static_assert(_VOLUMES == 1 && _MAX_SS == OPTICAL_SD_SECTOR_BYTES, "SD block geometry");
_Static_assert(_FS_EXFAT && _USE_FASTSEEK, "64-bit image length and FatFs CLMT required");
_Static_assert(sizeof(DWORD) == sizeof(uint32_t), "FatFs DWORD is a 32-bit sector address");

static uint64_t ReadLE(const uint8_t *bytes, size_t n)
{
  uint64_t value = 0;
  for (size_t i = 0; i < n; ++i) value |= (uint64_t)bytes[i] << (8U * i);
  return value;
}

static void WriteLE(uint8_t *bytes, uint64_t value, size_t n)
{
  for (size_t i = 0; i < n; ++i) {
    bytes[i] = (uint8_t)value;
    value >>= 8;
  }
}

static bool ValidDisplayName(const uint8_t *bytes, size_t length)
{
  size_t i = 0;
  while (i < length) {
    uint32_t code = bytes[i++];
    if (code >= 0x80U) {
      unsigned tails;
      uint32_t minimum;
      if (code >= 0xC2U && code <= 0xDFU) {
        tails = 1;
        minimum = 0x80U;
        code &= 0x1FU;
      } else if (code >= 0xE0U && code <= 0xEFU) {
        tails = 2;
        minimum = 0x800U;
        code &= 0x0FU;
      } else if (code >= 0xF0U && code <= 0xF4U) {
        tails = 3;
        minimum = 0x10000U;
        code &= 0x07U;
      } else {
        return false;
      }
      if (tails > length - i) return false;
      while (tails-- != 0U) {
        const uint8_t byte = bytes[i++];
        if ((byte & 0xC0U) != 0x80U) return false;
        code = (code << 6) | (byte & 0x3FU);
      }
      if (code < minimum || code > 0x10FFFFU ||
          (code >= 0xD800U && code <= 0xDFFFU)) return false;
    }
    if (code < 32U || (code >= 127U && code <= 159U) || code == '/' || code == '\\') {
      return false;
    }
  }
  return true;
}

StorageDiskResult StorageDisk_DecodeImage(const uint8_t *payload, size_t length, StorageDiskImage *image)
{
  if (length < STORAGE_IMAGE_HEADER_BYTES) return STORAGE_DISK_BAD_PAYLOAD;
  if (payload[40] != MEDIA_OPTICAL) return STORAGE_DISK_UNSUPPORTED;
  for (unsigned i = 12; i < 40; ++i) {
    if (payload[i] != 0U) return STORAGE_DISK_BAD_PAYLOAD;
  }
  const uint64_t size = ReadLE(payload, 8);
  const uint16_t name_length = (uint16_t)ReadLE(payload + 42, 2);
  if (payload[41] != 0U || name_length == 0U || name_length > STORAGE_IMAGE_NAME_BYTES ||
      length - STORAGE_IMAGE_HEADER_BYTES != name_length ||
      !ValidDisplayName(payload + STORAGE_IMAGE_HEADER_BYTES, name_length)) {
    return STORAGE_DISK_BAD_PAYLOAD;
  }
  if (size == 0U || size % OPTICAL_BLOCK_BYTES != 0U ||
      size > ((uint64_t)UINT32_MAX + 1U) * OPTICAL_SD_SECTOR_BYTES) return STORAGE_DISK_RANGE;
  *image = (StorageDiskImage){
    .size = size, .crc32 = (uint32_t)ReadLE(payload + 8, 4), .name_length = name_length,
  };
  memcpy(image->name, payload + STORAGE_IMAGE_HEADER_BYTES, name_length);
  return STORAGE_DISK_OK;
}

static size_t EncodeImage(uint8_t *payload, const StorageDiskImage *image)
{
  memset(payload, 0, STORAGE_IMAGE_HEADER_BYTES);
  WriteLE(payload, image->size, 8);
  WriteLE(payload + 8, image->crc32, 4);
  payload[40] = MEDIA_OPTICAL;
  WriteLE(payload + 42, image->name_length, 2);
  memcpy(payload + STORAGE_IMAGE_HEADER_BYTES, image->name, image->name_length);
  return STORAGE_IMAGE_HEADER_BYTES + image->name_length;
}

bool StorageDisk_Present(void)
{
  return BSP_SD_IsDetected() == SD_PRESENT;
}

static StorageDiskResult DiskError(FRESULT result)
{
  if (result == FR_OK) return STORAGE_DISK_OK;
  if (!StorageDisk_Present() || result == FR_NOT_READY) return STORAGE_DISK_NOT_READY;
  if (result == FR_NO_FILE || result == FR_NO_PATH) return STORAGE_DISK_NOT_FOUND;
  return STORAGE_DISK_IO_ERROR;
}

static FRESULT CloseFile(void)
{
  if (!file_open) return FR_OK;
  file_open = false;
  return f_close(&SDFile);
}

static FRESULT OpenFile(const TCHAR *path, BYTE mode)
{
  const FRESULT result = f_open(&SDFile, path, mode);
  file_open = result == FR_OK;
  return result;
}

void StorageDisk_Invalidate(void)
{
  extent_count = 0U;
  writing = false;
  if (StorageDisk_Present()) {
    (void)CloseFile();
  } else {
    (void)disk_status(DRIVE);
    file_open = false;
  }
  (void)f_mount(NULL, SDPath, 0);
}

StorageDiskResult StorageDisk_Format(bool exfat)
{
  if (!StorageDisk_Present()) return STORAGE_DISK_NOT_READY;
  if (file_open) return STORAGE_DISK_BAD_STATE;
  StorageDisk_Invalidate();
  FRESULT result = f_mkfs(SDPath, exfat ? FM_EXFAT : FM_FAT32, 0U, file_buffer, sizeof(file_buffer));
  if (result == FR_OK) result = f_mount(&SDFatFS, SDPath, 1);
  const bool correct_type = SDFatFS.fs_type == (exfat ? FS_EXFAT : FS_FAT32);
  (void)f_mount(NULL, SDPath, 0);
  if (result == FR_OK && !correct_type) return STORAGE_DISK_IO_ERROR;
  if (result == FR_WRITE_PROTECTED) return STORAGE_DISK_READ_ONLY;
  return DiskError(result);
}

StorageDiskResult StorageDisk_Begin(const StorageDiskImage *image)
{
  if (!StorageDisk_Present()) return STORAGE_DISK_NOT_READY;
  if (file_open || image->name_length == 0U || image->name_length > STORAGE_IMAGE_NAME_BYTES ||
      image->size == 0U || image->size % OPTICAL_BLOCK_BYTES != 0U) return STORAGE_DISK_BAD_STATE;

  FRESULT result = f_mount(&SDFatFS, SDPath, 1);
  if (result != FR_OK) {
    StorageDisk_Invalidate();
    return DiskError(result);
  }
  DWORD card_sectors = 0;
  if (disk_ioctl(DRIVE, GET_SECTOR_COUNT, &card_sectors) != RES_OK) {
    StorageDisk_Invalidate();
    return STORAGE_DISK_IO_ERROR;
  }
  if ((SDFatFS.fs_type != FS_EXFAT && image->size > UINT32_MAX) ||
      image->size > (uint64_t)card_sectors * OPTICAL_SD_SECTOR_BYTES) {
    StorageDisk_Invalidate();
    return STORAGE_DISK_RANGE;
  }
  extent_count = 0U;
  result = f_mkdir(directory_path);
  if (result == FR_EXIST) result = FR_OK;
  if (result == FR_OK) {
    /* Invalidate the old commit marker before opening the image for replacement. */
    result = f_unlink(marker_path);
    if (result == FR_NO_FILE) result = FR_OK;
  }
  if (result == FR_OK) result = OpenFile(image_path, FA_WRITE | FA_CREATE_ALWAYS);
  if (result != FR_OK) {
    StorageDisk_Invalidate();
    return DiskError(result);
  }

  current_image = *image;
  writing = true;
  return STORAGE_DISK_OK;
}

StorageDiskResult StorageDisk_Write(const void *data, size_t length)
{
  if (!writing || !file_open) return STORAGE_DISK_BAD_STATE;
  const uint64_t offset = f_tell(&SDFile);
  if (length == 0U || length > FILE_BUFFER_BYTES || offset > current_image.size ||
      length > current_image.size - offset) {
    return STORAGE_DISK_RANGE;
  }
  UINT transferred;
  const FRESULT result = f_write(&SDFile, data, (UINT)length, &transferred);
  if (result != FR_OK) return DiskError(result);
  return transferred == length ? STORAGE_DISK_OK : STORAGE_DISK_NO_SPACE;
}

static StorageDiskResult CheckImage(StorageDiskProgress progress, void *context)
{
  FRESULT result = OpenFile(image_path, FA_READ);
  if (result != FR_OK) return DiskError(result);
  if (f_size(&SDFile) != current_image.size) return STORAGE_DISK_CHECKSUM_MISMATCH;

  if (!StorageCrc32_Start()) return STORAGE_DISK_IO_ERROR;

  StorageDiskResult check = STORAGE_DISK_OK;
  for (uint64_t offset = 0; offset < current_image.size;) {
    if (progress != NULL && !progress(context, offset, current_image.size)) {
      check = STORAGE_DISK_CANCELLED;
      break;
    }
    const uint64_t remaining = current_image.size - offset;
    const UINT length = remaining < FILE_BUFFER_BYTES ? (UINT)remaining : FILE_BUFFER_BYTES;
    UINT transferred;
    result = f_read(&SDFile, file_buffer, length, &transferred);
    if (result != FR_OK || transferred != length) {
      check = result == FR_OK ? STORAGE_DISK_IO_ERROR : DiskError(result);
      break;
    }
    StorageCrc32_Append(file_buffer, length / sizeof(file_buffer[0]));
    offset += length;
  }
  const uint32_t checksum = StorageCrc32_End();
  if (check != STORAGE_DISK_OK) return check;
  if (checksum != current_image.crc32) return STORAGE_DISK_CHECKSUM_MISMATCH;
  if (progress != NULL && !progress(context, current_image.size, current_image.size)) {
    return STORAGE_DISK_CANCELLED;
  }

  link_map[0] = LINK_WORDS;
  SDFile.cltbl = link_map;
  result = f_lseek(&SDFile, CREATE_LINKMAP);
  if (result == FR_NOT_ENOUGH_CORE) return STORAGE_DISK_FRAGMENT_LIMIT;
  if (result != FR_OK) return DiskError(result);

  DWORD card_sectors = 0;
  if (SDFatFS.n_fatent <= 2U || disk_ioctl(DRIVE, GET_SECTOR_COUNT, &card_sectors) != RES_OK) {
    return STORAGE_DISK_IO_ERROR;
  }
  const OpticalVolume volume = {
    .card_sectors = card_sectors,
    .first_data_sector = SDFatFS.database,
    .data_clusters = SDFatFS.n_fatent - 2U,
    .sectors_per_cluster = SDFatFS.csize,
  };
  const OpticalMapResult map = OpticalExtent_Build(&volume, current_image.size, link_map,
                                                   LINK_WORDS, extent, OPTICAL_MAX_EXTENTS,
                                                   &extent_count);
  if (map == OPTICAL_MAP_TOO_FRAGMENTED) return STORAGE_DISK_FRAGMENT_LIMIT;
  if (map != OPTICAL_MAP_OK) return STORAGE_DISK_IO_ERROR;
  return DiskError(CloseFile());
}

static StorageDiskResult SaveMarker(void)
{
  uint8_t marker[sizeof(marker_magic) + STORAGE_IMAGE_HEADER_BYTES + STORAGE_IMAGE_NAME_BYTES];
  memcpy(marker, marker_magic, sizeof(marker_magic));
  const size_t length = sizeof(marker_magic) + EncodeImage(marker + sizeof(marker_magic), &current_image);
  FRESULT result = OpenFile(marker_path, FA_CREATE_NEW | FA_WRITE);
  if (result != FR_OK) return DiskError(result);
  UINT transferred;
  result = f_write(&SDFile, marker, (UINT)length, &transferred);
  if (result != FR_OK) return DiskError(result);
  if (transferred != length) return STORAGE_DISK_NO_SPACE;
  const FRESULT sync_result = f_sync(&SDFile);
  return DiskError(sync_result == FR_OK ? CloseFile() : sync_result);
}

StorageDiskResult StorageDisk_Finish(StorageDiskProgress progress, void *context)
{
  if (!writing || !file_open || f_tell(&SDFile) != current_image.size) return STORAGE_DISK_BAD_STATE;
  writing = false;
  FRESULT result = f_sync(&SDFile);
  if (result == FR_OK) result = CloseFile();
  StorageDiskResult status = DiskError(result);
  if (status == STORAGE_DISK_OK) status = CheckImage(progress, context);
  if (status == STORAGE_DISK_OK && progress != NULL &&
      !progress(context, current_image.size, current_image.size)) status = STORAGE_DISK_CANCELLED;
  if (status == STORAGE_DISK_OK) status = SaveMarker();
  (void)CloseFile();
  (void)f_mount(NULL, SDPath, 0);
  if (status != STORAGE_DISK_OK) extent_count = 0U;
  return status;
}

StorageDiskResult StorageDisk_Load(StorageDiskImage *image, StorageDiskProgress progress, void *context)
{
  if (!StorageDisk_Present()) return STORAGE_DISK_NOT_READY;
  if (file_open) return STORAGE_DISK_BAD_STATE;

  extent_count = 0U;
  FRESULT result = f_mount(&SDFatFS, SDPath, 1);
  if (result == FR_OK) result = OpenFile(marker_path, FA_READ);
  StorageDiskResult status = DiskError(result);
  if (status == STORAGE_DISK_OK) {
    uint8_t marker[sizeof(marker_magic) + STORAGE_IMAGE_HEADER_BYTES + STORAGE_IMAGE_NAME_BYTES];
    UINT length = 0;
    const uint64_t size = f_size(&SDFile);
    if (size <= sizeof(marker)) {
      result = f_read(&SDFile, marker, (UINT)size, &length);
    }
    if (result != FR_OK) {
      status = DiskError(result);
    } else if (length < sizeof(marker_magic) + STORAGE_IMAGE_HEADER_BYTES ||
               length != size || memcmp(marker, marker_magic, sizeof(marker_magic)) != 0) {
      status = STORAGE_DISK_NOT_FOUND;
    } else {
      status = StorageDisk_DecodeImage(marker + sizeof(marker_magic), length - sizeof(marker_magic),
                                       &current_image);
    }
    const FRESULT close = CloseFile();
    if (status == STORAGE_DISK_OK) status = DiskError(close);
  }
  if (status == STORAGE_DISK_OK) status = CheckImage(progress, context);
  (void)CloseFile();
  (void)f_mount(NULL, SDPath, 0);
  if (status == STORAGE_DISK_OK) {
    *image = current_image;
  } else {
    extent_count = 0U;
  }
  return status;
}

StorageDiskResult StorageDisk_Read(uint64_t offset, void *buffer, uint32_t sectors)
{
  if (extent_count == 0U) return STORAGE_DISK_BAD_STATE;
  if (!StorageDisk_Present()) {
    StorageDisk_Invalidate();
    return STORAGE_DISK_NOT_READY;
  }
  if (sectors == 0U || offset > current_image.size ||
      (uint64_t)sectors * OPTICAL_SD_SECTOR_BYTES > current_image.size - offset ||
      sectors > UINT32_MAX / OPTICAL_SD_SECTOR_BYTES) return STORAGE_DISK_RANGE;
  uint8_t *bytes = buffer;
  while (sectors != 0U) {
    uint32_t sector, contiguous;
    if (!OpticalExtent_Resolve(extent, extent_count, offset, &sector, &contiguous)) {
      return STORAGE_DISK_RANGE;
    }
    const UINT count = sectors < contiguous ? sectors : contiguous;
    if (disk_read(DRIVE, bytes, sector, count) != RES_OK) {
      StorageDisk_Invalidate();
      return STORAGE_DISK_IO_ERROR;
    }
    const size_t transferred = count * OPTICAL_SD_SECTOR_BYTES;
    offset += transferred;
    bytes += transferred;
    sectors -= count;
  }
  return STORAGE_DISK_OK;
}

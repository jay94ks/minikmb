#ifndef MINIKMB_OPTICAL_EXTENTS_H
#define MINIKMB_OPTICAL_EXTENTS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum { OPTICAL_BLOCK_BYTES = 2048, OPTICAL_SD_SECTOR_BYTES = 512, OPTICAL_MAX_EXTENTS = 4096 };

typedef struct {
  uint64_t file_offset;
  uint32_t sd_sector;
  uint32_t sector_count;
} OpticalExtent;

typedef struct {
  uint64_t card_sectors;
  uint32_t first_data_sector;
  uint32_t data_clusters;
  uint32_t sectors_per_cluster;
} OpticalVolume;

typedef enum {
  OPTICAL_MAP_OK,
  OPTICAL_MAP_INVALID,
  OPTICAL_MAP_TOO_FRAGMENTED,
} OpticalMapResult;

/* CLMT is the completed FatFs CREATE_LINKMAP output for this closed-for-write image. */
OpticalMapResult OpticalExtent_Build(const OpticalVolume *volume, uint64_t image_bytes,
                                    const uint32_t *clmt, size_t words, OpticalExtent *extent,
                                    size_t capacity, size_t *count);

/* Return the physical run covering a 512-byte-aligned byte offset of the image. */
bool OpticalExtent_Resolve(const OpticalExtent *extent, size_t count, uint64_t file_offset,
                           uint32_t *sd_sector, uint32_t *contiguous_sectors);

#endif


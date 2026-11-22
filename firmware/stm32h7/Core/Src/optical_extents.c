#include "optical_extents.h"

_Static_assert(sizeof(OpticalExtent) == 16, "extent map memory budget");

OpticalMapResult OpticalExtent_Build(const OpticalVolume *volume, uint64_t image_bytes,
                                    const uint32_t *clmt, size_t words, OpticalExtent *extent,
                                    size_t capacity, size_t *count)
{
  *count = 0;
  if (image_bytes == 0 || image_bytes % OPTICAL_BLOCK_BYTES != 0 || words < 2 ||
      volume->sectors_per_cluster == 0 || volume->data_clusters == 0 ||
      volume->card_sectors > (uint64_t)UINT32_MAX + 1U) {
    return OPTICAL_MAP_INVALID;
  }

  const size_t used = clmt[0];
  if (used < 4 || used % 2U != 0) return OPTICAL_MAP_INVALID;
  const size_t runs = (used - 2U) / 2U;
  if (runs > capacity || runs > OPTICAL_MAX_EXTENTS) return OPTICAL_MAP_TOO_FRAGMENTED;
  if (used > words || clmt[used - 1U] != 0) return OPTICAL_MAP_INVALID;

  const uint64_t image_sectors = image_bytes / OPTICAL_SD_SECTOR_BYTES;
  const uint64_t cluster_sectors = volume->sectors_per_cluster;
  uint64_t remaining = image_sectors;
  for (size_t i = 0; i < runs; ++i) {
    const uint32_t clusters = clmt[2U * i + 1U];
    const uint32_t first_cluster = clmt[2U * i + 2U];
    if (remaining == 0 || clusters == 0 || first_cluster < 2 ||
        first_cluster - 2U >= volume->data_clusters ||
        clusters > volume->data_clusters - (first_cluster - 2U)) {
      return OPTICAL_MAP_INVALID;
    }
    const uint64_t start = (uint64_t)volume->first_data_sector +
                           (uint64_t)(first_cluster - 2U) * cluster_sectors;
    const uint64_t allocated = clusters * cluster_sectors;
    if (start >= volume->card_sectors || allocated > volume->card_sectors - start) {
      return OPTICAL_MAP_INVALID;
    }
    if (allocated > remaining && (i + 1U != runs || allocated - remaining >= cluster_sectors)) {
      return OPTICAL_MAP_INVALID;
    }
    const uint64_t size = allocated < remaining ? allocated : remaining;
    if (size > UINT32_MAX) return OPTICAL_MAP_INVALID;
    extent[i] = (OpticalExtent){
      .file_offset = (image_sectors - remaining) * OPTICAL_SD_SECTOR_BYTES,
      .sd_sector = (uint32_t)start,
      .sector_count = (uint32_t)size,
    };
    remaining -= size;
  }
  if (remaining != 0) return OPTICAL_MAP_INVALID;
  *count = runs;
  return OPTICAL_MAP_OK;
}

bool OpticalExtent_Resolve(const OpticalExtent *extent, size_t count, uint64_t file_offset,
                           uint32_t *sd_sector, uint32_t *contiguous_sectors)
{
  if (file_offset % OPTICAL_SD_SECTOR_BYTES != 0) return false;
  size_t first = 0, stop = count;
  while (first < stop) {
    const size_t middle = first + (stop - first) / 2U;
    if (file_offset < extent[middle].file_offset) {
      stop = middle;
      continue;
    }
    const uint64_t delta = (file_offset - extent[middle].file_offset) / OPTICAL_SD_SECTOR_BYTES;
    if (delta < extent[middle].sector_count) {
      *sd_sector = extent[middle].sd_sector + (uint32_t)delta;
      *contiguous_sectors = extent[middle].sector_count - (uint32_t)delta;
      return true;
    }
    first = middle + 1U;
  }
  return false;
}


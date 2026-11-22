#include "optical_extents.h"
#include <assert.h>

int main(void)
{
  const OpticalVolume volume = {
    .card_sectors = 1000, .first_data_sector = 64,
    .data_clusters = 100, .sectors_per_cluster = 2,
  };
  const uint32_t clmt[] = {6, 1, 2, 3, 30, 0};
  OpticalExtent extent[4];
  size_t count;
  assert(OpticalExtent_Build(&volume, 4096, clmt, 6, extent, 4, &count) == OPTICAL_MAP_OK);
  assert(count == 2);
  uint32_t sector, available;
  assert(OpticalExtent_Resolve(extent, count, 512, &sector, &available));
  assert(sector == 65 && available == 1);
  assert(OpticalExtent_Resolve(extent, count, 1024, &sector, &available));
  assert(sector == 120 && available == 6);
  assert(!OpticalExtent_Resolve(extent, count, 4096, &sector, &available));
  assert(!OpticalExtent_Resolve(extent, count, 1, &sector, &available));
  assert(OpticalExtent_Build(&volume, 4096, clmt, 6, extent, 1, &count) == OPTICAL_MAP_TOO_FRAGMENTED);
  assert(count == 0);
  assert(OpticalExtent_Build(&volume, 4096, clmt, 5, extent, 4, &count) == OPTICAL_MAP_INVALID);
  assert(OpticalExtent_Build(&volume, 2048, clmt, 6, extent, 4, &count) == OPTICAL_MAP_INVALID);
  assert(OpticalExtent_Build(&volume, 8192, clmt, 6, extent, 4, &count) == OPTICAL_MAP_INVALID);
  const OpticalVolume short_card = {
    .card_sectors = 123, .first_data_sector = 64,
    .data_clusters = 100, .sectors_per_cluster = 2,
  };
  assert(OpticalExtent_Build(&short_card, 4096, clmt, 6, extent, 4, &count) == OPTICAL_MAP_INVALID);
  const OpticalVolume valid_large = {
    .card_sectors = 10000000, .first_data_sector = 64,
    .data_clusters = 312000, .sectors_per_cluster = 32,
  };
  const uint64_t aligned_bytes = UINT64_C(4800000000);
  const uint32_t valid_large_clmt[] = {4, 292969, 2, 0};
  assert(OpticalExtent_Build(&valid_large, aligned_bytes, valid_large_clmt, 4, extent, 4, &count) == OPTICAL_MAP_OK);
  assert(OpticalExtent_Resolve(extent, count, aligned_bytes - 512U, &sector, &available));
  assert(sector == 9375063U && available == 1);
  return 0;
}


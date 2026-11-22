#include "main.h"
#include "storage_crc32.h"

static void ResetCrc32(void)
{
  CRC->POL = 0x04C11DB7U;
  CRC->INIT = UINT32_MAX;
  /* Reverse each little-endian word so the first file byte shifts in first. */
  CRC->CR = CRC_CR_REV_IN | CRC_CR_REV_OUT | CRC_CR_RESET;
}

bool StorageCrc32_Start(void)
{
  __HAL_RCC_CRC_CLK_ENABLE();
  ResetCrc32();
  CRC->DR = 0x34333231U;
  CRC->DR = 0x38373635U;
  if ((CRC->DR ^ UINT32_MAX) != 0x9AE0DAAFU) {
    __HAL_RCC_CRC_CLK_DISABLE();
    return false;
  }
  ResetCrc32();
  return true;
}

void StorageCrc32_Append(const uint32_t *words, size_t count)
{
  while (count-- != 0U) CRC->DR = *words++;
}

uint32_t StorageCrc32_End(void)
{
  const uint32_t next = CRC->DR ^ UINT32_MAX;
  __HAL_RCC_CRC_CLK_DISABLE();
  return next;
}

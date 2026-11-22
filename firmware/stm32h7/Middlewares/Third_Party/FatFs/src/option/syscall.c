/*------------------------------------------------------------------------*/
/* Sample code of OS dependent controls for FatFs                         */
/* (C)ChaN, 2014                                                          */
/*   Portions COPYRIGHT 2017 STMicroelectronics                           */
/*   Portions Copyright (C) 2014, ChaN, all right reserved                */
/*------------------------------------------------------------------------*/

/**
  ******************************************************************************
  * @attention
  *
  * Copyright (c) 2017 STMicroelectronics. All rights reserved.
  *
  * This software component is licensed by ST under BSD 3-Clause license,
  * the "License"; You may not use this file except in compliance with the
  * License. You may obtain a copy of the License at:
  *                       opensource.org/licenses/BSD-3-Clause
  *
  ******************************************************************************
**/

#include "../ff.h"

#if _FS_REENTRANT
static StaticSemaphore_t volume_mutexes[_VOLUMES];

int ff_cre_syncobj(BYTE volume, _SYNC_t *object)
{
  if (volume >= _VOLUMES || object == NULL) return 0;
  *object = xSemaphoreCreateMutexStatic(&volume_mutexes[volume]);
  return *object != NULL;
}

int ff_del_syncobj(_SYNC_t object)
{
  vSemaphoreDelete(object);
  return 1;
}

int ff_req_grant(_SYNC_t object)
{
  return xSemaphoreTake(object, _FS_TIMEOUT) == pdTRUE;
}

void ff_rel_grant(_SYNC_t object)
{
  (void)xSemaphoreGive(object);
}
#endif

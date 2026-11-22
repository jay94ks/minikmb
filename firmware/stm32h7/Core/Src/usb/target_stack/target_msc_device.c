#include "target_tusb_namespace.h"
#include "class/msc/msc_device.c"
#if CFG_TUD_MSC
#include "target_msc.h"

void *TargetMsc_ReadBuffer(void)
{
  return tud_msc_io_ticket(0) != 0U &&
         _mscd_itf.cbw.command[0] == SCSI_CMD_READ_10 ? _mscd_epbuf.buf : NULL;
}

#endif

#ifndef MINIKMB_TARGET_NETWORK_H
#define MINIKMB_TARGET_NETWORK_H

#include "management_protocol.h"

void TargetNetwork_Poll(bool mounted, bool reset);
const ManagementNetworkBackend *TargetNetwork_Backend(void);

#endif

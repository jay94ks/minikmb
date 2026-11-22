#ifndef MINIKMB_TARGET_FEATURES_H
#define MINIKMB_TARGET_FEATURES_H

#ifndef MINIKMB_MANAGEMENT_FAST_CDC
/* Keep the verified management RX buffering in normal Debug and Release builds. */
#define MINIKMB_MANAGEMENT_FAST_CDC 1
#endif

#ifndef MINIKMB_RELEASE_PROFILE
#define MINIKMB_RELEASE_PROFILE 0
#endif

#ifndef MINIKMB_TARGET_HIGH_SPEED
#define MINIKMB_TARGET_HIGH_SPEED MINIKMB_RELEASE_PROFILE
#endif

#ifndef MINIKMB_TARGET_HID
/* HID is a normal control feature. Diagnostic builds may explicitly set it to 0. */
#define MINIKMB_TARGET_HID 1
#endif

#ifndef MINIKMB_TARGET_NCM
#ifdef MINIKMB_USB_HS_ONLY
#define MINIKMB_TARGET_NCM 0
#else
#define MINIKMB_TARGET_NCM 1
#endif
#endif

#ifndef MINIKMB_TARGET_CDC_BRIDGE
#define MINIKMB_TARGET_CDC_BRIDGE 1
#endif

#ifndef MINIKMB_TARGET_MSC
#ifdef MINIKMB_USB_HS_ONLY
#define MINIKMB_TARGET_MSC 0
#else
#define MINIKMB_TARGET_MSC 1
#endif
#endif

#endif

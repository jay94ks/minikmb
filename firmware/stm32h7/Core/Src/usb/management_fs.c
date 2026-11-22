#include "main.h"
#include "management_fs.h"
#include "management_protocol.h"
#include "storage_service.h"
#include "target_usb.h"
#if MINIKMB_TARGET_NCM
#include "target_network.h"
#endif
#if MINIKMB_TARGET_HID
#include "target_hid_usb.h"
#endif
#include "FreeRTOS.h"
#include "task.h"
#include "cmsis_os2.h"
#include "tusb.h"

void ManagementMsc_Poll(bool configured);
#include "device/dcd.h"

static StaticTask_t fs_task_control;
static StackType_t fs_task_stack[8192U / sizeof(StackType_t)]
    __attribute__((section(".rtos_stacks"), aligned(32)));
static volatile bool session_reset;
#if MINIKMB_MANAGEMENT_FAST_CDC
static uint8_t rx[CFG_TUD_CDC_RX_EPSIZE];
#else
static uint8_t rx[128];
#endif

static bool RngRead(uint32_t *word)
{
  const TickType_t start = xTaskGetTickCount();
  do {
    const uint32_t status = RNG->SR;
    if ((status & (RNG_SR_CECS | RNG_SR_SECS | RNG_SR_CEIS | RNG_SR_SEIS)) != 0U) {
      CLEAR_BIT(RNG->CR, RNG_CR_RNGEN);
      RNG->SR = 0;
      SET_BIT(RNG->CR, RNG_CR_RNGEN);
      return false;
    }
    if ((status & RNG_SR_DRDY) != 0U) {
      *word = RNG->DR;
      return true;
    }
    vTaskDelay(1);
  } while (xTaskGetTickCount() - start < pdMS_TO_TICKS(20));
  return false;
}

static uint64_t NewSession(void)
{
  uint32_t low, high;
  return RngRead(&low) && RngRead(&high) ? ((uint64_t)high << 32) | low : 0U;
}

static void InitBoard(void)
{
  RCC_PeriphCLKInitTypeDef clocks = {
    .PeriphClockSelection = RCC_PERIPHCLK_RNG,
    .RngClockSelection = RCC_RNGCLKSOURCE_PLL,
  };
  if (HAL_RCCEx_PeriphCLKConfig(&clocks) != HAL_OK) {
    Error_Handler();
  }

  __HAL_RCC_RNG_CLK_ENABLE();
  SET_BIT(RNG->CR, RNG_CR_RNGEN);

  __HAL_RCC_GPIOA_CLK_ENABLE();
  GPIO_InitTypeDef gpio = {
    .Pin = GPIO_PIN_11 | GPIO_PIN_12,
    .Mode = GPIO_MODE_AF_PP,
    .Pull = GPIO_NOPULL,
    .Speed = GPIO_SPEED_FREQ_HIGH,
    .Alternate = GPIO_AF10_OTG1_FS,
  };
  HAL_GPIO_Init(GPIOA, &gpio);
  __HAL_RCC_USB_OTG_FS_CLK_ENABLE();
  __HAL_RCC_USB2_OTG_FS_FORCE_RESET();
  __HAL_RCC_USB2_OTG_FS_RELEASE_RESET();
  NVIC_ClearPendingIRQ(OTG_FS_IRQn);
  NVIC_SetPriority(OTG_FS_IRQn, configLIBRARY_MAX_SYSCALL_INTERRUPT_PRIORITY);
}

static bool DrainReply(void)
{
  size_t bytes;
  const uint8_t *data = Management_Output(&bytes);
  if (bytes != 0) {
    Management_OutputConsumed(tud_cdc_write(data, bytes));
  }
  (void)Management_Output(&bytes);
  return bytes == 0;
}

static void ManagementTask(void *arg)
{
  (void)arg;
  InitBoard();
  ManagementPlatform platform = {
    .new_session = NewSession,
    .storage = StorageService_Backend(),
    .optical = MINIKMB_TARGET_MSC,
    .target_state = TargetUsb_State,
    .serial = TargetSerial_Backend(),
#if MINIKMB_TARGET_HID
    .hid = TargetHidUsb_Backend(),
#endif
  };
#if MINIKMB_TARGET_NCM
  platform.network = TargetNetwork_Backend();
#endif
  for (unsigned i = 0; i < MANAGEMENT_UID_BYTES; ++i) {
    platform.uid[i] = ((const volatile uint8_t *)UID_BASE)[i];
  }
  StorageService_SetUid(platform.uid);
  Management_Init(&platform, HAL_GetTick());
  Management_Log(2, 1, "MiniKMB FS CDC started");
  const tud_configure_param_t device_config = {
    .dwc2 = {.vbus_sensing = false},
  };
  const tusb_rhport_init_t port = {.role = TUSB_ROLE_DEVICE, .speed = TUSB_SPEED_FULL};
  if (!tud_configure(0, TUD_CFGID_DWC2, &device_config) || !tusb_init(0, &port)) {
    Error_Handler();
  }

  uint32_t previous_tick = HAL_GetTick();
  uint64_t uptime = previous_tick;
  uint32_t used = 0, length = 0;
  for (;;) {
    tud_task_ext(1, false);
    ManagementMsc_Poll(tud_mounted());
    const uint32_t tick = HAL_GetTick();
    uptime += (uint32_t)(tick - previous_tick);
    previous_tick = tick;
    Management_Tick(uptime);
    if (session_reset) {
      session_reset = false;
      used = length = 0;
      Management_Reset();
    }
    if (!tud_mounted()) continue;
    if (DrainReply()) {
      while (!session_reset) {
        if (used == length) {
          if (tud_cdc_available() == 0) break;
          length = tud_cdc_read(rx, sizeof(rx));
          used = 0;
          if (length == 0 || session_reset) break;
        }
        const uint8_t byte = rx[used++];
        Management_Input(byte);
        /* Only a frame delimiter can dispatch a command and produce its response. */
        if (byte == 0 && !DrainReply()) break;
      }
      if (!session_reset && used == length && tud_cdc_available() == 0 &&
          DrainReply() && tud_cdc_write_available() == CFG_TUD_CDC_TX_BUFSIZE) {
        Management_Poll();
        (void)DrainReply();
      }
    }
    (void)tud_cdc_write_flush();
  }
}

void ManagementUsb_Start(void)
{
  StorageService_Start();
  if (xTaskCreateStatic(ManagementTask, "mgmt-fs",
                        sizeof(fs_task_stack) / sizeof(fs_task_stack[0]),
                        NULL, (UBaseType_t)osPriorityAboveNormal,
                        fs_task_stack, &fs_task_control) == NULL) {
    Error_Handler();
  }
}

void OTG_FS_IRQHandler(void)
{
  tusb_int_handler(0, true);
}

void tud_event_hook_cb(uint8_t rhport, uint32_t event, bool in_isr)
{
  (void)in_isr;
  if (rhport == 0 && (event == DCD_EVENT_BUS_RESET || event == DCD_EVENT_UNPLUGGED)) {
    session_reset = true;
  }
}

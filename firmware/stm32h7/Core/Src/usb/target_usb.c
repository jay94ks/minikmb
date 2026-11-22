#include "main.h"
#include "target_usb.h"
#if MINIKMB_TARGET_HID
#include "target_hid_usb.h"
#endif
#include "FreeRTOS.h"
#include "task.h"
#include "cmsis_os2.h"
#include "target_tusb_namespace.h"
#include "tusb.h"
#include "device/dcd.h"
#if MINIKMB_TARGET_MSC
#include "target_msc.h"
#include "storage_service.h"
#endif
#if MINIKMB_TARGET_NCM
#include "target_network.h"
static volatile bool network_bus_reset;
#endif

static StaticTask_t target_control;
static StackType_t target_stack[8192U / sizeof(StackType_t)]
    __attribute__((section(".rtos_stacks"), aligned(32)));
static volatile uint8_t target_state;
extern bool target_usbd_event_queue_overflowed(void);

/* SWD-visible count of times the target device stack recovered its event queue. */
static volatile uint32_t target_queue_recoveries;

/* DTCM values can be read by SWD even before a CDC session is available. */
static volatile struct {
  uint32_t irq_flags;
  uint32_t resets;
  uint32_t enumerations;
  uint32_t setups;
  uint32_t enum_speed;
  uint32_t init_stage;
  uint32_t clkout_reset_hz;
  uint32_t clkout_before_link_hz;
} target_diagnostics;

#if MINIKMB_TARGET_CDC_BRIDGE
enum { SERIAL_FIFO_BYTES = 16 * 1024, SERIAL_CHUNK = 256 };
static uint8_t to_target_bytes[SERIAL_FIFO_BYTES]
    __attribute__((section(".axi_serial"), aligned(32)));
static uint8_t from_target_bytes[SERIAL_FIFO_BYTES]
    __attribute__((section(".axi_serial"), aligned(32)));
static tu_fifo_t to_target = TU_FIFO_INIT(to_target_bytes, SERIAL_FIFO_BYTES, false);
static tu_fifo_t from_target = TU_FIFO_INIT(from_target_bytes, SERIAL_FIFO_BYTES, false);
static ManagementSerialStatus serial_status;
static bool serial_active, serial_flush, serial_connected;
static uint32_t serial_epoch;

/* Both USB tasks call these helpers. The USB stack stays with the target task. */
static void SerialDiscard(void)
{
  serial_status.tx_discarded += tu_fifo_count(&to_target);
  serial_status.rx_discarded += tu_fifo_count(&from_target);
  tu_fifo_clear(&to_target);
  tu_fifo_clear(&from_target);
  serial_status.open = 0U;
  serial_flush = true;
  ++serial_epoch;
}

static void SerialSession(bool active)
{
  taskENTER_CRITICAL();
  if (serial_active != active) {
    SerialDiscard();
    serial_active = active;
  }
  taskEXIT_CRITICAL();
}

static uint16_t SerialSubmit(const uint8_t *data, uint16_t length)
{
  uint16_t status = 9U;
  taskENTER_CRITICAL();
  if (serial_status.open) {
    if (tu_fifo_remaining(&to_target) < length) {
      status = 4U;
    } else {
      const uint16_t stored = tu_fifo_write_n(&to_target, data, length);
      configASSERT(stored == length);
      status = 0U;
    }
  }
  taskEXIT_CRITICAL();
  return status;
}

static uint16_t SerialTakeRx(uint8_t *data, uint16_t capacity)
{
  taskENTER_CRITICAL();
  const uint16_t size = serial_status.open
      ? tu_fifo_read_n(&from_target, data, capacity) : 0U;
  taskEXIT_CRITICAL();
  return size;
}

static void SerialStatus(ManagementSerialStatus *status)
{
  taskENTER_CRITICAL();
  *status = serial_status;
  status->tx_queued = tu_fifo_count(&to_target);
  status->rx_queued = tu_fifo_count(&from_target);
  taskEXIT_CRITICAL();
}

void tud_cdc_line_state_cb(uint8_t instance, bool dtr, bool rts)
{
  (void)rts;
  if (instance == 0U && !dtr) {
    taskENTER_CRITICAL();
    SerialDiscard();
    taskEXIT_CRITICAL();
  }
}

static void SerialPoll(void)
{
  uint8_t pending[SERIAL_CHUNK];
  cdc_line_coding_t coding;
  tud_cdc_get_line_coding(&coding);
  const bool connected = tud_cdc_connected();
  const uint16_t tx_capacity = tu_min16(sizeof(pending), tud_cdc_write_available());
  const uint16_t rx_available = tu_min16(sizeof(pending), tud_cdc_available());

  taskENTER_CRITICAL();
  if (serial_connected && !connected) SerialDiscard();
  serial_connected = connected;
  serial_status.baud = coding.bit_rate;
  serial_status.stop_bits = coding.stop_bits;
  serial_status.parity = coding.parity;
  serial_status.data_bits = coding.data_bits;
  const bool flush = serial_flush;
  serial_flush = false;
  serial_status.open = serial_active && connected && !flush;
  if (!serial_status.open) {
    taskEXIT_CRITICAL();
    if (flush && tud_mounted()) {
      const uint32_t dropped_rx = tud_cdc_available();
      const uint32_t dropped_tx = CFG_TUD_CDC_TX_BUFSIZE - tud_cdc_write_available();
      tud_cdc_read_flush();
      tud_cdc_write_clear();
      taskENTER_CRITICAL();
      serial_status.rx_discarded += dropped_rx;
      serial_status.tx_discarded += dropped_tx;
      taskEXIT_CRITICAL();
    }
    return;
  }
  const uint32_t epoch = serial_epoch;
  const uint16_t to_send = tu_fifo_read_n(&to_target, pending, tx_capacity);
  const uint16_t to_read = tu_min16(rx_available, tu_fifo_remaining(&from_target));
  taskEXIT_CRITICAL();

  if (to_send != 0U) {
    const uint32_t written = tud_cdc_write(pending, to_send);
    configASSERT(written == to_send);
    tud_cdc_write_flush();
  }
  if (to_read != 0U) {
    const uint16_t length = (uint16_t)tud_cdc_read(pending, to_read);
    taskENTER_CRITICAL();
    if (epoch == serial_epoch) {
      const uint16_t stored = tu_fifo_write_n(&from_target, pending, length);
      configASSERT(stored == length);
    } else {
      serial_status.rx_discarded += length;
    }
    taskEXIT_CRITICAL();
  }
}
#endif

const ManagementSerialBackend *TargetSerial_Backend(void)
{
#if MINIKMB_TARGET_CDC_BRIDGE
  static const ManagementSerialBackend backend = {
    .set_session = SerialSession, .submit = SerialSubmit,
    .take_rx = SerialTakeRx, .status = SerialStatus,
  };
  return &backend;
#else
  return NULL;
#endif
}

void UsbBoard_InitClock(void)
{
  RCC_PeriphCLKInitTypeDef clock = {
    .PeriphClockSelection = RCC_PERIPHCLK_USB,
    .UsbClockSelection = RCC_USBCLKSOURCE_PLL,
  };
  if (HAL_RCCEx_PeriphCLKConfig(&clock) != HAL_OK) {
    Error_Handler();
  }
  HAL_PWREx_EnableUSBVoltageDetector();
}

static uint32_t MeasureUlpiClock(void)
{
  /* TIM2 divides external CLKOUT by four. DWT provides the sampling duration. */
  const uint32_t demcr = CoreDebug->DEMCR;
  const uint32_t dwt_ctrl = DWT->CTRL;
  CoreDebug->DEMCR = demcr | CoreDebug_DEMCR_TRCENA_Msk;
  DWT->CTRL = dwt_ctrl | DWT_CTRL_CYCCNTENA_Msk;
  const uint32_t start_tick = HAL_GetTick();
  const uint32_t start_cycles = DWT->CYCCNT;
  const uint32_t start_edges = TIM2->CNT;
  while (DWT->CYCCNT - start_cycles < SystemCoreClock / 1000U &&
         HAL_GetTick() - start_tick < 3U) {
  }
  const uint32_t edges = TIM2->CNT - start_edges;
  const uint32_t cycles = DWT->CYCCNT - start_cycles;
  DWT->CTRL = dwt_ctrl;
  CoreDebug->DEMCR = demcr;
  return cycles == 0U ? 0U : (uint32_t)((uint64_t)edges * 4U * SystemCoreClock / cycles);
}

static void StartUlpiClockCounter(GPIO_InitTypeDef pin)
{
  pin.Pin = GPIO_PIN_5;
  pin.Alternate = GPIO_AF1_TIM2;
  HAL_GPIO_Init(GPIOA, &pin);

  /* TIM2 is unused elsewhere. */
  __HAL_RCC_TIM2_CLK_ENABLE();
  __HAL_RCC_TIM2_FORCE_RESET();
  __HAL_RCC_TIM2_RELEASE_RESET();
  TIM2->ARR = UINT32_MAX;
  TIM2->SMCR = TIM_SMCR_ECE | TIM_SMCR_ETPS_1;
  TIM2->CR1 = TIM_CR1_CEN;
}

static bool WaitUlpiReady(uint32_t stage, bool require_dir_low, volatile uint32_t *measured)
{
  target_diagnostics.init_stage = stage;
  const TickType_t start = xTaskGetTickCount();
  unsigned good = 0U;
  do {
    const uint32_t hz = MeasureUlpiClock();
    *measured = hz;
    const bool dir_low = (GPIOC->IDR & GPIO_PIN_2) == 0U;
    if (hz >= 58800000U && hz <= 61200000U && (!require_dir_low || dir_low)) {
      if (++good == 3U) return true;
    } else {
      good = 0U;
    }
    vTaskDelay(1);
  } while (xTaskGetTickCount() - start < pdMS_TO_TICKS(250));
  return false;
}

static bool InitUlpi(void)
{
  __HAL_RCC_GPIOA_CLK_ENABLE();
  __HAL_RCC_GPIOB_CLK_ENABLE();
  __HAL_RCC_GPIOC_CLK_ENABLE();
  __HAL_RCC_SYSCFG_CLK_ENABLE();

  /* ULPI timing is characterized with the I/O compensation cell enabled.
     Keep this shared by the FS fallback and HS profiles. CSI clocks the cell;
     it does not replace the PHY's external 60 MHz ULPI clock. */
  target_diagnostics.init_stage = 10U;
  __HAL_RCC_CSI_ENABLE();
  TickType_t compensation_start = xTaskGetTickCount();
  while (__HAL_RCC_GET_FLAG(RCC_FLAG_CSIRDY) == RESET) {
    if ((TickType_t)(xTaskGetTickCount() - compensation_start) >= pdMS_TO_TICKS(20)) {
      return false;
    }
    vTaskDelay(1);
  }
  target_diagnostics.init_stage = 11U;
  HAL_EnableCompensationCell();
  compensation_start = xTaskGetTickCount();
  while ((SYSCFG->CCCSR & SYSCFG_CCCSR_READY) == 0U) {
    if ((TickType_t)(xTaskGetTickCount() - compensation_start) >= pdMS_TO_TICKS(20)) {
      return false;
    }
    vTaskDelay(1);
  }

  /* The VIH package brings out PC2_C and PC3_C for DIR and NXT. */
  HAL_SYSCFG_AnalogSwitchConfig(SYSCFG_SWITCH_PC2, SYSCFG_SWITCH_PC2_CLOSE);
  HAL_SYSCFG_AnalogSwitchConfig(SYSCFG_SWITCH_PC3, SYSCFG_SWITCH_PC3_CLOSE);
  /* Keep the PHY awake and protect its data bus until the link is ready. */
  HAL_GPIO_WritePin(GPIOC, GPIO_PIN_0, GPIO_PIN_SET);
  GPIO_InitTypeDef stp = {
    .Pin = GPIO_PIN_0, .Mode = GPIO_MODE_OUTPUT_PP, .Pull = GPIO_NOPULL,
    .Speed = GPIO_SPEED_FREQ_VERY_HIGH,
  };
  HAL_GPIO_Init(GPIOC, &stp);

  GPIO_InitTypeDef ulpi = {
    .Mode = GPIO_MODE_AF_PP,
    .Pull = GPIO_NOPULL,
    .Speed = GPIO_SPEED_FREQ_VERY_HIGH,
    .Alternate = GPIO_AF10_OTG2_HS,
  };
  ulpi.Pin = GPIO_PIN_3 | GPIO_PIN_5;                        /* D0, CLK */
  HAL_GPIO_Init(GPIOA, &ulpi);
  ulpi.Pin = GPIO_PIN_0 | GPIO_PIN_1 | GPIO_PIN_5 | GPIO_PIN_10 |
             GPIO_PIN_11 | GPIO_PIN_12 | GPIO_PIN_13;        /* D1..D7 */
  HAL_GPIO_Init(GPIOB, &ulpi);
  ulpi.Pin = GPIO_PIN_2 | GPIO_PIN_3;                         /* DIR, NXT */
  HAL_GPIO_Init(GPIOC, &ulpi);
  StartUlpiClockCounter(ulpi);
  bool ready = WaitUlpiReady(4U, false, &target_diagnostics.clkout_reset_hz);
  if (ready) {
    /* Keep STP asserted for wake-up. Release RESET only with CLKOUT running. */
    HAL_GPIO_WritePin(USB_ULPI_RST_GPIO_Port, USB_ULPI_RST_Pin, GPIO_PIN_SET);
    vTaskDelay(pdMS_TO_TICKS(2));
    ready = WaitUlpiReady(9U, false, &target_diagnostics.clkout_reset_hz);
  }
  if (ready) {
    HAL_GPIO_WritePin(USB_ULPI_RST_GPIO_Port, USB_ULPI_RST_Pin, GPIO_PIN_RESET);
    vTaskDelay(pdMS_TO_TICKS(2));
    ready = WaitUlpiReady(5U, true, &target_diagnostics.clkout_before_link_hz);
  }

  TIM2->CR1 = 0U;
  __HAL_RCC_TIM2_CLK_DISABLE();
  ulpi.Pin = GPIO_PIN_5;
  HAL_GPIO_Init(GPIOA, &ulpi);
  if (!ready) return false;

  __HAL_RCC_USB_OTG_HS_ULPI_CLK_ENABLE();
  __HAL_RCC_USB_OTG_HS_CLK_ENABLE();
  __HAL_RCC_USB_OTG_HS_FORCE_RESET();
  __HAL_RCC_USB_OTG_HS_RELEASE_RESET();
  ulpi.Pin = GPIO_PIN_0;
  HAL_GPIO_Init(GPIOC, &ulpi);
  target_diagnostics.init_stage = 2U;
  NVIC_ClearPendingIRQ(OTG_HS_IRQn);
  NVIC_SetPriority(OTG_HS_IRQn, configLIBRARY_MAX_SYSCALL_INTERRUPT_PRIORITY);
  return true;
}

static bool StartTargetStack(void)
{
  const tud_configure_param_t config = {.dwc2 = {.vbus_sensing = true}};
  const tusb_rhport_init_t port = {
    .role = TUSB_ROLE_DEVICE,
    .speed = MINIKMB_TARGET_HIGH_SPEED ? TUSB_SPEED_HIGH : TUSB_SPEED_FULL,
  };
  if (!tud_configure(1, TUD_CFGID_DWC2, &config) || !tusb_init(1, &port)) {
    return false;
  }
  target_diagnostics.init_stage = 3U;
  return true;
}

static void WaitForTargetPhy(void)
{
  while (!InitUlpi()) {
    /* Keep servicing management USB while the target PHY is unavailable. */
    vTaskDelay(pdMS_TO_TICKS(250));
  }
}

static bool RecoverTargetStack(void)
{
  ++target_queue_recoveries;
  target_state = 0U;
  NVIC_DisableIRQ(OTG_HS_IRQn);
  (void)tud_disconnect();
#if MINIKMB_TARGET_CDC_BRIDGE
  taskENTER_CRITICAL();
  SerialDiscard();
  serial_connected = false;
  taskEXIT_CRITICAL();
#endif
#if MINIKMB_TARGET_NCM
  TargetNetwork_Poll(false, true);
#endif
#if MINIKMB_TARGET_HID
  tud_umount_cb();
#endif
  vTaskDelay(pdMS_TO_TICKS(250));
  WaitForTargetPhy();
  if (!tusb_deinit(1)) {
    return false;
  }
  return StartTargetStack();
}

static void TargetTask(void *arg)
{
  (void)arg;
  WaitForTargetPhy();
  if (!StartTargetStack()) {
    vTaskSuspend(NULL);
    return;
  }
  #if !MINIKMB_TARGET_CDC_BRIDGE
  uint8_t echo[512];
  #endif
  for (;;) {
    tud_task_ext(1, false);
    if (target_usbd_event_queue_overflowed()) {
      if (!RecoverTargetStack()) {
        vTaskSuspend(NULL);
        return;
      }
      continue;
    }
#if MINIKMB_TARGET_HID
    TargetHidUsb_Poll(HAL_GetTick());
#endif
#if MINIKMB_TARGET_MSC
    const bool optical = StorageService_OpticalEnabled();
    if (optical != TargetMsc_OpticalVisible()) {
      target_state = 0;
      (void)tud_disconnect();
      TargetMsc_Poll(false);
#if MINIKMB_TARGET_NCM
      TargetNetwork_Poll(false, true);
#endif
      (void)tusb_deinit(1);
      vTaskDelay(pdMS_TO_TICKS(250));
      TargetMsc_SetOpticalVisible(optical);
      if (!StartTargetStack()) { vTaskSuspend(NULL); return; }
      StorageService_OpticalApplied(optical);
      continue;
    }
#endif
    const bool mounted = tud_mounted();
#if MINIKMB_TARGET_MSC
    TargetMsc_Poll(mounted);
#endif
    target_state = tud_suspended() ? 3U :
                   (mounted ? (tud_speed_get() == TUSB_SPEED_HIGH ? 2U : 1U) : 0U);
#if MINIKMB_TARGET_NCM
    taskENTER_CRITICAL();
    const bool reset_network = network_bus_reset;
    network_bus_reset = false;
    taskEXIT_CRITICAL();
    TargetNetwork_Poll(mounted && target_state != 3U, reset_network);
#endif
#if MINIKMB_TARGET_CDC_BRIDGE
    SerialPoll();
#else
    if (mounted && tud_cdc_available() != 0U) {
      uint32_t capacity = tud_cdc_write_available();
      if (capacity > sizeof(echo)) capacity = sizeof(echo);
      if (capacity != 0U) {
        const uint32_t bytes = tud_cdc_read(echo, capacity);
        const uint32_t written = tud_cdc_write(echo, bytes);
        configASSERT(written == bytes);
        tud_cdc_write_flush();
      }
    }
#endif
  }
}

void TargetUsb_Start(void)
{
#if MINIKMB_TARGET_HID
  TargetHidUsb_Init();
#endif
  if (xTaskCreateStatic(TargetTask, "target-hs",
                        sizeof(target_stack) / sizeof(target_stack[0]),
                        NULL, (UBaseType_t)osPriorityAboveNormal,
                        target_stack, &target_control) == NULL) {
    Error_Handler();
  }
}

uint8_t TargetUsb_State(void)
{
  return target_state;
}

void OTG_HS_IRQHandler(void)
{
  if (target_usbd_event_queue_overflowed()) {
    NVIC_DisableIRQ(OTG_HS_IRQn);
    return;
  }

  const uint32_t flags = USB_OTG_HS->GINTSTS & USB_OTG_HS->GINTMSK;
  target_diagnostics.irq_flags |= flags;
  if ((flags & USB_OTG_GINTSTS_USBRST) != 0U) ++target_diagnostics.resets;
  if ((flags & USB_OTG_GINTSTS_ENUMDNE) != 0U) {
    const USB_OTG_DeviceTypeDef *device =
        (const USB_OTG_DeviceTypeDef *)(USB_OTG_HS_PERIPH_BASE + USB_OTG_DEVICE_BASE);
    const uint32_t speed = (device->DSTS & USB_OTG_DSTS_ENUMSPD_Msk) >> USB_OTG_DSTS_ENUMSPD_Pos;
    target_diagnostics.enum_speed = speed;
    if (speed == USB_OTG_SPEED_HIGH) {
      /* A previous reset may have selected the FS turnaround. */
      (void)USB_SetTurnaroundTime(USB_OTG_HS, HAL_RCC_GetHCLKFreq(), USBD_HS_SPEED);
    } else if (speed == USB_OTG_SPEED_HIGH_IN_FULL || speed == USB_OTG_SPEED_FULL) {
      /* Program the FS turnaround before TinyUSB handles the first request. */
      (void)USB_SetTurnaroundTime(USB_OTG_HS, HAL_RCC_GetHCLKFreq(), USBD_FS_SPEED);
    }
    ++target_diagnostics.enumerations;
  }
  tusb_int_handler(1, true);
}

void tud_event_hook_cb(uint8_t rhport, uint32_t event, bool in_isr)
{
  (void)in_isr;
#if MINIKMB_TARGET_NCM
  if (rhport == 1 && (event == DCD_EVENT_BUS_RESET || event == DCD_EVENT_UNPLUGGED)) network_bus_reset = true;
#endif
  if (rhport == 1 && event == DCD_EVENT_SETUP_RECEIVED) ++target_diagnostics.setups;
}

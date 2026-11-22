/* USER CODE BEGIN Header */
/**
  ******************************************************************************
  * File Name          : freertos.c
  * Description        : Code for freertos applications
  ******************************************************************************
  * @attention
  *
  * Copyright (c) 2026 STMicroelectronics.
  * All rights reserved.
  *
  * This software is licensed under terms that can be found in the LICENSE file
  * in the root directory of this software component.
  * If no LICENSE file comes with this software, it is provided AS-IS.
  *
  ******************************************************************************
  */
/* USER CODE END Header */

/* Includes ------------------------------------------------------------------*/
#include "FreeRTOS.h"
#include "task.h"
#include "main.h"

/* Private includes ----------------------------------------------------------*/
/* USER CODE BEGIN Includes */
#include <stdint.h>

/* USER CODE END Includes */

/* Private typedef -----------------------------------------------------------*/
/* USER CODE BEGIN PTD */

/* USER CODE END PTD */

/* Private define ------------------------------------------------------------*/
/* USER CODE BEGIN PD */

/* USER CODE END PD */

/* Private macro -------------------------------------------------------------*/
/* USER CODE BEGIN PM */

/* USER CODE END PM */

/* Private variables ---------------------------------------------------------*/
/* USER CODE BEGIN Variables */
uint8_t ucHeap[configTOTAL_HEAP_SIZE] __attribute__((section(".rtos_heap"), aligned(32)));

static StaticTask_t idleControl;
static StaticTask_t timerControl;
static StackType_t idleStack[configMINIMAL_STACK_SIZE]
    __attribute__((section(".rtos_stacks"), aligned(8)));
static StackType_t timerStack[configTIMER_TASK_STACK_DEPTH]
    __attribute__((section(".rtos_stacks"), aligned(8)));

/* Available to SWD before the USB diagnostic transport is implemented. */
static volatile uint32_t allocationFailures;

/* USER CODE END Variables */

/* Private function prototypes -----------------------------------------------*/
/* USER CODE BEGIN FunctionPrototypes */

/* USER CODE END FunctionPrototypes */

/* Private application code --------------------------------------------------*/
/* USER CODE BEGIN Application */
extern uint32_t g_pfnVectors[];
extern uint64_t __axi_start__[], __axi_end__[];
extern uint32_t __d2_start__[], __d2_end__[];

void BoardMemory_Init(void)
{
  static const struct
  {
    uint32_t base;
    uint8_t size;
    uint8_t permission;
    uint8_t execute;
    uint8_t tex;
    uint8_t cacheable;
    uint8_t bufferable;
    uint8_t shareable;
  } regions[] = {
    /* base, size, AP, XN, TEX, C, B, S */
    {0x08000000U, MPU_REGION_SIZE_2MB,  MPU_REGION_PRIV_RO_URO, 0, 0, 1, 0, 0},
    {0x20000000U, MPU_REGION_SIZE_128KB, MPU_REGION_FULL_ACCESS, 1, 1, 0, 0, 0},
    {0x20018000U, MPU_REGION_SIZE_16KB, MPU_REGION_NO_ACCESS,   1, 1, 0, 0, 0},
    {0x24000000U, MPU_REGION_SIZE_512KB, MPU_REGION_FULL_ACCESS, 1, 1, 1, 1, 0},
    {0x24060000U, MPU_REGION_SIZE_128KB, MPU_REGION_FULL_ACCESS, 1, 1, 0, 0, 1},
    {0x30000000U, MPU_REGION_SIZE_256KB, MPU_REGION_FULL_ACCESS, 1, 1, 1, 1, 0},
    {0x30040000U, MPU_REGION_SIZE_32KB, MPU_REGION_FULL_ACCESS, 1, 1, 1, 1, 0},
    {0x38000000U, MPU_REGION_SIZE_64KB, MPU_REGION_FULL_ACCESS, 1, 1, 0, 0, 0},
    {0x00000000U, MPU_REGION_SIZE_64KB, MPU_REGION_NO_ACCESS,   1, 1, 0, 0, 0},
  };

  /* Setting VTOR explicitly also prevents the ITCM guard catching vector fetches. */
  SCB->VTOR = (uint32_t)g_pfnVectors;
  __DSB();
  __ISB();
  /* Reset leaves the cache disabled and potentially uninitialized.
     Cleaning invalid tag RAM before the first invalidate can fault. */
  if ((SCB->CCR & SCB_CCR_DC_Msk) != 0U)
  {
    SCB_DisableDCache();
  }
  if ((SCB->CCR & SCB_CCR_IC_Msk) != 0U)
  {
    SCB_DisableICache();
  }
  HAL_MPU_Disable();

  __HAL_RCC_D2SRAM1_CLK_ENABLE();
  __HAL_RCC_D2SRAM2_CLK_ENABLE();
  __HAL_RCC_D2SRAM3_CLK_ENABLE();
  /* H743 SRAM4 has no run-mode RCC enable; crash record is reserved only. */
  for (uintptr_t address = (uintptr_t)__d2_start__; address < (uintptr_t)__d2_end__; address += 4U)
  {
    *(volatile uint32_t *)address = 0U;
  }
  /* A VSTR.D makes each AXI ECC initialization write a full aligned word. */
  const uint64_t zero = 0U;
  for (uintptr_t address = (uintptr_t)__axi_start__; address < (uintptr_t)__axi_end__; address += 8U)
  {
    __asm volatile ("vstr %P0, [%1]" :: "w" (zero), "r" (address) : "memory");
  }
  __DSB();

  for (uint32_t number = 0; number < (sizeof(regions) / sizeof(regions[0])); ++number)
  {
    MPU_Region_InitTypeDef region = {
      .Enable = MPU_REGION_ENABLE,
      .Number = (uint8_t)number,
      .BaseAddress = regions[number].base,
      .Size = regions[number].size,
      .TypeExtField = regions[number].tex,
      .AccessPermission = regions[number].permission,
      .DisableExec = regions[number].execute,
      .IsShareable = regions[number].shareable,
      .IsCacheable = regions[number].cacheable,
      .IsBufferable = regions[number].bufferable,
    };
    HAL_MPU_ConfigRegion(&region);
  }

  HAL_MPU_Enable(MPU_PRIVILEGED_DEFAULT);
  SCB_EnableICache();
  SCB_EnableDCache();
}

void vApplicationGetIdleTaskMemory(StaticTask_t **control, StackType_t **stack,
                                   uint32_t *depth)
{
  *control = &idleControl;
  *stack = idleStack;
  *depth = configMINIMAL_STACK_SIZE;
}

void vApplicationGetTimerTaskMemory(StaticTask_t **control, StackType_t **stack,
                                    uint32_t *depth)
{
  *control = &timerControl;
  *stack = timerStack;
  *depth = configTIMER_TASK_STACK_DEPTH;
}

void vApplicationStackOverflowHook(TaskHandle_t task, char *name)
{
  (void)task;
  (void)name;
  Error_Handler();
}

void vApplicationMallocFailedHook(void)
{
  /* Reserve accounting for kernel task allocation. Application buffers are static. */
  taskENTER_CRITICAL();
  ++allocationFailures;
  taskEXIT_CRITICAL();
}

/* USER CODE END Application */


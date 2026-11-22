cmake_minimum_required(VERSION 3.13)

if(NOT DEFINED PICO_SDK_PATH)
    if(DEFINED ENV{PICO_SDK_PATH})
        set(PICO_SDK_PATH "$ENV{PICO_SDK_PATH}")
    else()
        message(FATAL_ERROR "Set PICO_SDK_PATH to an extracted Pico SDK 2.2.0 source tree")
    endif()
endif()
include(${PICO_SDK_PATH}/external/pico_sdk_import.cmake)

project(kbm_master C CXX ASM)
set(CMAKE_C_STANDARD 11)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

set(KBM_UART_BAUD 921600 CACHE STRING "UART baud rate")
set(KBM_UART0_TX_PIN 0 CACHE STRING "UART0 TX GPIO")
set(KBM_UART0_RX_PIN 1 CACHE STRING "UART0 RX GPIO")
set(KBM_UART1_TX_PIN 4 CACHE STRING "UART1 TX GPIO")
set(KBM_UART1_RX_PIN 5 CACHE STRING "UART1 RX GPIO")
set(KBM_USB_VID 0x8857 CACHE STRING "USB vendor ID")
set(KBM_USB_PID 0x1223 CACHE STRING "USB product ID")

set(KBM_AVAILABLE_PINS 0 1 2 3 4 5 6 7 8 9 19 20 21 22 23 24 25 26 27 28 29)
foreach(pin_var KBM_UART0_TX_PIN KBM_UART0_RX_PIN KBM_UART1_TX_PIN KBM_UART1_RX_PIN)
    if(NOT ${pin_var} IN_LIST KBM_AVAILABLE_PINS)
        message(FATAL_ERROR "${pin_var} must be one of GPIO 0..9 or 19..29")
    endif()
endforeach()

set(KBM_UART0_VALID_TX_PINS 0 12 16 28)
set(KBM_UART0_VALID_RX_PINS 1 13 17 29)
set(KBM_UART1_VALID_TX_PINS 4 8 20 24)
set(KBM_UART1_VALID_RX_PINS 5 9 21 25)
if(NOT KBM_UART0_TX_PIN IN_LIST KBM_UART0_VALID_TX_PINS)
    message(FATAL_ERROR "GPIO ${KBM_UART0_TX_PIN} is not a UART0 TX pin")
endif()
if(NOT KBM_UART0_RX_PIN IN_LIST KBM_UART0_VALID_RX_PINS)
    message(FATAL_ERROR "GPIO ${KBM_UART0_RX_PIN} is not a UART0 RX pin")
endif()
if(NOT KBM_UART1_TX_PIN IN_LIST KBM_UART1_VALID_TX_PINS)
    message(FATAL_ERROR "GPIO ${KBM_UART1_TX_PIN} is not a UART1 TX pin")
endif()
if(NOT KBM_UART1_RX_PIN IN_LIST KBM_UART1_VALID_RX_PINS)
    message(FATAL_ERROR "GPIO ${KBM_UART1_RX_PIN} is not a UART1 RX pin")
endif()

pico_sdk_init()

add_executable(kbm-master
    src/main.c
    src/protocol.c
    src/response_tracker.c
    src/master_commands.c
    src/cdc_transport.c
    src/uart_bridge.cpp
    src/usb_descriptors.c
    src/native_network.cpp
    src/native_dhcp.c
    ../common/usb_reset_interface.c
)

target_include_directories(kbm-master PRIVATE ${CMAKE_CURRENT_LIST_DIR}/src
    ${CMAKE_CURRENT_LIST_DIR}/../common)
target_sources(kbm-master PRIVATE ../common/uart_dma_transport.cpp)
target_compile_definitions(kbm-master PRIVATE KBM_DMA_UART_COUNT=2)
target_link_libraries(kbm-master PRIVATE hardware_dma)

target_compile_definitions(kbm-master PRIVATE
    PICO_XOSC_STARTUP_DELAY_MULTIPLIER=48
    KBM_UART_BAUD=${KBM_UART_BAUD}
    KBM_UART0_TX_PIN=${KBM_UART0_TX_PIN}
    KBM_UART0_RX_PIN=${KBM_UART0_RX_PIN}
    KBM_UART1_TX_PIN=${KBM_UART1_TX_PIN}
    KBM_UART1_RX_PIN=${KBM_UART1_RX_PIN}
    KBM_USB_VID=${KBM_USB_VID}
    KBM_USB_PID=${KBM_USB_PID}
    KBM_RESET_INTERFACE_NUMBER=2
    KBM_RESET_VIA_BAUD_RATE=1
)

target_compile_options(kbm-master PRIVATE -Wall -Wextra)
target_compile_options(kbm-master PRIVATE $<$<COMPILE_LANGUAGE:CXX>:-fno-exceptions;-fno-rtti>)

target_link_libraries(kbm-master PRIVATE
    pico_stdlib
    pico_multicore
    pico_sync
    pico_util
    pico_unique_id
    hardware_gpio
    hardware_uart
    hardware_watchdog
    pico_bootrom
    pico_usb_reset_interface
    tinyusb_device
    tinyusb_board
)

pico_enable_stdio_uart(kbm-master 0)
include(../common/ncm_driver.cmake)
kbm_add_ncm_driver(kbm-master)
pico_enable_stdio_usb(kbm-master 0)
pico_add_extra_outputs(kbm-master)


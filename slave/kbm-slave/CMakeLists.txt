cmake_minimum_required(VERSION 3.13)

# The local RP2040 SDK lives here unless PICO_SDK_PATH was supplied by the
# developer environment.
if(NOT DEFINED PICO_SDK_PATH)
    if(DEFINED ENV{PICO_SDK_PATH})
        set(PICO_SDK_PATH "$ENV{PICO_SDK_PATH}")
    else()
        message(FATAL_ERROR "Set PICO_SDK_PATH to an extracted Pico SDK 2.2.0 source tree")
    endif()
endif()
include(${PICO_SDK_PATH}/external/pico_sdk_import.cmake)

project(kbm_slave C CXX ASM)
set(CMAKE_C_STANDARD 11)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

set(KBM_UART_ID 0 CACHE STRING "UART peripheral (0 or 1)")
set(KBM_UART_TX_PIN 0 CACHE STRING "UART TX GPIO")
set(KBM_UART_RX_PIN 1 CACHE STRING "UART RX GPIO")
set(KBM_UART_BAUD 921600 CACHE STRING "UART baud rate")
set(KBM_USB_VID 0x8857 CACHE STRING "USB vendor ID")
set(KBM_USB_PID 0x1222 CACHE STRING "USB product ID")

if(NOT KBM_UART_ID MATCHES "^[01]$")
    message(FATAL_ERROR "KBM_UART_ID must be 0 or 1")
endif()

set(KBM_AVAILABLE_PINS 0 1 2 3 4 5 6 7 8 9 19 20 21 22 23 24 25 26 27 28 29)
if(NOT KBM_UART_TX_PIN IN_LIST KBM_AVAILABLE_PINS)
    message(FATAL_ERROR "KBM_UART_TX_PIN must be one of GPIO 0..9 or 19..29")
endif()
if(NOT KBM_UART_RX_PIN IN_LIST KBM_AVAILABLE_PINS)
    message(FATAL_ERROR "KBM_UART_RX_PIN must be one of GPIO 0..9 or 19..29")
endif()

if(KBM_UART_ID EQUAL 0)
    set(KBM_VALID_TX_PINS 0 12 16 28)
    set(KBM_VALID_RX_PINS 1 13 17 29)
else()
    set(KBM_VALID_TX_PINS 4 8 20 24)
    set(KBM_VALID_RX_PINS 5 9 21 25)
endif()
if(NOT KBM_UART_TX_PIN IN_LIST KBM_VALID_TX_PINS)
    message(FATAL_ERROR "GPIO ${KBM_UART_TX_PIN} is not a UART${KBM_UART_ID} TX pin")
endif()
if(NOT KBM_UART_RX_PIN IN_LIST KBM_VALID_RX_PINS)
    message(FATAL_ERROR "GPIO ${KBM_UART_RX_PIN} is not a UART${KBM_UART_ID} RX pin")
endif()

pico_sdk_init()

add_executable(kbm-slave
    src/main.c
    src/protocol.c
    src/command.c
    src/uart_transport.c
    src/hid_executor.c
    src/wake_bridge.cpp
    src/serial_bridge.c
    src/network_bridge.cpp
    src/disk_bridge.cpp
    src/usb_descriptors.c
    ../../common/usb_reset_interface.c
)

target_include_directories(kbm-slave PRIVATE ${CMAKE_CURRENT_LIST_DIR}/src
    ${CMAKE_CURRENT_LIST_DIR}/../../common)

target_compile_definitions(kbm-slave PRIVATE
    KBM_DMA_UART_COUNT=1
    # This custom board needs the longer XOSC settling time used by the other
    # USB HID project in C:/ARM.
    PICO_XOSC_STARTUP_DELAY_MULTIPLIER=48
    KBM_UART_ID=${KBM_UART_ID}
    KBM_UART_TX_PIN=${KBM_UART_TX_PIN}
    KBM_UART_RX_PIN=${KBM_UART_RX_PIN}
    KBM_UART_BAUD=${KBM_UART_BAUD}
    KBM_USB_VID=${KBM_USB_VID}
    KBM_USB_PID=${KBM_USB_PID}
    KBM_RESET_INTERFACE_NUMBER=3
    KBM_RESET_VIA_BAUD_RATE=0
)

target_compile_options(kbm-slave PRIVATE -Wall -Wextra)
target_compile_options(kbm-slave PRIVATE $<$<COMPILE_LANGUAGE:CXX>:-fno-exceptions;-fno-rtti>)

target_link_libraries(kbm-slave PRIVATE
    hardware_dma
    pico_stdlib
    pico_rand
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

include(../../common/ncm_driver.cmake)
target_sources(kbm-slave PRIVATE ../../common/uart_dma_transport.cpp)
kbm_add_ncm_driver(kbm-slave)
include(../../common/msc_driver.cmake)
kbm_add_msc_driver(kbm-slave)

pico_enable_stdio_uart(kbm-slave 0)
pico_enable_stdio_usb(kbm-slave 0)
pico_add_extra_outputs(kbm-slave)


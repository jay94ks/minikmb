cmake_minimum_required(VERSION 3.13)

set(PICO_BOARD pico CACHE STRING "Raspberry Pi Pico board")
if(NOT DEFINED PICO_SDK_PATH)
    set(PICO_SDK_PATH "$ENV{PICO_SDK_PATH}")
endif()
include("${PICO_SDK_PATH}/external/pico_sdk_import.cmake")
project(frwr VERSION 0.1.0 LANGUAGES C CXX ASM)
pico_sdk_init()

if(NOT PICO_SDK_VERSION_STRING VERSION_EQUAL "2.2.0")
    message(FATAL_ERROR "frwr uses Pico SDK 2.2.0 with its pinned TinyUSB submodule")
endif()
if(NOT PICO_BOARD STREQUAL "pico" OR NOT TARGET pico_stdio_usb)
    message(FATAL_ERROR "frwr requires the RP2040 Pico board and Pico SDK TinyUSB submodule")
endif()

add_executable(frwr main.c)
set_target_properties(frwr PROPERTIES C_STANDARD 11 C_STANDARD_REQUIRED YES)
target_compile_options(frwr PRIVATE -Wall -Wextra)
option(FRWR_CLANG22_COMPAT "Use Pico SDK compatibility helpers for Clang 22" OFF)
if(FRWR_CLANG22_COMPAT)
    target_compile_options(frwr PRIVATE
        "$<$<COMPILE_LANGUAGE:C,CXX>:SHELL:-include \"${CMAKE_CURRENT_SOURCE_DIR}/cmake/clang22-compat.h\">"
    )
endif()
# Reuse the SDK CDC descriptors, standard Reset driver and Windows WinUSB BOS.
# The main loop owns tud_task() and CDC traffic. It never initializes stdio.
target_link_libraries(frwr PRIVATE pico_stdlib hardware_spi pico_stdio_usb)
pico_enable_stdio_uart(frwr 0)
target_compile_definitions(frwr PRIVATE
    USBD_PRODUCT="frwr"
    PICO_STDIO_USB_ENABLE_IRQ_BACKGROUND_TASK=0
    PICO_STDIO_USB_ENABLE_RESET_VIA_BAUD_RATE=0
    PICO_STDIO_USB_ENABLE_RESET_VIA_VENDOR_INTERFACE=1
    PICO_STDIO_USB_RESET_INTERFACE_SUPPORT_MS_OS_20_DESCRIPTOR=1
)
pico_set_program_name(frwr "frwr")
pico_set_program_version(frwr ${PROJECT_VERSION})
pico_add_extra_outputs(frwr)

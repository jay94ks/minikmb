# SDK's read==0 path self-enqueues endlessly inside tud_task(), starving HID.
# Generate a cooperative variant; do not edit the shared SDK or spin on UART.
function(kbm_add_msc_driver target)
    set(source "${PICO_SDK_PATH}/lib/tinyusb/src/class/msc/msc_device.c")
    file(READ "${source}" code)
    set(old "dcd_event_xfer_complete(rhport, p_msc->ep_in, 0, XFER_RESULT_SUCCESS, false);")
    string(FIND "${code}" "${old}" found)
    if(found EQUAL -1)
        message(FATAL_ERROR "Unsupported TinyUSB MSC read deferral implementation")
    endif()
    string(REPLACE "static mscd_interface_t _mscd_itf;" "static mscd_interface_t _mscd_itf;\nstatic bool kbm_read_pending;" code "${code}")
    # USB reenumeration may remove MSC altogether. Never resume a previous
    # configuration's deferred transfer on an unopened/stale endpoint.
    string(REPLACE "tu_memclr(&_mscd_itf, sizeof(mscd_interface_t));"
        "tu_memclr(&_mscd_itf, sizeof(mscd_interface_t));\n  kbm_read_pending = false;" code "${code}")
    string(REPLACE "${old}" "kbm_read_pending = true;" code "${code}")
    string(REPLACE "static void proc_read10_cmd(uint8_t rhport, mscd_interface_t* p_msc) {"
        "static void proc_read10_cmd(uint8_t rhport, mscd_interface_t* p_msc) {\n  kbm_read_pending = false;" code "${code}")
    # Append outside the original guard with the same compile-time condition.
    string(APPEND code "\n#if (CFG_TUD_ENABLED && CFG_TUD_MSC)\nvoid kbm_msc_resume_read(void) {\n if (!kbm_read_pending) return;\n kbm_read_pending = false;\n if (_mscd_itf.stage == MSC_STAGE_DATA && _mscd_itf.cbw.command[0] == SCSI_CMD_READ_10) proc_read10_cmd(0, &_mscd_itf);\n}\n#endif\n")
    file(GENERATE OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/kbm_msc_device.c" CONTENT "${code}")
    set_source_files_properties("${source}" PROPERTIES HEADER_FILE_ONLY TRUE)
    target_sources(${target} PRIVATE "${CMAKE_CURRENT_BINARY_DIR}/kbm_msc_device.c")
    target_include_directories(${target} PRIVATE "${PICO_SDK_PATH}/lib/tinyusb/src/class/msc")
endfunction()

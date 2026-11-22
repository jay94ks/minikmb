# Keep SDK untouched. Windows UsbNcm requires divisor >= 4 (TinyUSB #3913).
function(kbm_add_ncm_driver target)
    set(source "${PICO_SDK_PATH}/lib/tinyusb/src/class/net/ncm_device.c")
    file(READ "${source}" code)
    string(REGEX REPLACE "(\\.wNdb(In|Out)Divisor[ ]*=[ ]*)1," "\\14," code "${code}")
    foreach(speed 480000000 12000000)
        string(REPLACE "= ${speed};" "= 115200;" code "${code}")
    endforeach()
    string(REPLACE "#endif // ( CFG_TUD_ENABLED && CFG_TUD_NCM )"
        "bool kbm_ncm_link_ready(void) { return ncm_interface.itf_data_alt == 1; }\n#endif // ( CFG_TUD_ENABLED && CFG_TUD_NCM )"
        code "${code}")
    file(GENERATE OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/kbm_ncm_device.c" CONTENT "${code}")
    set_source_files_properties("${source}" PROPERTIES HEADER_FILE_ONLY TRUE)
    target_sources(${target} PRIVATE "${CMAKE_CURRENT_BINARY_DIR}/kbm_ncm_device.c")
    target_include_directories(${target} PRIVATE "${PICO_SDK_PATH}/lib/tinyusb/src/class/net")
endfunction()

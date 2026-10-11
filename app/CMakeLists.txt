# ESP-IDF's requirement scan includes this file in script mode, where CONFIGURE_DEPENDS is an error.
if(NOT CMAKE_SCRIPT_MODE_FILE)
    set(APP_GLOB_FLAGS CONFIGURE_DEPENDS)
endif()

file(GLOB_RECURSE APP_SRCS ${APP_GLOB_FLAGS}
     "${CMAKE_CURRENT_SOURCE_DIR}/*.c"
     "${CMAKE_CURRENT_SOURCE_DIR}/*.cpp")

set(APP_REQUIRES audio_framework bsp esp_mm esp_timer harness jpeg_decode_enhanced lz4_block ui_framework usb_device)
if(ESP_PLATFORM)
    list(APPEND APP_REQUIRES nvs_flash)
endif()

idf_component_register(
    SRCS ${APP_SRCS}
    INCLUDE_DIRS "."
    REQUIRES ${APP_REQUIRES})

file(GLOB APP_RESOURCES ${APP_GLOB_FLAGS} "${CMAKE_CURRENT_SOURCE_DIR}/resources/resources.json")
if(APP_RESOURCES AND NOT CMAKE_SCRIPT_MODE_FILE)
    resgen_add_resources(TARGET ${COMPONENT_LIB} DEFINITION ${APP_RESOURCES})
endif()

# ESP-IDF's requirement scan includes this file in script mode, where CONFIGURE_DEPENDS is an error.
if(NOT CMAKE_SCRIPT_MODE_FILE)
    set(APP_GLOB_FLAGS CONFIGURE_DEPENDS)
endif()

file(GLOB_RECURSE APP_SRCS ${APP_GLOB_FLAGS}
     "${CMAKE_CURRENT_SOURCE_DIR}/*.c"
     "${CMAKE_CURRENT_SOURCE_DIR}/*.cpp")

idf_component_register(
    SRCS ${APP_SRCS}
    INCLUDE_DIRS "."
    REQUIRES bsp ui_framework)

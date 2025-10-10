idf_component_register(
    SRCS "usbd.c"
    INCLUDE_DIRS "."
    PRIV_REQUIRES "usb"
)

idf_component_get_property(tusb_lib espressif__tinyusb COMPONENT_LIB)
target_include_directories(${tusb_lib} PUBLIC "${COMPONENT_DIR}")
target_sources(${tusb_lib} PUBLIC "${COMPONENT_DIR}/usb_descriptors.c")

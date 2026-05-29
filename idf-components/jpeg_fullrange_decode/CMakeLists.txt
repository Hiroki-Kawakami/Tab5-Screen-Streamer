idf_component_register(
    SRCS
        "src/jpeg_fullrange_decode.c"
    INCLUDE_DIRS "include"
    PRIV_REQUIRES esp_driver_jpeg hal esp_mm esp_hw_support esp_pm driver
)

# NOTE: depends on ESP-IDF internal headers (jpeg_private.h, private/*.h,
# esp_private/dma2d.h, hal/jpeg_ll.h, hal/dma2d_ll.h). These live inside the
# esp_driver_jpeg / esp_hw_support components and are not exported as public
# include dirs, so we add them explicitly. We reuse the IDF engine
# (jpeg_new_decoder_engine) and only reimplement the per-frame process step,
# so there are no duplicate-symbol clashes with esp_driver_jpeg.
target_include_directories(${COMPONENT_LIB} PRIVATE
    "$ENV{IDF_PATH}/components/esp_driver_jpeg"
    "$ENV{IDF_PATH}/components/esp_driver_jpeg/private"
)

#include "tusb.h"

#define USBD_VID            (0x303a) // Espressif
#define USBD_PID            (0x4020)
#define USBD_MANUFACTURER   "Espressif Systems"
#define USBD_PRODUCT        "Espressif Device"
#define USBD_SERIAL         "123456"
#define USBD_DESC_LEN       (TUD_CONFIG_DESC_LEN + CFG_TUD_VENDOR * TUD_VENDOR_DESC_LEN)
#define USBD_DESC_STR_MAX   (32)
#define USBD_JPEG_STR       "JPEG Stream"
#define USBD_JPEG_EPNUM_OUT (0x01)
#define USBD_JPEG_EPNUM_IN  (0x81)

enum {
    STR_0,
    STR_MANUFACTURER,
    STR_PRODUCT,
    STR_SERIAL,
    STR_VENDOR_JPEG,
};

static const tusb_desc_device_t descriptor_dev = {
    .bLength = sizeof(descriptor_dev),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,

#if CFG_TUD_CDC
    // Use Interface Association Descriptor (IAD) for CDC
    // As required by USB Specs IAD's subclass must be common class (2) and protocol must be IAD (1)
    .bDeviceClass = TUSB_CLASS_MISC,
    .bDeviceSubClass = MISC_SUBCLASS_COMMON,
    .bDeviceProtocol = MISC_PROTOCOL_IAD,
#else
    .bDeviceClass = 0x00,
    .bDeviceSubClass = 0x00,
    .bDeviceProtocol = 0x00,
#endif

    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,
    .idVendor = USBD_VID,
    .idProduct = USBD_PID,
    .bcdDevice = 0x0100,

    .iManufacturer = STR_MANUFACTURER,
    .iProduct = STR_PRODUCT,
    .iSerialNumber = STR_SERIAL,

    .bNumConfigurations = 0x01
};

#if (TUD_OPT_HIGH_SPEED)
static const tusb_desc_device_qualifier_t descriptor_qualifier = {
    .bLength = sizeof(tusb_desc_device_qualifier_t),
    .bDescriptorType = TUSB_DESC_DEVICE_QUALIFIER,
    .bcdUSB = 0x0200,

#if CFG_TUD_CDC
    // Use Interface Association Descriptor (IAD) for CDC
    // As required by USB Specs IAD's subclass must be common class (2) and protocol must be IAD (1)
    .bDeviceClass = TUSB_CLASS_MISC,
    .bDeviceSubClass = MISC_SUBCLASS_COMMON,
    .bDeviceProtocol = MISC_PROTOCOL_IAD,
#else
    .bDeviceClass = 0x00,
    .bDeviceSubClass = 0x00,
    .bDeviceProtocol = 0x00,
#endif

    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,
    .bNumConfigurations = 0x01,
    .bReserved = 0
};
#endif // TUD_OPT_HIGH_SPEED

static uint8_t const descriptor_config[] = {
    // Config Header
    TUD_CONFIG_DESCRIPTOR(1, 1, STR_0, USBD_DESC_LEN, 0x00, 100),

    // Vendor Interface
    TUD_VENDOR_DESCRIPTOR(0, STR_VENDOR_JPEG, USBD_JPEG_EPNUM_OUT, USBD_JPEG_EPNUM_IN, 512)
};

static const char *descriptor_string[] = {
    [STR_MANUFACTURER] = USBD_MANUFACTURER,
    [STR_PRODUCT     ] = USBD_PRODUCT,
    [STR_SERIAL      ] = USBD_SERIAL,
    [STR_VENDOR_JPEG ] = USBD_JPEG_STR,
};

uint8_t const *tud_descriptor_device_cb(void) {
    return (uint8_t const*)&descriptor_dev;
}

uint8_t const *tud_descriptor_configuration_cb(uint8_t index) {
    return descriptor_config;
}

uint16_t const *tud_descriptor_string_cb(uint8_t index, uint16_t langid) {
    static uint16_t buf[USBD_DESC_STR_MAX];
    uint8_t len;

    if (index == 0) {
        buf[1] = 0x0409;
        len = 1;
    } else {
        const char *str = descriptor_string[index];
        for (len = 0; len < USBD_DESC_STR_MAX - 1 && str[len]; len++) {
            buf[1 + len] = str[len];
        }
    }
    buf[0] = (uint16_t)((TUSB_DESC_STRING << 8) | (2 * len + 2));
    return buf;
}

#if (TUD_OPT_HIGH_SPEED)
uint8_t const *tud_descriptor_device_qualifier_cb(void) {
    return (uint8_t const *)&descriptor_qualifier;
}

uint8_t const *tud_descriptor_other_speed_configuration_cb(uint8_t index) {
    return NULL;
}
#endif // TUD_OPT_HIGH_SPEED

// #pragma once
// #include "tinyusb.h"

// #define TUSB_DESC_TOTAL_LEN (TUD_CONFIG_DESC_LEN + CFG_TUD_VENDOR * TUD_VENDOR_DESC_LEN)
// #define EPNUM_VENDOR_OUT    (0x01)
// #define EPNUM_VENDOR_IN     (0x81)


// tinyusb_config_t tusb_install_cfg = {
//     .fs_configuration_descriptor = tusb_configuration_descriptor,
//     .hs_configuration_descriptor = tusb_configuration_descriptor,
// };

#pragma once
#include "tinyusb.h"

#define TUSB_DESC_TOTAL_LEN (TUD_CONFIG_DESC_LEN + CFG_TUD_VENDOR * TUD_VENDOR_DESC_LEN)
#define EPNUM_VENDOR_OUT    (0x01)
#define EPNUM_VENDOR_IN     (0x81)

uint8_t const tusb_configuration_descriptor[] = {
    // Config Header
    TUD_CONFIG_DESCRIPTOR(1, 1, 0, TUSB_DESC_TOTAL_LEN, 0x00, 100),

    // Vendor Interface
    TUD_VENDOR_DESCRIPTOR(0, 4, EPNUM_VENDOR_OUT, EPNUM_VENDOR_IN, 512)
};

tinyusb_config_t tusb_install_cfg = {
    .fs_configuration_descriptor = tusb_configuration_descriptor,
    .hs_configuration_descriptor = tusb_configuration_descriptor,
};

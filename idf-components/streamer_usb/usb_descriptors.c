#include "tusb.h"
#include "esp_mac.h"
#include "streamer_usb_uac.h"

#define USBD_VID            (0xf055)
#define USBD_PID            (0x1118)
#define USBD_MANUFACTURER   "M5Stack"
#define USBD_PRODUCT        "Tab5 Screen Streamer"
#define USBD_DESC_LEN       (TUD_CONFIG_DESC_LEN \
                             + CFG_TUD_VENDOR * TUD_VENDOR_DESC_LEN \
                             + CFG_TUD_AUDIO * CFG_TUD_AUDIO_FUNC_1_DESC_LEN)
#define USBD_DESC_STR_MAX   (32)
#define USBD_JPEG_STR       "JPEG Stream"
#define USBD_JPEG_EPNUM_OUT (0x01)
#define USBD_JPEG_EPNUM_IN  (0x81)
#define USBD_UAC_CTRL_STR   "Tab5 Speaker"
#define USBD_UAC_SPK_STR    "Speaker"

// AudioControl feature-unit per-channel control bitmap: mute + volume, RW.
#define UAC_FU_CTRL ((uint32_t)(AUDIO_CTRL_RW << AUDIO_FEATURE_UNIT_CTRL_MUTE_POS) \
                   | (uint32_t)(AUDIO_CTRL_RW << AUDIO_FEATURE_UNIT_CTRL_VOLUME_POS))

// Class-Specific AC total length (clock + feature unit + I/O terminals).
#define UAC_CS_AC_TOTAL_LEN ( \
      TUD_AUDIO_DESC_CLK_SRC_LEN \
    + TUD_AUDIO_DESC_FEATURE_UNIT_TWO_CHANNEL_LEN \
    + TUD_AUDIO_DESC_INPUT_TERM_LEN \
    + TUD_AUDIO_DESC_OUTPUT_TERM_LEN )

enum {
    STR_0,
    STR_MANUFACTURER,
    STR_PRODUCT,
    STR_SERIAL,
    STR_VENDOR_JPEG,
    STR_UAC_CTRL,
    STR_UAC_SPK,
};

static const tusb_desc_device_t descriptor_dev = {
    .bLength = sizeof(descriptor_dev),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,

#if CFG_TUD_CDC || CFG_TUD_AUDIO
    // The UAC (and CDC) functions use an Interface Association Descriptor (IAD),
    // so the device must be declared as a composite/IAD device.
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

#if CFG_TUD_CDC || CFG_TUD_AUDIO
    // The UAC (and CDC) functions use an Interface Association Descriptor (IAD),
    // so the device must be declared as a composite/IAD device.
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
    TUD_CONFIG_DESCRIPTOR(1, ITF_NUM_TOTAL, STR_0, USBD_DESC_LEN, 0x00, 100),

    // Vendor Interface (JPEG stream)
    TUD_VENDOR_DESCRIPTOR(ITF_NUM_VENDOR, STR_VENDOR_JPEG, USBD_JPEG_EPNUM_OUT, USBD_JPEG_EPNUM_IN, 512),

    // ---- UAC 2.0 stereo speaker (48 kHz / 16-bit) ----
    /* Standard Interface Association Descriptor (IAD): control + streaming */
    TUD_AUDIO_DESC_IAD(/*_firstitf*/ ITF_NUM_AUDIO_CONTROL, /*_nitfs*/ 2, /*_stridx*/ 0x00),
    /* Standard AC Interface Descriptor (4.7.1) */
    TUD_AUDIO_DESC_STD_AC(/*_itfnum*/ ITF_NUM_AUDIO_CONTROL, /*_nEPs*/ 0x00, /*_stridx*/ STR_UAC_CTRL),
    /* Class-Specific AC Interface Header Descriptor (4.7.2) */
    TUD_AUDIO_DESC_CS_AC(/*_bcdADC*/ 0x0200, /*_category*/ AUDIO_FUNC_DESKTOP_SPEAKER, /*_totallen*/ UAC_CS_AC_TOTAL_LEN, /*_ctrl*/ AUDIO_CS_AS_INTERFACE_CTRL_LATENCY_POS),
    /* Clock Source Descriptor (4.7.2.1): internal fixed clock, freq read-only */
    TUD_AUDIO_DESC_CLK_SRC(/*_clkid*/ UAC2_ENTITY_CLOCK, /*_attr*/ 3, /*_ctrl*/ 7, /*_assocTerm*/ 0x00, /*_stridx*/ 0x00),
    /* Input Terminal Descriptor (4.7.2.4): USB streaming in */
    TUD_AUDIO_DESC_INPUT_TERM(/*_termid*/ UAC2_ENTITY_SPK_INPUT_TERMINAL, /*_termtype*/ AUDIO_TERM_TYPE_USB_STREAMING, /*_assocTerm*/ 0x00, /*_clkid*/ UAC2_ENTITY_CLOCK, /*_nchannelslogical*/ UAC_SPK_CHANNELS, /*_channelcfg*/ AUDIO_CHANNEL_CONFIG_NON_PREDEFINED, /*_idxchannelnames*/ 0x00, /*_ctrl*/ (AUDIO_CTRL_R << AUDIO_IN_TERM_CTRL_CONNECTOR_POS), /*_stridx*/ 0x00),
    /* Feature Unit Descriptor (4.7.2.8): master + L + R, mute & volume */
    TUD_AUDIO_DESC_FEATURE_UNIT_TWO_CHANNEL(/*_unitid*/ UAC2_ENTITY_SPK_FEATURE_UNIT, /*_srcid*/ UAC2_ENTITY_SPK_INPUT_TERMINAL, /*_ctrlch0master*/ UAC_FU_CTRL, /*_ctrlch1*/ UAC_FU_CTRL, /*_ctrlch2*/ UAC_FU_CTRL, /*_stridx*/ 0x00),
    /* Output Terminal Descriptor (4.7.2.5): generic speaker */
    TUD_AUDIO_DESC_OUTPUT_TERM(/*_termid*/ UAC2_ENTITY_SPK_OUTPUT_TERMINAL, /*_termtype*/ AUDIO_TERM_TYPE_OUT_GENERIC_SPEAKER, /*_assocTerm*/ 0x00, /*_srcid*/ UAC2_ENTITY_SPK_FEATURE_UNIT, /*_clkid*/ UAC2_ENTITY_CLOCK, /*_ctrl*/ 0x0000, /*_stridx*/ 0x00),
    /* Standard AS Interface Descriptor (4.9.1): Alt 0 - zero-bandwidth */
    TUD_AUDIO_DESC_STD_AS_INT(/*_itfnum*/ ITF_NUM_AUDIO_STREAMING, /*_altset*/ 0x00, /*_nEPs*/ 0x00, /*_stridx*/ STR_UAC_SPK),
    /* Standard AS Interface Descriptor (4.9.1): Alt 1 - streaming (1 EP, no feedback) */
    TUD_AUDIO_DESC_STD_AS_INT(/*_itfnum*/ ITF_NUM_AUDIO_STREAMING, /*_altset*/ 0x01, /*_nEPs*/ 0x01, /*_stridx*/ STR_UAC_SPK),
    /* Class-Specific AS Interface Descriptor (4.9.2): PCM */
    TUD_AUDIO_DESC_CS_AS_INT(/*_termid*/ UAC2_ENTITY_SPK_INPUT_TERMINAL, /*_ctrl*/ AUDIO_CTRL_NONE, /*_formattype*/ AUDIO_FORMAT_TYPE_I, /*_formats*/ AUDIO_DATA_FORMAT_TYPE_I_PCM, /*_nchannelsphysical*/ UAC_SPK_CHANNELS, /*_channelcfg*/ AUDIO_CHANNEL_CONFIG_NON_PREDEFINED, /*_stridx*/ 0x00),
    /* Type I Format Type Descriptor (2.3.1.6) */
    TUD_AUDIO_DESC_TYPE_I_FORMAT(/*_subslotsize*/ UAC_BYTES_PER_SAMPLE, /*_bitresolution*/ UAC_BIT_RESOLUTION),
    /* Standard AS ISO Data Endpoint Descriptor (4.10.1.1): OUT, adaptive */
    TUD_AUDIO_DESC_STD_AS_ISO_EP(/*_ep*/ EPNUM_AUDIO_OUT, /*_attr*/ (TUSB_XFER_ISOCHRONOUS | TUSB_ISO_EP_ATT_ADAPTIVE | TUSB_ISO_EP_ATT_DATA), /*_maxEPsize*/ UAC_EP_SZ_OUT, /*_interval*/ 1),
    /* Class-Specific AS ISO Data Endpoint Descriptor (4.10.1.2) */
    TUD_AUDIO_DESC_CS_AS_ISO_EP(/*_attr*/ AUDIO_CS_AS_ISO_DATA_EP_ATT_NON_MAX_PACKETS_OK, /*_ctrl*/ AUDIO_CTRL_NONE, /*_lockdelayunit*/ AUDIO_CS_AS_ISO_DATA_EP_LOCK_DELAY_UNIT_MILLISEC, /*_lockdelay*/ 0x0001),
};

static char serial[13];
static const char *descriptor_string[] = {
    [STR_MANUFACTURER] = USBD_MANUFACTURER,
    [STR_PRODUCT     ] = USBD_PRODUCT,
    [STR_SERIAL      ] = serial,
    [STR_VENDOR_JPEG ] = USBD_JPEG_STR,
    [STR_UAC_CTRL    ] = USBD_UAC_CTRL_STR,
    [STR_UAC_SPK     ] = USBD_UAC_SPK_STR,
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

    if (!serial[0]) {
        uint8_t mac[6];
        esp_efuse_mac_get_default(mac);
        snprintf(serial, sizeof(serial), "%02X%02X%02X%02X%02X%02X", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
    }

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

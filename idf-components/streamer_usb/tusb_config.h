#pragma once

#include "tusb_option.h"
#include "sdkconfig.h"
#include "streamer_usb_uac.h"

#ifdef __cplusplus
extern "C" {
#endif

#define CONFIG_TINYUSB_CDC_ENABLED 0
#define CONFIG_TINYUSB_CDC_COUNT 0
#define CONFIG_TINYUSB_MSC_ENABLED 0
#define CONFIG_TINYUSB_HID_COUNT 0
#define CONFIG_TINYUSB_MIDI_COUNT 0
#define CONFIG_TINYUSB_VENDOR_COUNT 1

#define CONFIG_TINYUSB_NET_MODE_ECM_RNDIS 0
#define CONFIG_TINYUSB_NET_MODE_NCM 0
#define CONFIG_TINYUSB_DFU_MODE_DFU 0
#define CONFIG_TINYUSB_DFU_MODE_DFU_RUNTIME 0
#define CONFIG_TINYUSB_BTH_ENABLED 0
#define CONFIG_TINYUSB_BTH_ISO_ALT_COUNT 0
#define CONFIG_TINYUSB_DEBUG_LEVEL 0
// #define CONFIG_TINYUSB_DEBUG_LEVEL 2
#define CONFIG_TINYUSB_MODE_DMA 1

#define CFG_TUD_ENABLED                 1       // TinyUSB Device enabled
#define CFG_TUD_MAX_SPEED               OPT_MODE_HIGH_SPEED
#define CFG_TUSB_RHPORT1_MODE           OPT_MODE_DEVICE | OPT_MODE_HIGH_SPEED

// ------------------------------------------------------------------------
//                              DCD DWC2 Mode
// ------------------------------------------------------------------------
#define CFG_TUD_DWC2_SLAVE_ENABLE   1       // Enable Slave/IRQ by default

// ------------------------------------------------------------------------
//                              DMA & Cache
// ------------------------------------------------------------------------
#ifdef CONFIG_TINYUSB_MODE_DMA
// DMA Mode has a priority over Slave/IRQ mode and will be used if hardware supports it
#define CFG_TUD_DWC2_DMA_ENABLE     1       // Enable DMA

#if CONFIG_CACHE_L1_CACHE_LINE_SIZE
// To enable the dcd_dcache clean/invalidate/clean_invalidate calls
#   define CFG_TUD_MEM_DCACHE_ENABLE    1
#define CFG_TUD_MEM_DCACHE_LINE_SIZE    CONFIG_CACHE_L1_CACHE_LINE_SIZE
// NOTE: starting with esp-idf v5.3 there is specific attribute present: DRAM_DMA_ALIGNED_ATTR
#   define CFG_TUSB_MEM_SECTION         __attribute__((aligned(CONFIG_CACHE_L1_CACHE_LINE_SIZE))) DRAM_ATTR
#else
#   define CFG_TUD_MEM_CACHE_ENABLE     0
#   define CFG_TUSB_MEM_SECTION         TU_ATTR_ALIGNED(4) DRAM_ATTR
#endif // CONFIG_CACHE_L1_CACHE_LINE_SIZE
#endif // CONFIG_TINYUSB_MODE_DMA

#define CFG_TUSB_OS                 OPT_OS_FREERTOS

/* USB DMA on some MCUs can only access a specific SRAM region with restriction on alignment.
 * Tinyusb use follows macros to declare transferring memory so that they can be put
 * into those specific section.
 * e.g
 * - CFG_TUSB_MEM SECTION : __attribute__ (( section(".usb_ram") ))
 * - CFG_TUSB_MEM_ALIGN   : __attribute__ ((aligned(4)))
 */
#ifndef CFG_TUSB_MEM_SECTION
#   define CFG_TUSB_MEM_SECTION
#endif

#ifndef CFG_TUSB_MEM_ALIGN
#   define CFG_TUSB_MEM_ALIGN       TU_ATTR_ALIGNED(4)
#endif

#ifndef CFG_TUD_ENDPOINT0_SIZE
#define CFG_TUD_ENDPOINT0_SIZE      64
#endif

// Debug Level
#define CFG_TUSB_DEBUG              CONFIG_TINYUSB_DEBUG_LEVEL
#define CFG_TUSB_DEBUG_PRINTF       esp_rom_printf // TinyUSB can print logs from ISR, so we must use esp_rom_printf()

// CDC FIFO size of TX and RX
#define CFG_TUD_CDC_RX_BUFSIZE      CONFIG_TINYUSB_CDC_RX_BUFSIZE
#define CFG_TUD_CDC_TX_BUFSIZE      CONFIG_TINYUSB_CDC_TX_BUFSIZE
#define CFG_TUD_CDC_EP_BUFSIZE      CONFIG_TINYUSB_CDC_EP_BUFSIZE

// MSC Buffer size of Device Mass storage
#define CFG_TUD_MSC_BUFSIZE         CONFIG_TINYUSB_MSC_BUFSIZE

// MIDI macros
#define CFG_TUD_MIDI_EP_BUFSIZE     64
#define CFG_TUD_MIDI_EPSIZE         CFG_TUD_MIDI_EP_BUFSIZE
#define CFG_TUD_MIDI_RX_BUFSIZE     64
#define CFG_TUD_MIDI_TX_BUFSIZE     64

// Vendor FIFO size of TX and RX
#define CFG_TUD_VENDOR_RX_BUFSIZE 8192
#define CFG_TUD_VENDOR_TX_BUFSIZE (TUD_OPT_HIGH_SPEED ? 512 : 64)

// Endpoint DMA buffer size. MUST match the bulk max packet size declared in the
// vendor descriptor (512 at high speed). The default is 64, which leaves the
// per-endpoint DMA buffer too small for HS bulk packets — the controller then
// writes past it and corrupts whatever follows in memory.
#define CFG_TUD_VENDOR_EPSIZE     (TUD_OPT_HIGH_SPEED ? 512 : 64)

// DFU macros
#define CFG_TUD_DFU_XFER_BUFSIZE    CONFIG_TINYUSB_DFU_BUFSIZE

// Number of BTH ISO alternatives
#define CFG_TUD_BTH_ISO_ALT_COUNT   CONFIG_TINYUSB_BTH_ISO_ALT_COUNT

// Enabled device class driver
#define CFG_TUD_CDC                 CONFIG_TINYUSB_CDC_COUNT
#define CFG_TUD_MSC                 CONFIG_TINYUSB_MSC_ENABLED
#define CFG_TUD_HID                 CONFIG_TINYUSB_HID_COUNT
#define CFG_TUD_MIDI                CONFIG_TINYUSB_MIDI_COUNT
#define CFG_TUD_VENDOR              CONFIG_TINYUSB_VENDOR_COUNT
#define CFG_TUD_AUDIO               1     // UAC 2.0 speaker (see streamer_usb_uac.h)
#define CFG_TUD_ECM_RNDIS           CONFIG_TINYUSB_NET_MODE_ECM_RNDIS
#define CFG_TUD_NCM                 CONFIG_TINYUSB_NET_MODE_NCM
#define CFG_TUD_DFU                 CONFIG_TINYUSB_DFU_MODE_DFU
#define CFG_TUD_DFU_RUNTIME         CONFIG_TINYUSB_DFU_MODE_DFU_RUNTIME
#define CFG_TUD_BTH                 CONFIG_TINYUSB_BTH_ENABLED

// NCM NET Mode NTB buffers configuration
#define CFG_TUD_NCM_OUT_NTB_N         CONFIG_TINYUSB_NCM_OUT_NTB_BUFFS_COUNT
#define CFG_TUD_NCM_IN_NTB_N          CONFIG_TINYUSB_NCM_IN_NTB_BUFFS_COUNT
#define CFG_TUD_NCM_OUT_NTB_MAX_SIZE  CONFIG_TINYUSB_NCM_OUT_NTB_BUFF_MAX_SIZE
#define CFG_TUD_NCM_IN_NTB_MAX_SIZE   CONFIG_TINYUSB_NCM_IN_NTB_BUFF_MAX_SIZE

// ------------------------------------------------------------------------
//                          AUDIO CLASS (UAC 2.0)
// ------------------------------------------------------------------------
// One audio function: a 48 kHz / stereo / 16-bit speaker (OUT) with an async
// feedback endpoint. The *_DESC_LEN macros below come from device/usbd.h and
// are expanded lazily (when the audio driver / descriptor uses them), so it is
// fine that usbd.h is not yet included at this point.
#define CFG_TUD_AUDIO_FUNC_1_DESC_LEN ( \
      TUD_AUDIO_DESC_IAD_LEN \
    + TUD_AUDIO_DESC_STD_AC_LEN \
    + TUD_AUDIO_DESC_CS_AC_LEN \
    + TUD_AUDIO_DESC_CLK_SRC_LEN \
    + TUD_AUDIO_DESC_FEATURE_UNIT_TWO_CHANNEL_LEN \
    + TUD_AUDIO_DESC_INPUT_TERM_LEN \
    + TUD_AUDIO_DESC_OUTPUT_TERM_LEN \
    + TUD_AUDIO_DESC_STD_AS_INT_LEN /* alt 0 */ \
    + TUD_AUDIO_DESC_STD_AS_INT_LEN /* alt 1 */ \
    + TUD_AUDIO_DESC_CS_AS_INT_LEN \
    + TUD_AUDIO_DESC_TYPE_I_FORMAT_LEN \
    + TUD_AUDIO_DESC_STD_AS_ISO_EP_LEN \
    + TUD_AUDIO_DESC_CS_AS_ISO_EP_LEN )

#define CFG_TUD_AUDIO_FUNC_1_N_AS_INT             1
#define CFG_TUD_AUDIO_FUNC_1_CTRL_BUF_SZ          64

// Speaker = host-to-device = OUT endpoint. Adaptive sync (the device follows
// the host clock), so no feedback endpoint is needed.
#define CFG_TUD_AUDIO_ENABLE_EP_OUT               1
#define CFG_TUD_AUDIO_ENABLE_FEEDBACK_EP          0

#define CFG_TUD_AUDIO_FUNC_1_EP_OUT_SZ_MAX        UAC_EP_SZ_OUT
#define CFG_TUD_AUDIO_FUNC_1_EP_OUT_SW_BUF_SZ     UAC_EP_OUT_SW_BUF_SZ

#ifdef __cplusplus
}
#endif

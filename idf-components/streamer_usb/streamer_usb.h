#pragma once
#include "esp_err.h"
#include "stdbool.h"
#include "stdint.h"

#ifdef __cplusplus
extern "C" {
#endif

// Initialization
esp_err_t streamer_usb_init(void);
void streamer_usb_task(void);

// Common
bool streamer_usb_mounted(void);

// vendor specific class
uint32_t streamer_usb_vendor_available(void);
uint32_t streamer_usb_vendor_read(void *buffer, uint32_t bufsize);

#ifdef __cplusplus
}
#endif

#pragma once
#include "esp_err.h"
#include "stdbool.h"
#include "stdint.h"

// Initialization
esp_err_t usbd_init(void);
void usbd_task(void);

// Common
bool usbd_mounted(void);

// vendor specific class
uint32_t usbd_vendor_available(void);
uint32_t usbd_vendor_read(void *buffer, uint32_t bufsize);

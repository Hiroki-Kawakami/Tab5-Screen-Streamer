#include "usbd.h"
#include "esp_check.h"
#include "esp_err.h"
#include "esp_private/usb_phy.h"
#include "tusb.h"

const static char *TAG = "USBDevice";

// Initialization
esp_err_t usbd_init(void) {
    // Configure USB PHY
    usb_phy_handle_t phy_hdl = NULL;
    usb_phy_config_t phy_conf = {
        .controller = USB_PHY_CTRL_OTG,
        .target = USB_PHY_TARGET_UTMI,
        .otg_mode = USB_OTG_MODE_DEVICE,
        .otg_speed = USB_PHY_SPEED_HIGH,
    };
    ESP_RETURN_ON_ERROR(usb_new_phy(&phy_conf, &phy_hdl), TAG, "Install USB PHY failed");

    tusb_init();
    return ESP_OK;
}
void usbd_task(void) {
    while (true) {
        tud_task();
    }
}

bool usbd_mounted(void) { return tud_mounted(); }

// Vendor specific class
uint32_t usbd_vendor_available(void) { return tud_vendor_available(); }
uint32_t usbd_vendor_read(void *buffer, uint32_t bufsize) { return tud_vendor_read(buffer, bufsize); }

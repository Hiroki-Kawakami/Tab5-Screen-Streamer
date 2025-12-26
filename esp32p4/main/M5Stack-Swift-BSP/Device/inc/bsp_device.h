#include "esp_ldo_regulator.h"
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_panel_io.h"
#include "esp_lcd_mipi_dsi.h"
#include "ili9881_init_data.h"
#include "st7123_init_data.h"
#include "esp_lcd_touch_gt911.h"
#include "esp_lcd_touch_st7123.h"
#include "sd_pwr_ctrl_by_on_chip_ldo.h"
#include "usb/usb_host.h"

// MIPI DSI
esp_err_t esp_lcd_dpi_panel_get_frame_buffers(esp_lcd_panel_handle_t panel, uint32_t fb_num, void **fb0, void **fb1, void **fb2) {
    switch (fb_num) {
    case 1: return esp_lcd_dpi_panel_get_frame_buffer(panel, 1, fb0);
    case 2: return esp_lcd_dpi_panel_get_frame_buffer(panel, 2, fb0, fb1);
    case 3: return esp_lcd_dpi_panel_get_frame_buffer(panel, 3, fb0, fb1, fb2);
    }
    return ESP_ERR_INVALID_ARG;
}

// GT911
void _ESP_LCD_TOUCH_IO_I2C_GT911_CONFIG(esp_lcd_panel_io_i2c_config_t *ptr) {
    *ptr = (esp_lcd_panel_io_i2c_config_t)ESP_LCD_TOUCH_IO_I2C_GT911_CONFIG();
}

// ST7123
void _ESP_LCD_TOUCH_IO_I2C_ST7123_CONFIG(esp_lcd_panel_io_i2c_config_t *ptr) {
    *ptr = (esp_lcd_panel_io_i2c_config_t)ESP_LCD_TOUCH_IO_I2C_ST7123_CONFIG();
}
